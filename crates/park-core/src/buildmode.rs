//! Build mode: the on-screen coaster builder's brain (M3b T4, D75).
//!
//! One build session for the host player (LAN guests keep the wand, C4). It remembers where the
//! track ends (the "head"), the settings of the next piece, what was built (so Z undoes and Y
//! redoes), and the finish helper's proposal. It also writes the text of the HUD panel.
//!
//! Nothing here touches the game: the glue passes keys in, the runtime passes the results on.
//!
//! The flow, in order:
//!   1. **B** opens the panel. With no coaster yet it shows a menu; right-click starts "New coaster".
//!   2. Wheel changes the highlighted setting, **R** highlights the next one.
//!   3. Right-click builds the piece. **Z** undoes it, **Y** redoes it.
//!   4. **H** asks the finish helper for a path back to the station; right-click accepts it and
//!      the ride is complete; any other key cancels it.

use crate::builder::{cars_of, course_from, node_distances, physics_of, spacing_of};
use crate::coaster::spline::Track;
use crate::coaster::table::Table;
use crate::finish::{self, Finish};
use crate::pieces::{
    Head, MAX_LENGTH, MAX_SLOPE, MAX_TURN, MIN_LENGTH, Piece, PieceSettings, PieceType, SLOPE_STEP,
    TURN_STEP, build_piece, hits_track,
};
use crate::preview::{self, Preview, Setting};
use crate::report::{self, Blocker};
use crate::save::{Kind, NodeFile, RideFile};
use glam::DVec3;

/// The session's owner: the host player.
pub const HOST: u128 = 0;
/// How far in front of the player a new coaster's station starts, and how high above their feet.
pub const START_AHEAD: f64 = 4.0;
pub const START_ABOVE: f64 = 5.0;
/// A new coaster's station: three station nodes, 4 blocks apart (12 blocks, room for 3 cars, D55).
const STATION_NODES: usize = 3;
const STATION_GAP: f64 = 4.0;

/// The settings the wheel can change, one at a time; **R** moves to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Type,
    Length,
    Turn,
    Slope,
    Element,
}

impl Field {
    pub const ALL: [Field; 5] = [
        Field::Type,
        Field::Length,
        Field::Turn,
        Field::Slope,
        Field::Element,
    ];

    fn next(self) -> Field {
        let i = Field::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Field::ALL[(i + 1) % Field::ALL.len()]
    }
}

/// Which screen the panel shows.
#[derive(Clone, Debug, PartialEq)]
enum Screen {
    /// No coaster yet: choose "New coaster".
    Menu,
    /// Building piece by piece.
    Building,
    /// The finish helper's ghost is on show.
    Finishing(Finish),
}

/// What a panel line looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    /// The setting the wheel changes.
    Selected,
    /// Not available (with the reason in the text).
    Grey,
    /// Past a realism limit, or a refusal.
    Red,
    Good,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelLine {
    pub text: String,
    pub tone: Tone,
}

/// Everything the HUD draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub title: String,
    pub lines: Vec<PanelLine>,
}

fn line(text: impl Into<String>, tone: Tone) -> PanelLine {
    PanelLine {
        text: text.into(),
        tone,
    }
}

/// One built piece, kept so it can be undone and redone.
#[derive(Clone, Debug, PartialEq)]
struct Step {
    piece: Piece,
    /// The head before the piece, and the kind its last node had.
    head: Head,
    previous_kind: Kind,
}

/// What happened when the player confirmed (right-click).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Nothing for the world to do (the panel or ghost changed).
    Nothing,
    /// The ride is complete and checked: save and open it.
    Finished(RideFile),
}

/// The ghost to draw: nodes of the track around the join and the new part (see `render`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    /// A few real nodes before the new part, so the curve joins smoothly.
    pub lead_in: Vec<[f64; 3]>,
    /// The new nodes.
    pub new: Vec<[f64; 3]>,
    /// True for the finish helper's path (it is drawn the same way, S3).
    pub is_finish: bool,
}

