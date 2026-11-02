//! The finish helper (M3b T3, D75): from the end of your track, draw a smooth path back to the
//! station, with a straight brake run just before it, and check it is realistic and safe.
//!
//! How it works, in plain words:
//!   1. The **brake run** is a straight stretch of brake track that ends at the station's first
//!      node, pointing the way the station points.
//!   2. We join the end of your track to the start of the brake run with one smooth curve (a
//!      "Hermite curve": a curve given by two points and the direction to leave and arrive in).
//!      The "stiffness" of the curve is the length of those directions; we try a few and keep
//!      the first one that passes every check.
//!   3. Every try is checked like a finished ride: the track must be valid (D65), the train must
//!      be able to stop in the station and never stall (D68), and the new stretch must stay
//!      within the realism limits (D58).
//!   4. If nothing passes, we say why in plain words.

use crate::builder::{cars_of, course_from, physics_of, spacing_of};
use crate::pieces::{Head, MIN_NODE_GAP, NODE_SPACING};
use crate::report::{self, Blocker};
use crate::save::{Kind, NodeFile, RideFile};
use glam::DVec3;

/// Length of the brake run before the station, in blocks.
pub const BRAKE_RUN: f64 = 14.0;
/// The curve must be at least this long to be worth drawing, in blocks.
const MIN_CURVE: f64 = 6.0;
/// How stiff the curve is, as a multiple of the straight distance to its end. Tried in order.
const STIFFNESS: [f64; 5] = [0.7, 1.0, 1.4, 1.9, 2.6];
/// The steepest average fall (degrees) we are willing to draw: a real drop is 50-65 (D58), but
/// a helper that dives to the station would be a surprise, so we stay well below.
const MAX_AVERAGE_FALL: f64 = 40.0;
/// Steps used to measure the curve.
const SAMPLES: usize = 200;
/// An issue counts as "ours" if it is within this many blocks of the new stretch.
const NEARBY: f64 = 4.0;

/// The helper's answer: the nodes to add. The last of them is the node before the station's
/// first node (the loop closes into it).
#[derive(Clone, Debug, PartialEq)]
pub struct Finish {
    pub nodes: Vec<NodeFile>,
    /// The curve and brake run lengths in blocks, for the screen.
    pub curve_length: f64,
    pub brake_length: f64,
}

