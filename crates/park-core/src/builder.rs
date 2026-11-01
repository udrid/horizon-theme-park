//! The track wand's brain (D62–D66): who has which nodes, what the wand's mode is, undo,
//! and turning a list of nodes into a real ride (`create`).
//!
//! Nothing here touches the game. The runtime feeds it wand clicks and asks it for answers, so
//! `cargo test` can check every rule.
//!
//! How building works, in order:
//!   1. right-click a block with the wand: a node appears 1 block above that block's centre;
//!   2. right-click the air: the wand's mode changes (track -> lift -> brake -> station);
//!      the mode is written on every node you place next, and applies from that node to the next;
//!   3. `/coaster create <name>`: the nodes close into a loop, are checked, and become a ride.

use crate::coaster::elements::{Course, Element, Placed};
use crate::coaster::physics::{CAR_SPACING, MAX_CARS, Physics, Train};
use crate::coaster::spline::Track;
use crate::coaster::table::Table;
use crate::report::{self, Blocker};
use crate::save::{Kind, NodeFile, RideFile};
use glam::DVec3;

/// A ride needs at least this many nodes (D65).
pub const MIN_NODES: usize = 4;
/// Two neighbouring nodes must be at least this far apart, in blocks (D65).
pub const MIN_GAP: f64 = 1.0;
/// The longest ride name (S1).
pub const MAX_NAME: usize = 32;
/// The preview and the node markers stay for this many ticks after the last wand use (S2: 30 s).
pub const PREVIEW_TICKS: u64 = 600;
/// A node is placed this far above the clicked block's centre (D62).
pub const NODE_LIFT: f64 = 1.0;
/// The front car stops this far before the end of the station (the demo: 10 of 12.5 blocks).
pub const STOP_MARGIN: f64 = 2.5;
/// Extra room behind the last car in the station.
pub const STATION_REAR: f64 = 1.0;
/// A track steeper than this (the sine of its angle, about 82 degrees) is "vertical": not allowed yet.
const MAX_STEEP: f64 = 0.99;

/// The mode after `kind` when the player right-clicks the air (D63).
pub fn next_mode(kind: Kind) -> Kind {
    match kind {
        Kind::Track => Kind::Lift,
        Kind::Lift => Kind::Brake,
        Kind::Brake => Kind::Station,
        Kind::Station => Kind::Track,
    }
}

/// The word shown on the action bar.
pub fn mode_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Track => "track",
        Kind::Lift => "lift",
        Kind::Brake => "brake",
        Kind::Station => "station",
    }
}

/// Is `name` allowed for a ride (S1)? Lowercase letters, digits and `-`, up to 32 characters.
/// Also refused: `s` followed only by digits, which would clash with the session tag `htp_s<n>` (D66).
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(format!("A ride name needs 1 to {MAX_NAME} characters."));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("A ride name may only use lowercase letters, digits and -.".to_string());
    }
    if let Some(rest) = name.strip_prefix('s')
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        return Err(
            "Please pick another name: \"s\" and a number is kept for the mod itself.".to_string(),
        );
    }
    if name == "demo" {
        return Err("\"demo\" is the name of /coaster demo; please pick another name.".to_string());
    }
    Ok(())
}

/// Where a node goes when `block` is clicked (D62): 1 block above the block's centre.
pub fn node_for_block(block: [i32; 3]) -> [f64; 3] {
    [
        block[0] as f64 + 0.5,
        block[1] as f64 + 0.5 + NODE_LIFT,
        block[2] as f64 + 0.5,
    ]
}

