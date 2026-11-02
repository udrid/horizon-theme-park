//! The builder's live numbers (M3b T2, D75): what the train would do on the track you have,
//! and how the piece you are about to build would feel.
//!
//! The track you are building is still open (it is not a loop yet), but the physics wants a
//! loop. So we cheat in a way that does no harm: we add two invisible nodes straight ahead of
//! the end, close the loop through them and back to the station, and run a dry ride that stops
//! as soon as the front car reaches the end. Everything past the end is never driven on.
//!
//! The numbers come from the same physics and the same limits (D58) as the realism report
//! (D64), so the screen and the report never disagree.

use crate::builder::{cars_of, physics_of, spacing_of};
use crate::builder::{course_checked, node_distances};
use crate::coaster::physics::Train;
use crate::coaster::sim::{self, Sample};
use crate::coaster::spline::Track;
use crate::coaster::table::Table;
use crate::pieces::{Head, Piece, PieceSettings, build_piece};
use crate::report::{AIRTIME_MIN_G, LATERAL_MAX_G, RATE_MAX_G_PER_S, VALLEY_MAX_G};
use crate::save::{Kind, NodeFile, RideFile};
use glam::DVec3;

/// How far past the end the two invisible nodes sit, in blocks.
const PHANTOM_GAP: f64 = 3.0;
/// The longest dry ride we allow, in seconds.
const MAX_SECONDS: f64 = 240.0;
/// A train slower than this for this many ticks in a row (2 s) has stalled.
const STALL_SPEED: f64 = 0.1;
const STALL_TICKS: usize = 40;

/// What the new piece would feel like, measured on the front car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostStats {
    /// Speed when the train enters the piece and when it leaves it, blocks per second.
    pub entry_speed: f64,
    pub exit_speed: f64,
    pub max_vertical: f64,
    pub min_vertical: f64,
    pub max_lateral: f64,
    pub max_vertical_rate: f64,
}

/// The answer for the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    /// Speed of the train as it reaches the end of the track you have, blocks per second. `None`
    /// if the train never gets there (see `issues`).
    pub end_speed: Option<f64>,
    /// Height of the end of the track (the new piece's start).
    pub end_height: f64,
    /// Present when a piece was asked about and the train reaches it.
    pub ghost: Option<GhostStats>,
    /// Everything that is past the realism limits or would stop the train, in plain words.
    pub issues: Vec<String>,
}