/// The preview for one state of the builder.
impl Ghost {
    /// The rail pieces to draw for this ghost: only the new stretch, from the old end of the
    /// track to the new one. The curve is worked out through the last real nodes, the new
    /// nodes and two invisible ones straight ahead, so it joins the real track smoothly.
    pub fn poses(&self) -> Vec<crate::render::RailPose> {
        let v = |p: [f64; 3]| DVec3::new(p[0], p[1], p[2]);
        let mut points: Vec<DVec3> = self.lead_in.iter().copied().map(v).collect();
        let head_index = points.len().saturating_sub(1);
        points.extend(self.new.iter().copied().map(v));
        let end_index = points.len() - 1;
        let n = points.len();
        if n < 4 || head_index == 0 {
            return Vec::new();
        }
        let ahead = (points[n - 1] - points[n - 2]).normalize_or_zero();
        for k in 1..=2 {
            points.push(points[end_index] + ahead * (3.0 * k as f64));
        }
        // The loop closes through the phantoms; only the new stretch is read.
        let track = Track::closed(&points);
        let table = Table::new_level(&track);
        if table.is_empty() {
            return Vec::new();
        }
        let node_s = node_distances(&track, &table);
        crate::render::rail_pieces_between(&table, node_s[head_index], node_s[end_index])
            .into_iter()
            .map(|p| crate::render::rail_pose(&table, p))
            .collect()
    }
}

#[derive(Clone, Debug)]
struct Cached {
    version: u64,
    settings: PieceSettings,
    preview: Option<Preview>,
    /// Why the current value of the highlighted number is blocked, if it is.
    blocked: Vec<Option<String>>,
}

#[derive(Clone, Debug)]
pub struct BuildMode {
    open: bool,
    screen: Screen,
    ride: Option<RideFile>,
    head: Head,
    settings: PieceSettings,
    field: Field,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Count of changes to the track or the ghost; the runtime redraws the ghost when it moves on.
    pub version: u64,
    /// A short message shown on the panel (a refusal, or "Built").
    message: Option<(String, Tone)>,
    /// The last preview and what it was for, so the HUD (every frame) does not re-run it.
    cache: Option<Cached>,
}

impl Default for BuildMode {
    fn default() -> Self {
        BuildMode::new()
    }
}

impl BuildMode {
    /// A closed panel, no coaster. `const`, so the runtime (kept in a `static`) can start with it.
    pub const fn new() -> BuildMode {
        BuildMode {
            open: false,
            screen: Screen::Menu,
            ride: None,
            head: Head {
                pos: DVec3::ZERO,
                heading: 0.0,
                slope: 0.0,
            },
            settings: PieceSettings {
                piece: PieceType::Straight,
                length: 8,
                turn: 0,
                slope: 0,
                element: Kind::Track,
            },
            field: Field::Type,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            message: None,
            cache: None,
        }
    }

    /// Is the panel on screen?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Forget everything (a new world opened).
    pub fn reset(&mut self) {
        *self = BuildMode::new();
    }

    fn changed(&mut self) {
        self.version += 1;
        self.cache = None;
    }