fn vec(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

fn node(p: DVec3, kind: Kind) -> NodeFile {
    NodeFile {
        pos: [p.x, p.y, p.z],
        roll: 0.0,
        kind,
    }
}

/// The point of the Hermite curve at `t` (0 to 1): leaves `p0` along `m0`, arrives at `p1` along `m1`.
fn hermite(p0: DVec3, m0: DVec3, p1: DVec3, m1: DVec3, t: f64) -> DVec3 {
    let (t2, t3) = (t * t, t * t * t);
    p0 * (2.0 * t3 - 3.0 * t2 + 1.0)
        + m0 * (t3 - 2.0 * t2 + t)
        + p1 * (-2.0 * t3 + 3.0 * t2)
        + m1 * (t3 - t2)
}

/// Why no smooth path exists, if the plain geometry already says so.
fn geometry_problem(head: &Head, target: DVec3, entry: DVec3) -> Option<String> {
    let to = target - head.pos;
    let flat = DVec3::new(to.x, 0.0, to.z);
    let distance = flat.length();
    if to.length() < MIN_CURVE {
        return Some(
            "Your track already ends at the station's brake run. Add a piece or two first, then press H."
                .to_string(),
        );
    }
    let fall = -to.y;
    let allowed = distance * MAX_AVERAGE_FALL.to_radians().tan();
    if fall > allowed {
        let extra = fall - allowed;
        return Some(format!(
            "Too high above the station: build down about {:.0} blocks first, or end farther away.",
            extra.ceil()
        ));
    }
    // Facing away from the station: a smooth path would need a tight U-turn.
    let ahead = DVec3::new(head.direction().x, 0.0, head.direction().z).normalize_or_zero();
    if distance > 0.0 && ahead.dot(flat / distance) < -0.5 {
        return Some(
            "Your track points away from the station. Turn it around with a wide turn first, then press H."
                .to_string(),
        );
    }
    // Arriving the wrong way round: the curve must come up to the brake run from behind it.
    let behind = (target - entry).normalize_or_zero();
    if distance > 0.0 && behind.dot(flat / distance) > 0.5 && to.length() < 3.0 * BRAKE_RUN {
        return Some(
            "Your track ends past the station's brake run. Build a wide turn so it comes back from behind the station."
                .to_string(),
        );
    }
    None
}

/// Draw the finish for `ride` from `head`, or say in plain words why not.
pub fn finish(ride: &RideFile, head: &Head) -> Result<Finish, String> {
    if ride.nodes.len() < 2 || ride.nodes[0].kind != Kind::Station {
        return Err("The finish helper needs a station to finish in.".to_string());
    }
    let station_start = vec(ride.nodes[0].pos);
    let station_dir = (vec(ride.nodes[1].pos) - station_start).normalize_or_zero();
    if station_dir.y.abs() > 0.05 {
        return Err("The station is not flat, so there is no clean brake run.".to_string());
    }
    // Where the smooth curve must end: the start of the brake run.
    let target = station_start - station_dir * BRAKE_RUN;
    if let Some(problem) = geometry_problem(head, target, station_start) {
        return Err(problem);
    }

    let chord = head.pos.distance(target);
    let mut last_reason = String::from("No smooth path found.");
    for stiffness in STIFFNESS {
        let m0 = head.direction() * (chord * stiffness);
        let m1 = station_dir * (chord * stiffness);
        let (nodes, length) = sample(head.pos, m0, target, m1);
        if nodes.len() < 2 {
            continue;
        }
        // The brake run: nodes every NODE_SPACING from the curve's end to the station start.
        let mut new_nodes = nodes;
        let brake_nodes = (BRAKE_RUN / NODE_SPACING).round() as usize;
        for k in 1..brake_nodes {
            let p = target + station_dir * (BRAKE_RUN / brake_nodes as f64 * k as f64);
            new_nodes.push(node(p, Kind::Brake));
        }
        // The curve's last node (the target) starts the brake zone.
        let curve_end = new_nodes.len() - (brake_nodes - 1) - 1;
        new_nodes[curve_end].kind = Kind::Brake;
        match check(ride, &new_nodes) {
            Ok(()) => {
                return Ok(Finish {
                    nodes: new_nodes,
                    curve_length: length,
                    brake_length: BRAKE_RUN,
                });
            }
            Err(reason) => last_reason = reason,
        }
    }
    Err(last_reason)
}

/// Sample the curve into nodes about `NODE_SPACING` apart, from just after the start up to and
/// including the end. Also returns the curve's length.
fn sample(p0: DVec3, m0: DVec3, p1: DVec3, m1: DVec3) -> (Vec<NodeFile>, f64) {
    let points: Vec<DVec3> = (0..=SAMPLES)
        .map(|i| hermite(p0, m0, p1, m1, i as f64 / SAMPLES as f64))
        .collect();
    let mut length = 0.0;
    let mut along = vec![0.0];
    for pair in points.windows(2) {
        length += pair[0].distance(pair[1]);
        along.push(length);
    }
    let count = ((length / NODE_SPACING).round() as usize).max(1);
    let mut nodes = Vec::new();
    for k in 1..=count {
        let wanted = length * k as f64 / count as f64;
        let i = along.partition_point(|&a| a < wanted).min(SAMPLES);
        nodes.push(node(points[i], Kind::Track));
    }
    (nodes, length)
}

/// Try the ride with the new nodes added: valid, stoppable, and within the limits near the new part.
fn check(ride: &RideFile, new_nodes: &[NodeFile]) -> Result<(), String> {
    let mut trial = ride.clone();
    trial.nodes.extend(new_nodes.iter().cloned());
    // Nodes too close together (the end of a tight curve) would be refused by `create`.
    let mut previous = vec(ride.nodes[ride.nodes.len() - 1].pos);
    for n in new_nodes {
        let p = vec(n.pos);
        if p.distance(previous) < MIN_NODE_GAP {
            return Err("The curve is too tight to draw: end your track farther from the station or turn earlier.".to_string());
        }
        previous = p;
    }
    let course = course_from(&trial)?;
    let cars = cars_of(&trial);
    let spacing = spacing_of(&trial);
    let physics = physics_of(&trial);
    if let Some(blocker) = report::blocker(&course, cars, spacing, &physics) {
        return Err(match blocker {
            Blocker::Overshoot { .. } => {
                "The train arrives too fast for the brake run. Make the last hill lower, or end farther from the station.".to_string()
            }
            Blocker::Stall { .. } => {
                "The train runs out of speed before the station. Make the last hill lower so it keeps its speed, or add a lift.".to_string()
            }
        });
    }
    // Realism near the new stretch only: older parts already show in the report.
    let ours: Vec<DVec3> = new_nodes.iter().map(|n| vec(n.pos)).collect();
    let analysis = report::analyse(&course, cars, spacing, &physics);
    for issue in analysis.issues {
        let near = issue
            .at
            .is_none_or(|at| ours.iter().any(|p| p.distance(at) < NEARBY));
        if near {
            return Err(format!("{}.", issue.text.trim_end_matches('.')));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::{PieceSettings, PieceType, build_piece};

    fn start() -> (RideFile, Head) {
        let n = |z: f64, kind: Kind| NodeFile {
            pos: [0.0, 70.0, z],
            roll: 0.0,
            kind,
        };
        let nodes = vec![
            n(0.0, Kind::Station),
            n(4.0, Kind::Station),
            n(8.0, Kind::Station),
            n(12.0, Kind::Track),
        ];
        let head = Head {
            pos: DVec3::new(0.0, 70.0, 12.0),
            heading: 0.0,
            slope: 0.0,
        };
        (RideFile::new("t", nodes), head)
    }

    fn add(
        ride: &mut RideFile,
        head: &mut Head,
        piece: PieceType,
        length: u32,
        turn: i32,
        slope: i32,
        element: Kind,
    ) {
        let s = PieceSettings {
            piece,
            length,
            turn,
            slope,
            element,
        };
        let p = build_piece(head, &s).unwrap();
        let last = ride.nodes.len() - 1;
        ride.nodes[last].kind = p.start_kind;
        ride.nodes.extend(p.nodes.iter().cloned());
        *head = p.end;
    }

    /// A lift hill south, a gentle drop, then three left turns round to a head that points west
    /// towards the station's approach.
    fn ride_round_the_station() -> (RideFile, Head) {
        let (mut r, mut h) = start();
        let t = PieceType::SlopeChange;
        add(&mut r, &mut h, t, 24, 0, 25, Kind::Lift);
        add(&mut r, &mut h, PieceType::Straight, 10, 0, 0, Kind::Lift);
        add(&mut r, &mut h, t, 12, 0, 0, Kind::Lift);
        add(&mut r, &mut h, t, 20, 0, -30, Kind::Track);
        add(&mut r, &mut h, t, 14, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Turn, 32, -90, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Straight, 20, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Turn, 32, -90, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Straight, 32, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Straight, 32, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Straight, 32, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Straight, 32, 0, 0, Kind::Track);
        add(&mut r, &mut h, PieceType::Turn, 32, -90, 0, Kind::Track);
        (r, h)
    }

    #[test]
    fn it_finishes_a_ride_round_the_station() {
        let (ride, head) = ride_round_the_station();
        let f = finish(&ride, &head).unwrap();
        // The brake run is made of brake nodes and the loop now works as a real ride.
        assert!(f.nodes.iter().filter(|n| n.kind == Kind::Brake).count() >= 5);
        let mut done = ride.clone();
        done.nodes.extend(f.nodes.iter().cloned());
        assert!(course_from(&done).is_ok());
        let course = course_from(&done).unwrap();
        assert!(report::blocker(&course, 3, spacing_of(&done), &physics_of(&done)).is_none());
        // The last node sits on the station's line, in front of its first node.
        let last = vec(f.nodes.last().unwrap().pos);
        assert!(
            last.x.abs() < 0.01 && last.z < 0.0 && last.z > -3.0,
            "{last:?}"
        );
    }

    #[test]
    fn the_curve_arrives_along_the_station_direction() {
        let (ride, head) = ride_round_the_station();
        let f = finish(&ride, &head).unwrap();
        let n = f.nodes.len();
        let d = (vec(f.nodes[n - 1].pos) - vec(f.nodes[n - 3].pos)).normalize();
        assert!(d.z > 0.99, "{d:?}");
    }

    #[test]
    fn too_high_above_the_station_says_how_much_to_build_down() {
        let (ride, mut head) = ride_round_the_station();
        head.pos.y += 80.0;
        let e = finish(&ride, &head).unwrap_err();
        assert!(
            e.contains("Too high above the station: build down about"),
            "{e}"
        );
    }

    #[test]
    fn pointing_away_from_the_station_says_to_turn_round() {
        let (ride, mut head) = ride_round_the_station();
        head.heading += 180.0;
        let e = finish(&ride, &head).unwrap_err();
        assert!(e.contains("points away"), "{e}");
    }

    #[test]
    fn a_head_on_top_of_the_brake_run_says_so() {
        let (ride, mut head) = ride_round_the_station();
        head.pos = vec(ride.nodes[0].pos) - DVec3::new(0.0, 0.0, 16.0);
        head.heading = 0.0;
        head.slope = 0.0;
        let e = finish(&ride, &head).unwrap_err();
        assert!(e.contains("already ends"), "{e}");
    }

    #[test]
    fn a_ride_with_no_station_is_refused() {
        let (mut ride, head) = ride_round_the_station();
        ride.nodes[0].kind = Kind::Track;
        assert!(finish(&ride, &head).unwrap_err().contains("station"));
    }

    #[test]
    fn the_same_ride_gives_the_same_finish() {
        let (ride, head) = ride_round_the_station();
        assert_eq!(finish(&ride, &head), finish(&ride, &head));
    }

    #[test]
    fn an_accepted_finish_stays_within_the_realism_limits_near_it() {
        let (ride, head) = ride_round_the_station();
        let f = finish(&ride, &head).unwrap();
        let mut done = ride.clone();
        done.nodes.extend(f.nodes.iter().cloned());
        let course = course_from(&done).unwrap();
        let r = report::analyse(&course, 3, spacing_of(&done), &physics_of(&done));
        let ours: Vec<DVec3> = f.nodes.iter().map(|n| vec(n.pos)).collect();
        for issue in r.issues {
            let near = issue
                .at
                .is_some_and(|at| ours.iter().any(|p| p.distance(at) < NEARBY));
            assert!(!near, "{}", issue.text);
        }
    }

    #[test]
    fn a_sideways_squeeze_is_refused_in_plain_words() {
        // Just beside the approach, pointing the right way: the curve would have to jink sideways.
        let (ride, mut head) = ride_round_the_station();
        head.pos = DVec3::new(6.0, 70.0, -40.0);
        head.heading = 0.0;
        head.slope = 0.0;
        let e = finish(&ride, &head).unwrap_err();
        assert!(!e.is_empty());
        assert!(
            e.contains("wider") || e.contains("too tight") || e.contains("too fast"),
            "{e}"
        );
    }
}