impl Preview {
    /// True when nothing is past a limit: the piece may be shown in white, not red.
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

fn vec(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

/// Run the train from the station to the end of `ride`'s nodes plus `piece` (if any), and say
/// what it did. `Err` is a plain-words reason the track can't be tried at all (for example, no
/// station yet).
pub fn preview(ride: &RideFile, piece: Option<&Piece>) -> Result<Preview, String> {
    let mut nodes: Vec<NodeFile> = ride.nodes.clone();
    if nodes.len() < 2 {
        return Err("There is no track yet.".to_string());
    }
    let head_index = nodes.len() - 1;
    if let Some(piece) = piece {
        nodes[head_index].kind = piece.start_kind;
        nodes.extend(piece.nodes.iter().cloned());
    }
    let real = nodes.len();
    // The two invisible nodes: straight on in the direction the track leaves its last two nodes.
    let last = vec(nodes[real - 1].pos);
    if real < 2 {
        return Err("There is no track yet.".to_string());
    }
    let before = vec(nodes[real - 2].pos);
    let ahead = (last - before).normalize_or_zero();
    if ahead == DVec3::ZERO {
        return Err("The last two nodes are on the same spot.".to_string());
    }
    let phantom = |p: DVec3| NodeFile {
        pos: [p.x, p.y, p.z],
        roll: 0.0,
        kind: Kind::Track,
    };
    for k in 1..=2 {
        nodes.push(phantom(last + ahead * (PHANTOM_GAP * k as f64)));
    }
    // And two behind the first node, so the loop's wrap-around does not bend the station either.
    let first = vec(nodes[0].pos);
    let start_dir = (vec(nodes[1].pos) - first).normalize_or_zero();
    for k in 1..=2 {
        nodes.insert(0, phantom(first - start_dir * (PHANTOM_GAP * k as f64)));
    }
    // The real nodes now sit two places further along the list.
    let (head_index, real) = (head_index + 2, real + 2);
    let trial = RideFile {
        nodes,
        ..ride.clone()
    };
    // Only the real track is checked for being too steep, not the invisible return.
    let course = course_checked(&trial, 2, real - 1)?;

    // Where along the lap the old end (the head) and the new end are.
    let points: Vec<DVec3> = trial.nodes.iter().map(|n| vec(n.pos)).collect();
    let track = Track::closed(&points);
    let table = Table::new_level(&track);
    let node_s = node_distances(&track, &table);
    let s_head = node_s[head_index];
    let s_end = node_s[real - 1];

    let cars = cars_of(&trial);
    let spacing = spacing_of(&trial);
    let physics = physics_of(&trial);
    let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
    let mut train = Train {
        spacing,
        ..Train::held(cars, stop)
    };
    train.start_countdown(0.0);

    let mut samples: Vec<Sample> = Vec::new();
    let mut slow = 0;
    let mut stalled = false;
    let mut t = 0.0;
    while t < MAX_SECONDS {
        samples.push(Sample {
            t,
            s: train.s,
            v: train.v,
            felt: sim::felt_all(&course, &train, &physics),
            cars: train.cars,
        });
        if train.s >= s_end {
            break;
        }
        train.step(&course, &physics);
        t += 0.05;
        if samples.len() > 20 && train.v < STALL_SPEED && !train.is_held() {
            slow += 1;
            if slow >= STALL_TICKS {
                stalled = true;
                break;
            }
        } else {
            slow = 0;
        }
    }
    let reached_end = !stalled && train.s >= s_end;

    let mut issues = Vec::new();
    let reached_head = samples.iter().find(|s| s.s >= s_head);
    let end_speed = reached_head.map(|s| s.v);
    let end_height = table.position(s_head).y;
    if reached_head.is_none() {
        issues.push(
            "The train runs out of speed before the end of the track. Add a lift or lower the hills."
                .to_string(),
        );
    } else if !reached_end && piece.is_some() {
        issues.push(
            "The train would run out of speed in this piece. Make it less steep, or add a lift."
                .to_string(),
        );
    } else if stalled {
        issues.push(
            "The train runs out of speed before the end of the track. Add a lift or lower the hills."
                .to_string(),
        );
    }

    let ghost = if piece.is_some() && reached_end {
        Some(ghost_stats(&samples, s_head, &mut issues))
    } else {
        None
    };
    Ok(Preview {
        end_speed,
        end_height,
        ghost,
        issues,
    })
}

/// One setting of a piece that the panel can scroll through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Length,
    Turn,
    Slope,
}

/// For each of `values` of `setting` (the others as in `settings`): `None` if that value is fine,
/// or the plain-words reason it should be greyed out on the panel (D75). A value is greyed when
/// the piece can't be built or the train would pass a realism limit (D58) or stall on it.
pub fn blocked_values(
    ride: &RideFile,
    head: &Head,
    settings: &PieceSettings,
    setting: Setting,
    values: &[i32],
) -> Vec<Option<String>> {
    values
        .iter()
        .map(|&value| {
            let mut s = *settings;
            match setting {
                Setting::Length => s.length = value.max(0) as u32,
                Setting::Turn => s.turn = value,
                Setting::Slope => s.slope = value,
            }
            match build_piece(head, &s) {
                Err(why) => Some(why),
                Ok(piece) => match preview(ride, Some(&piece)) {
                    Err(why) => Some(why),
                    Ok(p) => p.issues.into_iter().next(),
                },
            }
        })
        .collect()
}

/// Measure the front car over the new piece and write down every limit it passes (D58).
fn ghost_stats(samples: &[Sample], s_head: f64, issues: &mut Vec<String>) -> GhostStats {
    // Only the front car, only while it is on the new piece.
    let on_piece: Vec<Sample> = samples
        .iter()
        .filter(|s| s.s >= s_head)
        .map(|s| Sample { cars: 1, ..*s })
        .collect();
    let st = sim::stats(&on_piece);
    let first = on_piece.first().copied();
    let last = on_piece.last().copied();
    let stats = GhostStats {
        entry_speed: first.map_or(0.0, |s| s.v),
        exit_speed: last.map_or(0.0, |s| s.v),
        max_vertical: if on_piece.is_empty() {
            0.0
        } else {
            st.max_vertical
        },
        min_vertical: if on_piece.is_empty() {
            0.0
        } else {
            st.min_vertical
        },
        max_lateral: st.max_lateral,
        max_vertical_rate: st.max_vertical_rate,
    };
    if stats.max_vertical > VALLEY_MAX_G {
        issues.push(format!(
            "Vertical {:.1} g (limit {VALLEY_MAX_G:.1} g): round the bottom more, or make it longer.",
            stats.max_vertical
        ));
    }
    if stats.min_vertical < AIRTIME_MIN_G {
        issues.push(format!(
            "Airtime {:.1} g (limit {AIRTIME_MIN_G:.1} g): make the crest rounder, or longer.",
            stats.min_vertical
        ));
    }
    if stats.max_lateral > LATERAL_MAX_G {
        issues.push(format!(
            "Lateral {:.1} g (limit {LATERAL_MAX_G:.1} g): widen the turn.",
            stats.max_lateral
        ));
    }
    if stats.max_vertical_rate > RATE_MAX_G_PER_S {
        issues.push(format!(
            "Vertical g changes {:.1} g per second (limit {RATE_MAX_G_PER_S:.1}): use a gentler change.",
            stats.max_vertical_rate
        ));
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::{Head, PieceSettings, PieceType, build_piece};

    /// A station 12 blocks long heading south from (0, 70, 0), and the pen at its end.
    fn start() -> (RideFile, Head) {
        let node = |z: f64, kind: Kind| NodeFile {
            pos: [0.0, 70.0, z],
            roll: 0.0,
            kind,
        };
        let nodes = vec![
            node(0.0, Kind::Station),
            node(4.0, Kind::Station),
            node(8.0, Kind::Station),
            node(12.0, Kind::Track),
        ];
        let head = Head {
            pos: DVec3::new(0.0, 70.0, 12.0),
            heading: 0.0,
            slope: 0.0,
        };
        (RideFile::new("t", nodes), head)
    }

    /// Add a piece to the ride, the way the builder will.
    fn add(ride: &mut RideFile, head: &mut Head, s: PieceSettings) -> Piece {
        let piece = build_piece(head, &s).unwrap();
        let last = ride.nodes.len() - 1;
        ride.nodes[last].kind = piece.start_kind;
        ride.nodes.extend(piece.nodes.iter().cloned());
        *head = piece.end;
        piece
    }

    /// A lift hill the train can get over: up, along, and the lift carries it across the crest.
    fn lift_hill_up(ride: &mut RideFile, head: &mut Head) {
        add(ride, head, lift(24, 25));
        add(
            ride,
            head,
            PieceSettings {
                element: Kind::Lift,
                ..PieceSettings::straight(10)
            },
        );
        add(ride, head, lift(12, 0));
    }

    fn lift(length: u32, slope: i32) -> PieceSettings {
        PieceSettings {
            piece: PieceType::SlopeChange,
            length,
            turn: 0,
            slope,
            element: Kind::Lift,
        }
    }

    fn plain(piece: PieceType, length: u32, turn: i32, slope: i32) -> PieceSettings {
        PieceSettings {
            piece,
            length,
            turn,
            slope,
            element: Kind::Track,
        }
    }

    #[test]
    fn the_station_alone_gives_the_push_out_speed() {
        let (mut ride, mut head) = start();
        add(&mut ride, &mut head, PieceSettings::straight(6));
        let p = preview(&ride, None).unwrap();
        let v = p.end_speed.unwrap();
        assert!(v > 1.0 && v < 4.0, "speed {v}");
        assert!(p.ghost.is_none());
        assert!((p.end_height - 70.0).abs() < 0.01);
    }

    #[test]
    fn a_lift_hill_then_a_drop_gains_speed_and_reports_the_ghost() {
        let (mut ride, mut head) = start();
        lift_hill_up(&mut ride, &mut head);
        let top = preview(&ride, None).unwrap();
        assert!(top.end_height > 80.0, "height {}", top.end_height);
        // A gentle drop.
        let drop = build_piece(&head, &plain(PieceType::SlopeChange, 20, 0, -40)).unwrap();
        let p = preview(&ride, Some(&drop)).unwrap();
        let g = p.ghost.unwrap();
        assert!(g.exit_speed > g.entry_speed + 3.0, "{g:?}");
        assert!(p.ok(), "{:?}", p.issues);
    }

    #[test]
    fn a_tight_fast_turn_is_flagged_for_sideways_g() {
        let (mut ride, mut head) = start();
        lift_hill_up(&mut ride, &mut head);
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, -50),
        );
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, 0),
        );
        // A half circle in 8 blocks (radius about 2.5) at speed.
        let turn = build_piece(&head, &plain(PieceType::Turn, 8, 180, 0)).unwrap();
        let p = preview(&ride, Some(&turn)).unwrap();
        assert!(!p.ok());
        assert!(
            p.issues.iter().any(|i| i.contains("widen the turn")),
            "{:?}",
            p.issues
        );
        assert!(p.ghost.unwrap().max_lateral > LATERAL_MAX_G);
    }

    #[test]
    fn a_wide_turn_at_the_same_speed_is_fine() {
        let (mut ride, mut head) = start();
        lift_hill_up(&mut ride, &mut head);
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, -50),
        );
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, 0),
        );
        let turn = build_piece(&head, &plain(PieceType::Turn, 32, 45, 0)).unwrap();
        let p = preview(&ride, Some(&turn)).unwrap();
        assert!(p.ok(), "{:?}", p.issues);
    }

    #[test]
    fn a_climb_with_no_lift_stalls_and_says_so() {
        let (mut ride, mut head) = start();
        add(&mut ride, &mut head, PieceSettings::straight(6));
        let climb = build_piece(&head, &plain(PieceType::SlopeChange, 30, 0, 60)).unwrap();
        let p = preview(&ride, Some(&climb)).unwrap();
        assert!(!p.ok());
        assert!(p.issues[0].contains("run out of speed"), "{:?}", p.issues);
        assert!(p.ghost.is_none());
    }

    #[test]
    fn a_track_with_no_station_gives_a_plain_refusal() {
        let node = |z: f64| NodeFile {
            pos: [0.0, 70.0, z],
            roll: 0.0,
            kind: Kind::Track,
        };
        let ride = RideFile::new("t", vec![node(0.0), node(4.0), node(8.0), node(12.0)]);
        assert!(preview(&ride, None).is_err());
    }

    #[test]
    fn asking_twice_gives_the_same_answer() {
        let (mut ride, mut head) = start();
        add(&mut ride, &mut head, lift(24, 25));
        let piece = build_piece(&head, &plain(PieceType::SlopeChange, 16, 0, 0)).unwrap();
        assert_eq!(
            preview(&ride, Some(&piece)).unwrap(),
            preview(&ride, Some(&piece)).unwrap()
        );
    }

    #[test]
    fn greyed_out_values_come_with_a_reason() {
        let (mut ride, mut head) = start();
        lift_hill_up(&mut ride, &mut head);
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, -50),
        );
        add(
            &mut ride,
            &mut head,
            plain(PieceType::SlopeChange, 12, 0, 0),
        );
        let base = plain(PieceType::Turn, 8, 0, 0);
        let turns = [0, 15, 90, 180];
        let blocked = blocked_values(&ride, &head, &base, Setting::Turn, &turns);
        assert_eq!(blocked[0], None, "no turn is fine");
        assert!(
            blocked[3].as_ref().is_some_and(|r| r.contains("widen")),
            "{blocked:?}"
        );
        // A turn that isn't in 15 degree steps is refused with the piece's own words.
        let odd = blocked_values(&ride, &head, &base, Setting::Turn, &[10]);
        assert!(odd[0].as_ref().unwrap().contains("steps of 15"));
    }
}