fn vec(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

/// One player's wand: the nodes so far, the mode, and what is on show.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub uuid: u128,
    pub nodes: Vec<NodeFile>,
    pub mode: Kind,
    /// Set by `/coaster edit`: this session may save over that ride.
    pub editing: Option<String>,
    /// The ride being edited, as saved: `create` keeps its settings and trains (cars, speeds...).
    pub base: Option<RideFile>,
    /// The tick of the last wand use.
    pub last_use: u64,
    /// How many node markers are on show in the world right now (kept in step by the runtime).
    pub markers: usize,
    /// Count of changes to `nodes`; the preview is redrawn when it moves on.
    pub version: u64,
    /// The preview points for `preview_version` (worked out by the runtime).
    pub preview: Vec<DVec3>,
    pub preview_version: u64,
    /// The markers belong to other nodes (after `edit` or `create`): remove them all first.
    pub reload: bool,
}

/// All wand sessions, one per player.
#[derive(Clone, Debug, Default)]
pub struct Builder {
    sessions: Vec<Session>,
}

impl Builder {
    /// No sessions yet. `const`, so the runtime (kept in a `static` by the glue) can start with it.
    pub const fn new() -> Builder {
        Builder {
            sessions: Vec::new(),
        }
    }

    /// Forget everything (a new world opened).
    pub fn clear_all(&mut self) {
        self.sessions.clear();
    }

    /// The session of `uuid`, if they ever used the wand.
    pub fn session(&self, uuid: u128) -> Option<&Session> {
        self.sessions.iter().find(|s| s.uuid == uuid)
    }

    /// Every session (for the runtime's preview).
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// Mutable access to every session (the runtime switches `showing` off when time is up).
    pub fn sessions_mut(&mut self) -> &mut [Session] {
        &mut self.sessions
    }

    fn entry(&mut self, uuid: u128, now: u64) -> &mut Session {
        let at = match self.sessions.iter().position(|s| s.uuid == uuid) {
            Some(at) => at,
            None => {
                self.sessions.push(Session {
                    uuid,
                    nodes: Vec::new(),
                    mode: Kind::Track,
                    editing: None,
                    base: None,
                    last_use: now,
                    markers: 0,
                    version: 0,
                    preview: Vec::new(),
                    preview_version: u64::MAX,
                    reload: false,
                });
                self.sessions.len() - 1
            }
        };
        let session = &mut self.sessions[at];
        session.last_use = now;
        session
    }

    /// Right-click on a block: add a node (D62). Returns its number (1 is the first).
    pub fn click_block(&mut self, uuid: u128, block: [i32; 3], now: u64) -> usize {
        let session = self.entry(uuid, now);
        let kind = session.mode;
        session.nodes.push(NodeFile {
            pos: node_for_block(block),
            roll: 0.0,
            kind,
        });
        session.version += 1;
        session.nodes.len()
    }

    /// Right-click on the air: next mode (D63).
    pub fn click_air(&mut self, uuid: u128, now: u64) -> Kind {
        let session = self.entry(uuid, now);
        session.mode = next_mode(session.mode);
        session.mode
    }

    /// Take the last node away. Returns the number it had, or `None` if there was none.
    ///
    /// (Horizon's stable helpers do not say whether a player is sneaking, so D62's "sneak +
    /// right-click" cannot be heard; the runtime offers this as `/coaster undo` instead.)
    pub fn undo(&mut self, uuid: u128, now: u64) -> Option<usize> {
        let session = self.entry(uuid, now);
        session.nodes.pop()?;
        session.version += 1;
        Some(session.nodes.len() + 1)
    }

    /// Put a saved ride's nodes into the wand to change them (`/coaster edit`).
    pub fn load_for_edit(&mut self, uuid: u128, ride: &RideFile, now: u64) {
        let session = self.entry(uuid, now);
        session.nodes = ride.nodes.clone();
        session.mode = Kind::Track;
        session.editing = Some(ride.id.clone());
        session.base = Some(ride.clone());
        session.version += 1;
        session.reload = true;
    }