    /// **B**: open or close the panel. Closing keeps the unfinished coaster for next time.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.message = None;
        if let Screen::Finishing(_) = self.screen {
            self.screen = Screen::Building;
        }
        self.changed();
    }

    /// Any key other than right-click while the finish ghost shows: cancel it.
    pub fn cancel_finish(&mut self) {
        if let Screen::Finishing(_) = self.screen {
            self.screen = Screen::Building;
            self.message = None;
            self.changed();
        }
    }

    /// True while the finish helper's proposal is on show.
    pub fn is_finishing(&self) -> bool {
        matches!(self.screen, Screen::Finishing(_))
    }

    /// **R**: highlight the next setting.
    pub fn next_field(&mut self) {
        if self.screen == Screen::Building {
            self.field = self.field.next();
            self.message = None;
            self.cache = None;
        }
    }

    /// The mouse wheel: change the highlighted setting by `steps` steps (positive = up).
    pub fn wheel(&mut self, steps: f32) {
        if self.screen != Screen::Building {
            return;
        }
        let n = (steps.abs().round() as i32).max(1) * if steps < 0.0 { -1 } else { 1 };
        let s = &mut self.settings;
        match self.field {
            Field::Type => {
                let all = PieceType::ALL;
                let i = all.iter().position(|p| *p == s.piece).unwrap_or(0) as i32;
                s.piece = all[(i + n).rem_euclid(all.len() as i32) as usize];
            }
            Field::Length => {
                s.length = (s.length as i32 + n).clamp(MIN_LENGTH as i32, MAX_LENGTH as i32) as u32;
            }
            Field::Turn => s.turn = (s.turn + n * TURN_STEP).clamp(-MAX_TURN, MAX_TURN),
            Field::Slope => s.slope = (s.slope + n * SLOPE_STEP).clamp(-MAX_SLOPE, MAX_SLOPE),
            Field::Element => {
                const ELEMENTS: [Kind; 3] = [Kind::Track, Kind::Lift, Kind::Brake];
                let i = ELEMENTS.iter().position(|k| *k == s.element).unwrap_or(0) as i32;
                s.element = ELEMENTS[(i + n).rem_euclid(3) as usize];
            }
        }
        self.message = None;
        self.changed();
    }

    /// Start "New coaster": a station 4 blocks in front of the player, level with 5 blocks above
    /// their feet, pointing the way they look (snapped to the nearest compass direction so the
    /// platform of blocks lines up). `yaw` is Minecraft's yaw in degrees.
    pub fn new_coaster(&mut self, name: &str, feet: DVec3, yaw: f64) {
        let heading = (yaw / 90.0).round() * 90.0;
        let forward = Head {
            pos: DVec3::ZERO,
            heading,
            slope: 0.0,
        }
        .direction();
        let first = DVec3::new(
            feet.x.floor() + 0.5,
            feet.y.floor() + START_ABOVE,
            feet.z.floor() + 0.5,
        ) + forward * START_AHEAD;
        let mut nodes = Vec::new();
        for i in 0..=STATION_NODES {
            let p = first + forward * (STATION_GAP * i as f64);
            nodes.push(NodeFile {
                pos: [p.x, p.y, p.z],
                roll: 0.0,
                kind: if i < STATION_NODES {
                    Kind::Station
                } else {
                    Kind::Track
                },
            });
        }
        self.head = Head {
            pos: first + forward * (STATION_GAP * STATION_NODES as f64),
            heading,
            slope: 0.0,
        };
        self.ride = Some(RideFile::new(name, nodes));
        self.screen = Screen::Building;
        self.undo.clear();
        self.redo.clear();
        self.settings = PieceSettings::straight(8);
        self.field = Field::Type;
        self.message = Some((
            "Station placed. Scroll to choose a piece, right-click to build it.".to_string(),
            Tone::Good,
        ));
        self.changed();
    }

    /// The ride under construction (to draw a station platform, or to save).
    pub fn ride(&self) -> Option<&RideFile> {
        self.ride.as_ref()
    }

    /// Right-click while in the menu needs the player's place; the runtime calls `new_coaster`
    /// for that. Everything else is here: build a piece, or accept the finish.
    pub fn confirm(&mut self, exists: impl Fn(&str) -> bool) -> Outcome {
        match self.screen.clone() {
            Screen::Menu => Outcome::Nothing,
            Screen::Building => {
                self.build();
                Outcome::Nothing
            }
            Screen::Finishing(plan) => self.accept_finish(&plan, exists),
        }
    }

    /// Is the menu showing (the next right-click starts a new coaster)?
    pub fn in_menu(&self) -> bool {
        self.screen == Screen::Menu
    }

    fn build(&mut self) {
        let Some(ride) = self.ride.as_mut() else {
            return;
        };
        let piece = match build_piece(&self.head, &self.settings) {
            Ok(p) => p,
            Err(why) => {
                self.message = Some((why, Tone::Red));
                self.version += 1;
                return;
            }
        };
        if let Some(n) = hits_track(&ride.nodes, &piece.nodes, false) {
            self.message = Some((
                format!("Would hit your track near node {n}: move it away or raise it."),
                Tone::Red,
            ));
            self.version += 1;
            return;
        }
        let last = ride.nodes.len() - 1;
        let step = Step {
            piece: piece.clone(),
            head: self.head,
            previous_kind: ride.nodes[last].kind,
        };
        ride.nodes[last].kind = piece.start_kind;
        ride.nodes.extend(piece.nodes.iter().cloned());
        self.head = piece.end;
        self.undo.push(step);
        self.redo.clear();
        self.message = Some(("Built.".to_string(), Tone::Good));
        self.changed();
    }

    /// **Z**.
    pub fn undo(&mut self) {
        if self.screen != Screen::Building {
            return;
        }
        let Some(step) = self.undo.pop() else {
            self.message = Some(("Nothing to undo.".to_string(), Tone::Grey));
            self.version += 1;
            return;
        };
        if let Some(ride) = self.ride.as_mut() {
            let keep = ride.nodes.len() - step.piece.nodes.len();
            ride.nodes.truncate(keep);
            ride.nodes[keep - 1].kind = step.previous_kind;
        }
        self.head = step.head;
        self.redo.push(step);
        self.message = Some(("Undone.".to_string(), Tone::Normal));
        self.changed();
    }

    /// **Y**.
    pub fn redo(&mut self) {
        if self.screen != Screen::Building {
            return;
        }
        let Some(step) = self.redo.pop() else {
            self.message = Some(("Nothing to redo.".to_string(), Tone::Grey));
            self.version += 1;
            return;
        };
        if let Some(ride) = self.ride.as_mut() {
            let last = ride.nodes.len() - 1;
            ride.nodes[last].kind = step.piece.start_kind;
            ride.nodes.extend(step.piece.nodes.iter().cloned());
        }
        self.head = step.piece.end;
        self.undo.push(step);
        self.message = Some(("Redone.".to_string(), Tone::Normal));
        self.changed();
    }

    /// **H**: ask the finish helper; show its ghost, or the reason it can't.
    pub fn finish(&mut self) {
        match self.screen {
            Screen::Finishing(_) => self.cancel_finish(),
            Screen::Building => {
                let Some(ride) = self.ride.as_ref() else {
                    return;
                };
                match finish::finish(ride, &self.head) {
                    Ok(plan) => {
                        self.message = Some((
                            "Finish path ready. Right-click to accept, any other key to cancel."
                                .to_string(),
                            Tone::Good,
                        ));
                        self.screen = Screen::Finishing(plan);
                    }
                    Err(why) => self.message = Some((why, Tone::Red)),
                }
                self.changed();
            }
            Screen::Menu => {}
        }
    }

    /// Right-click on the finish ghost: add it, close the loop, and check the ride like `create`.
    fn accept_finish(&mut self, plan: &Finish, exists: impl Fn(&str) -> bool) -> Outcome {
        let Some(ride) = self.ride.as_ref() else {
            return Outcome::Nothing;
        };
        let mut done = ride.clone();
        done.nodes.extend(plan.nodes.iter().cloned());
        if exists(&done.id) {
            self.message = Some((
                format!("A ride called {} already exists.", done.id),
                Tone::Red,
            ));
            self.changed();
            return Outcome::Nothing;
        }
        // The same checks as `/coaster create` (D65, D68), so the screen never promises more.
        let course = match course_from(&done) {
            Ok(c) => c,
            Err(why) => {
                self.message = Some((why, Tone::Red));
                self.changed();
                return Outcome::Nothing;
            }
        };
        if let Some(blocker) = report::blocker(
            &course,
            cars_of(&done),
            spacing_of(&done),
            &physics_of(&done),
        ) {
            let why = match blocker {
                Blocker::Overshoot { .. } => "The train would roll past the station's stop point.",
                Blocker::Stall { .. } => "The train would stall before it gets back.",
            };
            self.message = Some((why.to_string(), Tone::Red));
            self.changed();
            return Outcome::Nothing;
        }
        // The coaster is complete: the next B starts a fresh menu.
        self.ride = None;
        self.screen = Screen::Menu;
        self.undo.clear();
        self.redo.clear();
        self.open = false;
        self.message = None;
        self.changed();
        Outcome::Finished(done)
    }

    /// The preview for the current settings, computed only when something changed.
    fn refresh(&mut self) {
        let fresh = self
            .cache
            .as_ref()
            .is_some_and(|c| c.version == self.version && c.settings == self.settings);
        if fresh {
            return;
        }
        let Some(ride) = self.ride.as_ref() else {
            return;
        };
        let piece = build_piece(&self.head, &self.settings).ok();
        let preview = preview::preview(ride, piece.as_ref()).ok();
        // Which values of the highlighted number would be blocked? (Shown for the current one.)
        let blocked = match self.field {
            Field::Length => {
                let v = self.settings.length as i32;
                preview::blocked_values(ride, &self.head, &self.settings, Setting::Length, &[v])
            }
            Field::Turn => preview::blocked_values(
                ride,
                &self.head,
                &self.settings,
                Setting::Turn,
                &[self.settings.turn],
            ),
            Field::Slope => preview::blocked_values(
                ride,
                &self.head,
                &self.settings,
                Setting::Slope,
                &[self.settings.slope],
            ),
            _ => Vec::new(),
        };
        self.cache = Some(Cached {
            version: self.version,
            settings: self.settings,
            preview,
            blocked,
        });
    }

    /// What the ghost looks like, for the runtime to draw (None when nothing is on show).
    pub fn ghost(&self) -> Option<Ghost> {
        if !self.open {
            return None;
        }
        let ride = self.ride.as_ref()?;
        let lead_in: Vec<[f64; 3]> = ride
            .nodes
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(|n| n.pos)
            .collect();
        match &self.screen {
            Screen::Building => {
                let piece = build_piece(&self.head, &self.settings).ok()?;
                Some(Ghost {
                    lead_in,
                    new: piece.nodes.iter().map(|n| n.pos).collect(),
                    is_finish: false,
                })
            }
            Screen::Finishing(plan) => Some(Ghost {
                lead_in,
                new: plan.nodes.iter().map(|n| n.pos).collect(),
                is_finish: true,
            }),
            Screen::Menu => None,
        }
    }

    /// The panel text. `None` when the panel is closed.
    pub fn panel(&mut self) -> Option<Panel> {
        if !self.open {
            return None;
        }
        let mut lines = Vec::new();
        match self.screen.clone() {
            Screen::Menu => {
                lines.push(line("> New coaster", Tone::Selected));
                lines.push(line("Right-click to place a station", Tone::Normal));
                lines.push(line("in front of you.", Tone::Normal));
                lines.push(line("B closes this panel.", Tone::Grey));
                return Some(Panel {
                    title: "Coaster builder".to_string(),
                    lines,
                });
            }
            Screen::Finishing(plan) => {
                lines.push(line(
                    format!(
                        "Curve {:.0} blocks, then a {:.0}-block brake run.",
                        plan.curve_length, plan.brake_length
                    ),
                    Tone::Good,
                ));
                lines.push(line("Right-click: accept and open the ride", Tone::Normal));
                lines.push(line("Any other key: cancel", Tone::Grey));
                if let Some((text, tone)) = &self.message {
                    lines.push(line(text.clone(), *tone));
                }
                return Some(Panel {
                    title: "Finish helper".to_string(),
                    lines,
                });
            }
            Screen::Building => {}
        }
        self.refresh();
        let s = self.settings;
        let sel = |f: Field| {
            if self.field == f {
                Tone::Selected
            } else {
                Tone::Normal
            }
        };
        let mark = |f: Field| if self.field == f { "> " } else { "  " };
        let blocked = self
            .cache
            .as_ref()
            .and_then(|c| c.blocked.first().cloned().flatten());
        let turn_text = match s.turn {
            0 => "straight".to_string(),
            t if t > 0 => format!("{t} degrees right"),
            t => format!("{} degrees left", -t),
        };
        let turn_effective = matches!(s.piece, PieceType::Turn | PieceType::TurnSlope);
        let slope_effective = matches!(s.piece, PieceType::SlopeChange | PieceType::TurnSlope);
        lines.push(line(
            format!("{}Type: {}", mark(Field::Type), s.piece.name()),
            sel(Field::Type),
        ));
        lines.push(line(
            format!("{}Length: {} blocks", mark(Field::Length), s.length),
            sel(Field::Length),
        ));
        let radius = build_piece(&self.head, &s)
            .ok()
            .and_then(|p| p.radius)
            .map(|r| format!(" (radius {r:.0})"))
            .unwrap_or_default();
        lines.push(line(
            format!(
                "{}Turn: {}{}",
                mark(Field::Turn),
                turn_text,
                if turn_effective {
                    radius
                } else {
                    String::new()
                }
            ),
            if turn_effective {
                sel(Field::Turn)
            } else if self.field == Field::Turn {
                Tone::Selected
            } else {
                Tone::Grey
            },
        ));
        lines.push(line(
            format!("{}Slope at end: {} degrees", mark(Field::Slope), s.slope),
            if slope_effective {
                sel(Field::Slope)
            } else if self.field == Field::Slope {
                Tone::Selected
            } else {
                Tone::Grey
            },
        ));
        let element = match s.element {
            Kind::Lift => "Lift",
            Kind::Brake => "Brake",
            _ => "Track",
        };
        lines.push(line(
            format!("{}Element: {}", mark(Field::Element), element),
            sel(Field::Element),
        ));
        lines.push(line("  Banking: comes with M5", Tone::Grey));
        if let Some(why) = blocked {
            lines.push(line(format!("Blocked: {why}"), Tone::Red));
        }
        // Live numbers.
        if let Some(p) = self.cache.as_ref().and_then(|c| c.preview.as_ref()) {
            if let Some(v) = p.end_speed {
                lines.push(line(
                    format!("Speed here: {:.0} km/h", v * 3.6),
                    Tone::Normal,
                ));
            }
            lines.push(line(
                format!("Height here: {:.0}", p.end_height),
                Tone::Normal,
            ));
            if let Some(g) = p.ghost {
                lines.push(line(
                    format!(
                        "Piece: {:.1} to {:.1} g up, {:.1} g side",
                        g.min_vertical, g.max_vertical, g.max_lateral
                    ),
                    if p.ok() { Tone::Normal } else { Tone::Red },
                ));
                lines.push(line(
                    format!("Leaves at {:.0} km/h", g.exit_speed * 3.6),
                    Tone::Normal,
                ));
            }
            for issue in &p.issues {
                lines.push(line(issue.clone(), Tone::Red));
            }
        }
        if let Some((text, tone)) = &self.message {
            lines.push(line(text.clone(), *tone));
        }
        lines.push(line(
            "Wheel: change  R: next  Right-click: build",
            Tone::Grey,
        ));
        lines.push(line("Z undo  Y redo  H finish  B close", Tone::Grey));
        Some(Panel {
            title: "Coaster builder".to_string(),
            lines,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn started() -> BuildMode {
        let mut b = BuildMode::new();
        b.toggle();
        b.new_coaster("coaster-1", DVec3::new(100.3, 64.0, 200.7), 0.0);
        b
    }

    fn no_rides(_: &str) -> bool {
        false
    }

    #[test]
    fn b_opens_a_menu_and_right_click_starts_a_station_in_front_facing_the_view() {
        let mut b = BuildMode::new();
        assert!(!b.is_open());
        assert!(b.panel().is_none());
        b.toggle();
        assert!(b.in_menu());
        assert!(b.panel().unwrap().lines[0].text.contains("New coaster"));
        // Facing 90 (west): the station runs along -X.
        b.new_coaster("coaster-1", DVec3::new(100.3, 64.0, 200.7), 80.0);
        let ride = b.ride().unwrap();
        assert_eq!(ride.nodes.len(), 4);
        assert_eq!(ride.nodes[0].kind, Kind::Station);
        assert_eq!(ride.nodes[3].kind, Kind::Track);
        let (a, z) = (ride.nodes[0].pos, ride.nodes[3].pos);
        assert!((a[0] - (100.5 - START_AHEAD)).abs() < 1e-9, "{a:?}");
        assert!((a[1] - (64.0 + START_ABOVE)).abs() < 1e-9);
        assert!((z[0] - (a[0] - 12.0)).abs() < 1e-9 && (z[2] - a[2]).abs() < 1e-9);
        // And a station that long is fine for 3 cars (D55).
        assert!(
            course_from(&RideFile {
                nodes: {
                    let mut n = ride.nodes.clone();
                    n.push(NodeFile {
                        pos: [z[0] - 4.0, z[1], z[2]],
                        roll: 0.0,
                        kind: Kind::Track,
                    });
                    n
                },
                ..ride.clone()
            })
            .is_ok()
        );
    }

    #[test]
    fn the_wheel_changes_the_highlighted_setting_in_steps_and_r_moves_on() {
        let mut b = started();
        b.wheel(1.0); // Type: Straight -> Turn
        assert_eq!(b.settings.piece, PieceType::Turn);
        b.next_field(); // Length
        b.wheel(1.0);
        assert_eq!(b.settings.length, 9);
        b.next_field(); // Turn
        b.wheel(2.0);
        assert_eq!(b.settings.turn, 30);
        b.wheel(-100.0);
        assert_eq!(b.settings.turn, -MAX_TURN);
        b.next_field(); // Slope
        b.wheel(1.0);
        assert_eq!(b.settings.slope, 5);
        b.next_field(); // Element
        b.wheel(1.0);
        assert_eq!(b.settings.element, Kind::Lift);
        b.wheel(1.0);
        b.wheel(1.0);
        assert_eq!(b.settings.element, Kind::Track, "it wraps round");
    }

    #[test]
    fn right_click_builds_and_z_y_undo_and_redo_piece_by_piece() {
        let mut b = started();
        let start_nodes = b.ride().unwrap().nodes.clone();
        let head0 = b.head;
        b.confirm(no_rides);
        let after_one = b.ride().unwrap().nodes.clone();
        assert!(after_one.len() > start_nodes.len());
        b.confirm(no_rides);
        let after_two = b.ride().unwrap().nodes.clone();
        b.undo();
        assert_eq!(b.ride().unwrap().nodes, after_one);
        b.undo();
        assert_eq!(b.ride().unwrap().nodes, start_nodes);
        assert_eq!(b.head, head0);
        b.undo();
        assert_eq!(
            b.panel()
                .unwrap()
                .lines
                .iter()
                .filter(|l| l.text == "Nothing to undo.")
                .count(),
            1
        );
        b.redo();
        b.redo();
        assert_eq!(b.ride().unwrap().nodes, after_two);
        b.redo();
        // A new build clears the redo list.
        b.undo();
        b.confirm(no_rides);
        b.redo();
        assert!(
            b.panel()
                .unwrap()
                .lines
                .iter()
                .any(|l| l.text == "Nothing to redo.")
        );
    }

    #[test]
    fn an_element_piece_marks_the_node_before_it_and_undo_puts_that_back() {
        let mut b = started();
        b.next_field();
        b.next_field();
        b.next_field();
        b.next_field(); // Element
        b.wheel(1.0); // Lift
        let before = b.ride().unwrap().nodes.clone();
        b.confirm(no_rides);
        let last_before = before.len() - 1;
        assert_eq!(b.ride().unwrap().nodes[last_before].kind, Kind::Lift);
        b.undo();
        assert_eq!(b.ride().unwrap().nodes, before);
    }

    #[test]
    fn a_bad_piece_is_refused_with_the_reason_on_the_panel() {
        let mut b = started();
        b.wheel(1.0); // Turn
        b.next_field();
        b.wheel(-7.0); // Length 1
        b.next_field();
        b.wheel(1.0); // Turn 15
        let nodes = b.ride().unwrap().nodes.len();
        b.confirm(no_rides);
        assert_eq!(b.ride().unwrap().nodes.len(), nodes, "nothing built");
        let panel = b.panel().unwrap();
        assert!(
            panel
                .lines
                .iter()
                .any(|l| l.tone == Tone::Red && l.text.contains("Too short"))
        );
    }

    #[test]
    fn the_panel_shows_the_settings_and_the_live_numbers() {
        let mut b = started();
        let text: Vec<String> = b
            .panel()
            .unwrap()
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect();
        assert!(text.iter().any(|t| t.contains("Type: Straight")));
        assert!(text.iter().any(|t| t.contains("Length: 8 blocks")));
        assert!(text.iter().any(|t| t.contains("Banking: comes with M5")));
        assert!(text.iter().any(|t| t.contains("Speed here")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("Height here")));
    }

    #[test]
    fn the_ghost_follows_the_settings_and_the_version_moves_on() {
        let mut b = started();
        let v = b.version;
        let g1 = b.ghost().unwrap();
        assert!(!g1.is_finish && !g1.new.is_empty());
        b.wheel(1.0);
        assert!(b.version > v);
        b.close_for_test();
        assert!(b.ghost().is_none());
    }

    impl BuildMode {
        fn close_for_test(&mut self) {
            self.toggle();
        }
    }

    #[test]
    fn the_ghost_poses_cover_exactly_the_new_stretch() {
        let b = started();
        let ghost = b.ghost().unwrap();
        let poses = ghost.poses();
        // An 8-block straight is two 4-block rails, end to end.
        let total: f64 = poses.iter().map(|p| p.scale.z).sum();
        assert!((total - 8.0).abs() < 0.05, "{total}");
        assert!(poses.len() <= 4);
        // They start at the old end of the track.
        let head = b.head.pos;
        let first = poses[0].pos;
        assert!(first.distance(head) < 4.5);
    }

    #[test]
    fn closing_keeps_the_unfinished_coaster() {
        let mut b = started();
        b.confirm(no_rides);
        let n = b.ride().unwrap().nodes.len();
        b.toggle();
        b.toggle();
        assert_eq!(b.ride().unwrap().nodes.len(), n);
        assert!(!b.in_menu());
    }

    /// Build a lift hill, a drop, turns round to a head that points at the station's approach.
    pub(crate) fn built_round_the_station() -> BuildMode {
        let mut b = started();
        let build = |b: &mut BuildMode, piece, length, turn, slope, element| {
            b.settings = PieceSettings {
                piece,
                length,
                turn,
                slope,
                element,
            };
            b.confirm(no_rides);
        };
        let t = PieceType::SlopeChange;
        build(&mut b, t, 24, 0, 25, Kind::Lift);
        build(&mut b, PieceType::Straight, 10, 0, 0, Kind::Lift);
        build(&mut b, t, 12, 0, 0, Kind::Lift);
        build(&mut b, t, 20, 0, -30, Kind::Track);
        build(&mut b, t, 14, 0, 0, Kind::Track);
        build(&mut b, PieceType::Turn, 32, 90, 0, Kind::Track);
        build(&mut b, PieceType::Straight, 20, 0, 0, Kind::Track);
        build(&mut b, PieceType::Turn, 32, 90, 0, Kind::Track);
        for _ in 0..4 {
            build(&mut b, PieceType::Straight, 32, 0, 0, Kind::Track);
        }
        build(&mut b, PieceType::Turn, 32, 90, 0, Kind::Track);
        b
    }

    #[test]
    fn h_shows_the_finish_ghost_any_key_cancels_and_right_click_completes_the_ride() {
        let mut b = built_round_the_station();
        b.finish();
        if !b.is_finishing() {
            let lines: Vec<String> = b
                .panel()
                .unwrap()
                .lines
                .iter()
                .map(|l| l.text.clone())
                .collect();
            panic!("no finish: {lines:?}");
        }
        assert!(b.ghost().unwrap().is_finish);
        b.cancel_finish();
        assert!(!b.is_finishing());
        b.finish();
        assert!(b.is_finishing());
        // Accepting a name that already exists is refused.
        assert_eq!(b.confirm(|_| true), Outcome::Nothing);
        assert!(b.is_finishing());
        let Outcome::Finished(ride) = b.confirm(no_rides) else {
            panic!("the ride did not finish");
        };
        assert_eq!(ride.id, "coaster-1");
        assert!(course_from(&ride).is_ok());
        assert!(b.in_menu() && b.ride().is_none());
    }

    #[test]
    fn h_with_nothing_sensible_says_why_in_red() {
        let mut b = started();
        b.finish();
        assert!(!b.is_finishing());
        let panel = b.panel().unwrap();
        assert!(panel.lines.iter().any(|l| l.tone == Tone::Red), "{panel:?}");
    }
}