    /// Start over with no nodes (after a successful `create`).
    pub fn reset(&mut self, uuid: u128) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.uuid == uuid) {
            session.nodes.clear();
            session.mode = Kind::Track;
            session.editing = None;
            session.base = None;
            session.version += 1;
            session.reload = true;
        }
    }

    /// Check the player's nodes and make the ride `name` (D65, C4). `exists` says whether a ride
    /// of that name is already there. A name that exists is refused unless this session loaded it
    /// with `edit` (no silent overwrite). Returns the ride to save, with its course already checked.
    pub fn create(
        &self,
        uuid: u128,
        name: &str,
        exists: impl Fn(&str) -> bool,
    ) -> Result<RideFile, String> {
        check_name(name)?;
        let Some(session) = self.session(uuid) else {
            return Err("You have no nodes yet. Get the wand with /coaster wand.".to_string());
        };
        if exists(name) && session.editing.as_deref() != Some(name) {
            return Err(format!(
                "A ride called {name} already exists. Use /coaster edit {name} to change it."
            ));
        }
        // Editing a ride keeps everything but its nodes.
        let ride = match &session.base {
            Some(base) if session.editing.as_deref() == Some(name) => RideFile {
                nodes: session.nodes.clone(),
                ..base.clone()
            },
            _ => RideFile::new(name, session.nodes.clone()),
        };
        let course = course_from(&ride)?;
        // D68: a track whose train can't stop in the station, or stalls, does not open.
        let found = report::blocker(
            &course,
            cars_of(&ride),
            spacing_of(&ride),
            &physics_of(&ride),
        );
        match found {
            Some(Blocker::Overshoot { s }) => {
                let node = nearest_node(&ride, &course, s);
                return Err(format!(
                    "The train comes into the station too fast and would roll past its stop point (the approach is near node {node}). Add brake nodes before the station: right-click the air until the wand says \"brake\" and place nodes there, or lower the hill before it."
                ));
            }
            Some(Blocker::Stall { s }) => {
                let node = nearest_node(&ride, &course, s);
                return Err(format!(
                    "The train runs out of speed and stalls near node {node}. Lower the hill near node {node}, or put lift nodes before it."
                ));
            }
            None => {}
        }
        Ok(ride)
    }
}

/// The points the preview draws: the curve through the nodes every `step` blocks (S2: 0.5).
/// With fewer than 3 nodes there is no loop yet, so only the nodes (and a line for two) show.
/// At most `max_points` points: a long track gets a wider step instead.
pub fn preview_points(nodes: &[NodeFile], step: f64, max_points: usize) -> Vec<DVec3> {
    let points: Vec<DVec3> = nodes.iter().map(|n| vec(n.pos)).collect();
    if points.len() < 3 {
        let mut out = points.clone();
        if let [a, b] = points[..] {
            let n = (a.distance(b) / step).floor() as usize;
            out.extend((1..n).map(|i| a.lerp(b, i as f64 / n as f64)));
        }
        return out;
    }
    let table = Table::new_level(&Track::closed(&points));
    if table.is_empty() {
        return points;
    }
    let step = step.max(table.length() / max_points.max(1) as f64);
    let mut out = Vec::new();
    let mut s = 0.0;
    while s < table.length() {
        out.push(table.position(s));
        s += step;
    }
    out
}

/// The distance along the lap of every node, plus the lap's length at the end (so `n + 1` numbers).
/// Each curve between two nodes is measured with 64 little steps, then scaled to the table's length.
fn node_distances(track: &Track, table: &Table) -> Vec<f64> {
    let mut sums = vec![0.0];
    for segment in track.segments() {
        let mut length = 0.0;
        let mut last = segment.point(0.0);
        for i in 1..=64 {
            let p = segment.point(i as f64 / 64.0);
            length += p.distance(last);
            last = p;
        }
        sums.push(sums[sums.len() - 1] + length);
    }
    let total = sums[sums.len() - 1];
    let scale = if total > 0.0 {
        table.length() / total
    } else {
        0.0
    };
    sums.iter().map(|s| s * scale).collect()
}

/// The number (1 is the first) of the node nearest to the point `s` along the lap.
pub fn nearest_node(ride: &RideFile, course: &Course, s: f64) -> usize {
    let points: Vec<DVec3> = ride.nodes.iter().map(|node| vec(node.pos)).collect();
    let track = Track::closed(&points);
    let node_s = node_distances(&track, &course.table);
    let length = course.table.length();
    let s = course.table.wrap(s);
    (0..ride.nodes.len())
        .min_by(|&a, &b| {
            let gap = |i: usize| {
                let d = (node_s[i] - s).abs();
                d.min(length - d)
            };
            gap(a).total_cmp(&gap(b))
        })
        .map_or(1, |i| i + 1)
}

/// The physics numbers saved with a ride.
pub fn physics_of(ride: &RideFile) -> Physics {
    Physics {
        gravity_scale: ride.settings.gravity_scale,
        friction: ride.settings.friction,
        drag: ride.settings.drag,
    }
}

/// How many cars the ride's first train has.
pub fn cars_of(ride: &RideFile) -> usize {
    ride.trains
        .first()
        .map_or(3, |t| t.cars as usize)
        .clamp(1, MAX_CARS)
}

/// The gap between car middles of the ride's first train.
pub fn spacing_of(ride: &RideFile) -> f64 {
    ride.trains.first().map_or(CAR_SPACING, |t| t.spacing)
}

/// A train standing in the station of `course`, ready for riders.
pub fn train_of(ride: &RideFile, course: &Course) -> Train {
    let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
    Train {
        spacing: spacing_of(ride),
        ..Train::held(cars_of(ride), stop)
    }
}

/// Turn a ride's nodes into a course a train can run on, or say plainly why not (D65).
pub fn course_from(ride: &RideFile) -> Result<Course, String> {
    let n = ride.nodes.len();
    if n < MIN_NODES {
        return Err(format!(
            "A coaster needs at least {MIN_NODES} nodes; you have {n}."
        ));
    }
    let points: Vec<DVec3> = ride.nodes.iter().map(|node| vec(node.pos)).collect();
    for i in 0..n {
        let j = (i + 1) % n;
        let gap = points[i].distance(points[j]);
        if gap < MIN_GAP {
            return Err(format!(
                "Nodes {} and {} are only {:.1} blocks apart; they must be at least {MIN_GAP} block apart.",
                i + 1,
                j + 1,
                gap
            ));
        }
    }
    let kinds: Vec<Kind> = ride.nodes.iter().map(|node| node.kind).collect();
    if kinds[0] == Kind::Station && kinds[n - 1] == Kind::Station {
        return Err(
            "The station may not hold both the first and the last node. Put your first node before the station."
                .to_string(),
        );
    }

    let track = Track::closed(&points);
    let table = Table::new_level(&track);
    if table.is_empty() {
        return Err("The track could not be measured. Check the nodes.".to_string());
    }
    let node_s = node_distances(&track, &table);

    // Runs of the same non-track kind become one element each (D63): it covers the track from
    // the run's first node to the node after its last.
    let mut placed = Vec::new();
    let mut stations = 0;
    let mut i = 0;
    while i < n {
        let kind = kinds[i];
        let mut j = i;
        while j < n && kinds[j] == kind {
            j += 1;
        }
        let (s0, s1) = (node_s[i], node_s[j]);
        match kind {
            Kind::Track => {}
            Kind::Lift => placed.push(Placed {
                s0,
                s1,
                element: Element::Lift {
                    speed: ride.settings.lift_speed,
                },
            }),
            Kind::Brake => placed.push(Placed {
                s0,
                s1,
                element: Element::Brake {
                    target_v: ride.settings.brake_target,
                    decel: ride.settings.brake_decel,
                },
            }),
            Kind::Station => {
                stations += 1;
                let cars = cars_of(ride);
                let need = (cars - 1) as f64 * spacing_of(ride) + STOP_MARGIN + STATION_REAR;
                if s1 - s0 < need {
                    return Err(format!(
                        "The station is {:.1} blocks long, but {cars} cars need at least {need:.1}. Make the station longer or use fewer cars.",
                        s1 - s0
                    ));
                }
                placed.push(Placed {
                    s0,
                    s1,
                    element: Element::Station {
                        stop: s1 - STOP_MARGIN,
                        push: ride.settings.station_push,
                    },
                });
            }
        }
        i = j;
    }
    if stations == 0 {
        return Err("There is no station. Right-click the air until the wand says \"station\", place the station's nodes, then switch back to track.".to_string());
    }
    if stations > 1 {
        return Err(
            "There is more than one station. A ride has exactly one station stretch.".to_string(),
        );
    }

    // Vertical track would break the level frames (D59): refuse it with the place.
    let mut s = 0.0;
    while s < table.length() {
        if table.frame(s).forward.y.abs() > MAX_STEEP {
            let node = node_s.partition_point(|&x| x <= s).clamp(1, n);
            return Err(format!(
                "The track goes almost straight up or down near node {node}. Vertical track is not supported yet; make that stretch less steep."
            ));
        }
        s += 0.25;
    }

    let course = Course { table, placed };
    course.validate()?;
    Ok(course)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn node(pos: [f64; 3], kind: Kind) -> NodeFile {
        NodeFile {
            pos,
            roll: 0.0,
            kind,
        }
    }

    /// A valid little loop: a 20-block station, a lift and a gentle turnaround.
    pub(crate) fn loop_nodes() -> Vec<NodeFile> {
        vec![
            node([0.0, 70.0, 0.0], Kind::Station),
            node([10.0, 70.0, 0.0], Kind::Station),
            node([20.0, 70.0, 0.0], Kind::Lift),
            node([30.0, 72.0, 5.0], Kind::Lift),
            node([35.0, 74.0, 15.0], Kind::Lift),
            node([25.0, 74.0, 25.0], Kind::Lift),
            node([10.0, 72.0, 25.0], Kind::Brake),
            node([0.0, 70.0, 15.0], Kind::Track),
        ]
    }

    fn ride_with(nodes: Vec<NodeFile>) -> RideFile {
        RideFile::new("test", nodes)
    }

    // Test 5.
    #[test]
    fn clicks_add_nodes_one_block_above_the_block_centre_and_modes_cycle() {
        let mut b = Builder::new();
        assert_eq!(b.click_block(1, [10, 64, -3], 0), 1);
        assert_eq!(b.click_block(1, [11, 64, -3], 1), 2);
        let nodes = &b.session(1).unwrap().nodes;
        assert_eq!(nodes[0].pos, [10.5, 65.5, -2.5]);
        assert_eq!(nodes[1].pos, [11.5, 65.5, -2.5]);
        assert!(nodes.iter().all(|n| n.kind == Kind::Track));

        // The mode is written on the next node placed.
        assert_eq!(b.click_air(1, 2), Kind::Lift);
        b.click_block(1, [12, 64, -3], 3);
        assert_eq!(b.click_air(1, 4), Kind::Brake);
        assert_eq!(b.click_air(1, 5), Kind::Station);
        assert_eq!(b.click_air(1, 6), Kind::Track);
        assert_eq!(b.session(1).unwrap().nodes[2].kind, Kind::Lift);

        // Undo takes the last one away; another player has their own wand.
        assert_eq!(b.undo(1, 7), Some(3));
        assert_eq!(b.session(1).unwrap().nodes.len(), 2);
        b.click_block(2, [0, 0, 0], 8);
        assert_eq!(b.session(2).unwrap().nodes.len(), 1);
        assert_eq!(b.undo(3, 9), None);
    }

    // Test 6.
    #[test]
    fn create_refuses_too_few_nodes_no_station_and_nodes_too_close() {
        let none = |_: &str| false;
        let mut b = Builder::new();
        assert!(b.create(1, "x", none).unwrap_err().contains("wand"));
        for i in 0..3 {
            b.click_block(1, [i * 5, 64, 0], 0);
        }
        assert_eq!(
            b.create(1, "x", none).unwrap_err(),
            "A coaster needs at least 4 nodes; you have 3."
        );
        // Four nodes but no station.
        b.click_block(1, [10, 64, 10], 0);
        let error = b.create(1, "x", none).unwrap_err();
        assert!(error.contains("no station"), "{error}");
        // Two nodes closer than a block.
        b.click_block(1, [10, 64, 10], 0); // the same spot twice
        let error = b.create(1, "x", none).unwrap_err();
        assert!(
            error.starts_with("Nodes 4 and 5 are only 0.0 blocks apart"),
            "{error}"
        );
        // The wrap-around pair counts too.
        let mut wrap = ride_with(loop_nodes());
        wrap.nodes[7].pos = [0.5, 70.0, 0.3];
        let error = course_from(&wrap).unwrap_err();
        assert!(error.starts_with("Nodes 8 and 1 are only"), "{error}");
    }

    #[test]
    fn a_good_loop_makes_a_course_with_one_station_a_lift_and_room_for_the_train() {
        let ride = ride_with(loop_nodes());
        let course = course_from(&ride).unwrap();
        let (station, stop, _) = course.station().unwrap();
        assert!((station.s1 - station.s0 - 20.0).abs() < 1.5, "{station:?}");
        assert!((stop - (station.s1 - STOP_MARGIN)).abs() < 1e-9);
        assert_eq!(
            course
                .placed
                .iter()
                .filter(|p| matches!(p.element, Element::Lift { .. }))
                .count(),
            1
        );
        let train = train_of(&ride, &course);
        assert!(train.is_held() && train.cars == 3 && train.s == stop);
    }

    #[test]
    fn a_station_that_is_too_short_for_the_cars_says_how_long_it_must_be() {
        let mut ride = ride_with(loop_nodes());
        ride.trains[0].cars = 8;
        let error = course_from(&ride).unwrap_err();
        assert!(error.contains("8 cars need at least 25.9"), "{error}");
    }

    #[test]
    fn a_station_across_the_first_node_or_two_stations_are_refused() {
        let mut ride = ride_with(loop_nodes());
        ride.nodes[7].kind = Kind::Station;
        assert!(
            course_from(&ride)
                .unwrap_err()
                .contains("first and the last")
        );
        let mut two = ride_with(loop_nodes());
        two.nodes[5].kind = Kind::Station;
        two.nodes[6].kind = Kind::Station;
        assert!(
            course_from(&two)
                .unwrap_err()
                .contains("more than one station")
        );
    }

    #[test]
    fn straight_up_track_is_refused_and_names_the_node() {
        let mut ride = ride_with(loop_nodes());
        ride.nodes[3].pos = [30.0, 72.0, 5.0];
        ride.nodes[4].pos = [30.5, 90.0, 5.0];
        ride.nodes[5].pos = [30.5, 72.0, 5.5];
        let error = course_from(&ride).unwrap_err();
        assert!(error.contains("almost straight up or down"), "{error}");
    }

    // Test 7.
    #[test]
    fn a_name_that_exists_asks_for_edit_unless_you_are_editing_it() {
        let mut b = Builder::new();
        for node in loop_nodes() {
            let s = b.sessions.iter().position(|s| s.uuid == 1);
            if s.is_none() {
                b.click_block(1, [0, 0, 0], 0);
                b.sessions[0].nodes.clear();
            }
            b.sessions[0].nodes.push(node);
        }
        let taken = |name: &str| name == "thunder";
        let error = b.create(1, "thunder", taken).unwrap_err();
        assert_eq!(
            error,
            "A ride called thunder already exists. Use /coaster edit thunder to change it."
        );
        assert!(b.create(1, "fresh", taken).is_ok());
        // With `edit thunder` the same name is fine.
        let old = ride_with(loop_nodes());
        let old = RideFile {
            id: "thunder".into(),
            ..old
        };
        b.load_for_edit(1, &old, 5);
        assert!(b.create(1, "thunder", taken).is_ok());
        // ...but not another name that exists.
        assert!(b.create(1, "other", |_| true).is_err());
    }

    #[test]
    fn editing_and_creating_again_keeps_the_cars_and_settings() {
        let mut old = RideFile {
            id: "thunder".into(),
            ..ride_with(loop_nodes())
        };
        old.trains[0].cars = 5;
        old.settings.friction = 0.03;
        let mut b = Builder::new();
        b.load_for_edit(1, &old, 0);
        // Change one node, then create again.
        b.sessions_mut()[0].nodes[5].pos = [26.0, 74.0, 26.0];
        let new = b.create(1, "thunder", |n| n == "thunder").unwrap();
        assert_eq!(new.trains[0].cars, 5);
        assert_eq!(new.settings.friction, 0.03);
        assert_eq!(new.nodes[5].pos, [26.0, 74.0, 26.0]);
        // A different name starts from defaults.
        let other = b.create(1, "other", |_| false).unwrap();
        assert_eq!(other.trains[0].cars, 3);
    }

    // D68.
    #[test]
    fn the_little_loop_may_open_but_a_hill_with_no_lift_is_refused_with_the_node() {
        let mut b = Builder::new();
        let load = |b: &mut Builder, nodes: Vec<NodeFile>| {
            b.click_block(1, [0, 0, 0], 0);
            b.sessions_mut()[0].nodes = nodes;
        };
        load(&mut b, loop_nodes());
        assert!(b.create(1, "ok", |_| false).is_ok());
        // Take the lift away: the 4-block hill is too high for the station's push.
        let mut flat = loop_nodes();
        for node in &mut flat[3..=5] {
            node.kind = Kind::Track;
        }
        load(&mut b, flat);
        let error = b.create(1, "nope", |_| false).unwrap_err();
        assert!(error.contains("stalls near node"), "{error}");
        assert!(error.contains("Lower the hill near node"), "{error}");
    }

    #[test]
    fn a_fast_station_approach_is_refused_and_names_the_brake_fix() {
        // A tall drop straight into the station, with a lift to get up and no brakes.
        let node = |pos: [f64; 3], kind| NodeFile {
            pos,
            roll: 0.0,
            kind,
        };
        let nodes = vec![
            node([0.0, 70.0, 0.0], Kind::Station),
            node([10.0, 70.0, 0.0], Kind::Station),
            node([22.0, 70.0, 0.0], Kind::Lift),
            node([36.0, 80.0, 6.0], Kind::Lift),
            node([48.0, 92.0, 16.0], Kind::Lift),
            node([44.0, 96.0, 30.0], Kind::Lift),
            node([30.0, 88.0, 38.0], Kind::Track),
            node([18.0, 76.0, 34.0], Kind::Track),
            node([6.0, 71.0, 24.0], Kind::Track),
            node([-8.0, 70.0, 12.0], Kind::Track),
        ];
        let mut b = Builder::new();
        b.click_block(1, [0, 0, 0], 0);
        b.sessions_mut()[0].nodes = nodes;
        let error = b.create(1, "fast", |_| false).unwrap_err();
        assert!(
            error.contains("too fast") && error.contains("brake nodes"),
            "{error}"
        );
        assert!(error.contains("near node"), "{error}");
    }

    #[test]
    fn names_follow_the_rules() {
        assert!(check_name("thunder-run-2").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(&"a".repeat(33)).is_err());
        assert!(check_name("Thunder").is_err());
        assert!(check_name("a b").is_err());
        assert!(check_name("a_b").is_err());
        assert!(check_name("s12").is_err());
        assert!(check_name("sky").is_ok());
        assert!(check_name("demo").is_err());
    }

    #[test]
    fn the_preview_follows_the_curve_and_is_capped() {
        let nodes = loop_nodes();
        let points = preview_points(&nodes, 0.5, 10_000);
        assert!(points.len() > 100);
        let capped = preview_points(&nodes, 0.5, 40);
        assert!(capped.len() <= 41, "{}", capped.len());
        // One node shows itself; two nodes show a line.
        assert_eq!(preview_points(&nodes[..1], 0.5, 40).len(), 1);
        assert!(preview_points(&nodes[..2], 0.5, 40).len() > 5);
    }
}
