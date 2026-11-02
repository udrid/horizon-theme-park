//! Coaster pieces for the on-screen builder (M3b, D75).
//!
//! A *piece* is one smooth stretch of track that grows from the end of the track you already
//! have. Think of drawing with a pen: the pen has a place and a direction (the **head**), and
//! each piece moves it forward along an easy curve, then leaves the pen pointing the new way.
//! Because the next piece starts exactly where the last one ended, with the same direction and
//! the same slope, the joins are smooth.
//!
//! Under the hood a piece is just a few new track nodes (D42), so a ride stays the same kind of
//! file (D20) and the track wand can still edit it.
//!
//! Angles in the settings are in degrees (as saved files and the screen use, D11). `heading` is
//! Minecraft's *yaw*: 0 faces south (+Z), 90 faces west (-X). A **positive turn is to the right**
//! (clockwise seen from above). A **positive slope climbs**.

use crate::save::{Kind, NodeFile};
use glam::DVec3;

/// Shortest and longest piece, in blocks (D75).
pub const MIN_LENGTH: u32 = 1;
pub const MAX_LENGTH: u32 = 32;
/// Biggest turn either way, and the step the screen moves in, in degrees.
pub const MAX_TURN: i32 = 180;
pub const TURN_STEP: i32 = 15;
/// Steepest slope either way, and the step, in degrees.
pub const MAX_SLOPE: i32 = 70;
pub const SLOPE_STEP: i32 = 5;
/// One new node about every this many blocks of track (the curve is drawn through the nodes).
pub const NODE_SPACING: f64 = 2.0;
/// Nodes closer than this to each other are refused by `create` (D65), so a piece may not make them.
pub const MIN_NODE_GAP: f64 = 1.0;
/// Track must stay at least this far from the rest of the track (rails and trains need room), in
/// blocks, except near the place where it joins.
pub const CLEARANCE: f64 = 3.5;
/// The stretch of track next to a join that is not compared (it is, of course, close).
pub const JOIN_ZONE: f64 = 6.0;
/// Steps used to walk along a piece when working out where it ends (more = more exact).
const WALK_STEPS: usize = 64;

/// What the pen looks like at the end of the track: where, which way, how steep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    /// The track point (where the last node is), in world blocks.
    pub pos: DVec3,
    /// Yaw in degrees (0 = south, 90 = west).
    pub heading: f64,
    /// Slope in degrees (positive = climbing).
    pub slope: f64,
}

impl Head {
    /// The unit direction the track points in at this head.
    pub fn direction(&self) -> DVec3 {
        direction(self.heading.to_radians(), self.slope.to_radians())
    }
}

/// The unit vector for a yaw and a slope, both in radians.
fn direction(heading: f64, slope: f64) -> DVec3 {
    DVec3::new(
        -heading.sin() * slope.cos(),
        slope.sin(),
        heading.cos() * slope.cos(),
    )
}

/// The four kinds of piece in the panel (D75). The kind decides which settings count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceType {
    /// Keeps the heading and the slope.
    Straight,
    /// Changes the heading, keeps the slope.
    Turn,
    /// Keeps the heading, changes the slope.
    SlopeChange,
    /// Changes both.
    TurnSlope,
}

impl PieceType {
    pub const ALL: [PieceType; 4] = [
        PieceType::Straight,
        PieceType::Turn,
        PieceType::SlopeChange,
        PieceType::TurnSlope,
    ];

    /// The name shown on the panel.
    pub fn name(self) -> &'static str {
        match self {
            PieceType::Straight => "Straight",
            PieceType::Turn => "Turn",
            PieceType::SlopeChange => "Slope change",
            PieceType::TurnSlope => "Turn + slope",
        }
    }
}

/// Everything the player sets for the next piece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceSettings {
    pub piece: PieceType,
    /// Length in blocks of track, 1 to 32.
    pub length: u32,
    /// Heading change in degrees, -180 to 180 in steps of 15 (used by Turn and Turn + slope).
    pub turn: i32,
    /// The slope at the END of the piece in degrees, -70 to 70 in steps of 5 (used by Slope change
    /// and Turn + slope).
    pub slope: i32,
    /// What the track does along this piece: track, lift or brake (a station comes only from
    /// "New coaster").
    pub element: Kind,
}

impl PieceSettings {
    /// A plain straight of `length` blocks.
    pub fn straight(length: u32) -> PieceSettings {
        PieceSettings {
            piece: PieceType::Straight,
            length,
            turn: 0,
            slope: 0,
            element: Kind::Track,
        }
    }

    /// The turn that really happens, after the piece kind has had its say.
    pub fn effective_turn(&self) -> f64 {
        match self.piece {
            PieceType::Turn | PieceType::TurnSlope => f64::from(self.turn),
            _ => 0.0,
        }
    }

    /// The slope the piece ends at, after the piece kind has had its say.
    pub fn effective_end_slope(&self, head: &Head) -> f64 {
        match self.piece {
            PieceType::SlopeChange | PieceType::TurnSlope => f64::from(self.slope),
            _ => head.slope,
        }
    }

    /// Why these settings are not allowed, in plain words, or `None` when they are fine.
    pub fn problem(&self) -> Option<String> {
        if !(MIN_LENGTH..=MAX_LENGTH).contains(&self.length) {
            return Some(format!(
                "Length must be {MIN_LENGTH} to {MAX_LENGTH} blocks."
            ));
        }
        if self.turn.abs() > MAX_TURN || self.turn % TURN_STEP != 0 {
            return Some(format!(
                "Turn must be -{MAX_TURN} to {MAX_TURN} degrees in steps of {TURN_STEP}."
            ));
        }
        if self.slope.abs() > MAX_SLOPE || self.slope % SLOPE_STEP != 0 {
            return Some(format!(
                "Slope must be -{MAX_SLOPE} to {MAX_SLOPE} degrees in steps of {SLOPE_STEP}."
            ));
        }
        if self.element == Kind::Station {
            return Some("A station only comes with \"New coaster\".".to_string());
        }
        None
    }
}

/// What a piece adds to the track.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// The new nodes, in order. All but the last carry the piece's element; the last is plain
    /// track until the next piece says otherwise.
    pub nodes: Vec<NodeFile>,
    /// The kind the node that was last BEFORE this piece must take: a node's kind is what the
    /// track does from it to the next node, so the stretch into this piece belongs to it.
    pub start_kind: Kind,
    /// Where the pen is afterwards.
    pub end: Head,
    /// The TIGHTEST radius of the turn in blocks (the screen shows it), or `None` for no turn.
    pub radius: Option<f64>,
}

/// The easing used for slope changes: 0 at the start, 1 at the end, flat at both ends so the
/// slope eases in and out and the climb never kinks (D58).
fn ease(u: f64) -> f64 {
    u * u * (3.0 - 2.0 * u)
}

/// Heading and slope (radians) after walking `u` (0 to 1) of the piece.
fn at(head: &Head, turn: f64, end_slope: f64, u: f64) -> (f64, f64) {
    // Both the heading and the slope ease in and out (smoothstep), so the sideways push ramps up
    // gently instead of snapping on at the start of a turn, like the transition curves of a real
    // coaster. The curvature is 0 at both ends and peaks at 1.5 times the average in the middle.
    let heading = (head.heading + turn * ease(u)).to_radians();
    let slope = (head.slope + (end_slope - head.slope) * ease(u)).to_radians();
    (heading, slope)
}

/// The point reached after walking `fraction` (0 to 1) of the piece's arc length.
fn walk(head: &Head, turn: f64, end_slope: f64, length: f64, fraction: f64) -> DVec3 {
    let steps = ((WALK_STEPS as f64 * fraction).ceil() as usize).max(1);
    let du = fraction / steps as f64;
    let mut pos = head.pos;
    for i in 0..steps {
        // The middle of each little step gives a good direction for it (the "midpoint rule").
        let (h, s) = at(head, turn, end_slope, (i as f64 + 0.5) * du);
        pos += direction(h, s) * (length * du);
    }
    pos
}

/// Points along the polyline through `nodes`, about one block apart, each with the distance
/// walked from the first node.
fn along(nodes: &[DVec3]) -> Vec<(DVec3, f64)> {
    let mut out = Vec::new();
    let mut walked = 0.0;
    for pair in nodes.windows(2) {
        let length = pair[0].distance(pair[1]);
        let count = (length.ceil() as usize).max(1);
        for k in 0..count {
            let t = k as f64 / count as f64;
            out.push((pair[0].lerp(pair[1], t), walked + length * t));
        }
        walked += length;
    }
    if let Some(last) = nodes.last() {
        out.push((*last, walked));
    }
    out
}

/// Does the new stretch come within `CLEARANCE` blocks of the rest of the track? Returns the
/// number (1 is the first) of the nearest old node to the worst spot. The new stretch starts at
/// the last old node. The `JOIN_ZONE` blocks of old track next to that join are ignored, and so
/// are those at the start of the lap when `closes_lap` (the finish arrives there).
pub fn hits_track(old: &[NodeFile], new: &[NodeFile], closes_lap: bool) -> Option<usize> {
    let to_vec = |n: &NodeFile| DVec3::new(n.pos[0], n.pos[1], n.pos[2]);
    let old_points: Vec<DVec3> = old.iter().map(to_vec).collect();
    let &head = old_points.last()?;
    let old_samples = along(&old_points);
    let total = old_samples.last().map_or(0.0, |s| s.1);
    let mut new_points = vec![head];
    new_points.extend(new.iter().map(to_vec));
    let new_samples = along(&new_points);
    let mut worst: Option<(f64, f64)> = None; // (distance, old arc length)
    for &(a, arc) in &old_samples {
        if total - arc < JOIN_ZONE || (closes_lap && arc < JOIN_ZONE) {
            continue;
        }
        for &(b, _) in &new_samples {
            let d = a.distance(b);
            if d < CLEARANCE && worst.is_none_or(|w| d < w.0) {
                worst = Some((d, arc));
            }
        }
    }
    let (_, arc) = worst?;
    // The node at or just before that arc length.
    let mut walked = 0.0;
    for (i, pair) in old_points.windows(2).enumerate() {
        walked += pair[0].distance(pair[1]);
        if walked >= arc {
            return Some(i + 1);
        }
    }
    Some(old.len())
}

/// Build the next piece from `head`, or say in plain words why it can't be built.
pub fn build_piece(head: &Head, settings: &PieceSettings) -> Result<Piece, String> {
    if let Some(problem) = settings.problem() {
        return Err(problem);
    }
    let turn = settings.effective_turn();
    let end_slope = settings.effective_end_slope(head);
    let length = f64::from(settings.length);

    // How many nodes: about one per NODE_SPACING blocks, at least one (the end).
    let count = ((length / NODE_SPACING).round() as usize).max(1);
    let mut nodes = Vec::with_capacity(count);
    let mut previous = head.pos;
    for i in 1..=count {
        let pos = walk(head, turn, end_slope, length, i as f64 / count as f64);
        if pos.distance(previous) < MIN_NODE_GAP {
            return Err(if turn == 0.0 {
                "That piece is too short.".to_string()
            } else {
                "Too short for that much turn: make it longer or turn less.".to_string()
            });
        }
        previous = pos;
        let kind = if i == count {
            Kind::Track
        } else {
            settings.element
        };
        nodes.push(NodeFile {
            pos: [pos.x, pos.y, pos.z],
            roll: 0.0,
            kind,
        });
    }
    let end = Head {
        pos: previous,
        heading: head.heading + turn,
        slope: end_slope,
    };
    // The tightest radius, in the middle of the turn: the heading changes 1.5 times faster there
    // than the average, so radius = length / (1.5 x angle in radians).
    let radius = (turn != 0.0).then(|| length / (1.5 * turn.abs().to_radians()));
    Ok(Piece {
        nodes,
        start_kind: settings.element,
        end,
        radius,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Head {
        Head {
            pos: DVec3::new(10.0, 70.0, 20.0),
            heading: 0.0,
            slope: 0.0,
        }
    }

    fn settings(piece: PieceType, length: u32, turn: i32, slope: i32) -> PieceSettings {
        PieceSettings {
            piece,
            length,
            turn,
            slope,
            element: Kind::Track,
        }
    }

    /// The direction the track really leaves the last two nodes in.
    fn exit_direction(h: &Head, p: &Piece) -> DVec3 {
        let n = p.nodes.len();
        let last = DVec3::from(p.nodes[n - 1].pos);
        let before = if n >= 2 {
            DVec3::from(p.nodes[n - 2].pos)
        } else {
            h.pos
        };
        (last - before).normalize()
    }

    fn angle_between(a: DVec3, b: DVec3) -> f64 {
        a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[test]
    fn a_straight_goes_the_right_way_and_distance() {
        // Heading 0 faces south, +Z.
        let p = build_piece(&head(), &PieceSettings::straight(8)).unwrap();
        let end = p.nodes.last().unwrap().pos;
        assert!((end[0] - 10.0).abs() < 1e-9);
        assert!((end[1] - 70.0).abs() < 1e-9);
        assert!((end[2] - 28.0).abs() < 1e-9);
        assert_eq!(p.end.heading, 0.0);
        assert_eq!(p.end.slope, 0.0);
        assert!(p.radius.is_none());
    }

    #[test]
    fn heading_90_faces_west() {
        let h = Head {
            heading: 90.0,
            ..head()
        };
        let p = build_piece(&h, &PieceSettings::straight(4)).unwrap();
        let end = p.nodes.last().unwrap().pos;
        assert!((end[0] - 6.0).abs() < 1e-9, "x was {}", end[0]);
        assert!((end[2] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn nodes_are_about_two_blocks_apart_and_never_closer_than_one() {
        for length in MIN_LENGTH..=MAX_LENGTH {
            let p = build_piece(&head(), &PieceSettings::straight(length)).unwrap();
            let mut prev = head().pos;
            for node in &p.nodes {
                let pos = DVec3::from(node.pos);
                assert!(pos.distance(prev) >= MIN_NODE_GAP - 1e-9);
                prev = pos;
            }
            let expected = ((f64::from(length) / 2.0).round() as usize).max(1);
            assert_eq!(p.nodes.len(), expected);
        }
    }

    #[test]
    fn a_right_turn_of_90_degrees_has_the_right_tightest_radius_and_ends_facing_west() {
        let p = build_piece(&head(), &settings(PieceType::Turn, 16, 90, 0)).unwrap();
        assert_eq!(p.end.heading, 90.0);
        // Tightest radius = length / (1.5 x angle), because the heading eases in and out.
        let r = p.radius.unwrap();
        assert!((r - 16.0 / (1.5 * std::f64::consts::PI / 2.0)).abs() < 1e-9);
        // Facing south, a right turn ends to the west (-X) and further south (+Z).
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        assert!(end.x < 10.0 - 5.0 && end.z > 20.0 + 5.0, "{end:?}");
    }

    #[test]
    fn a_left_turn_goes_the_other_way() {
        let p = build_piece(&head(), &settings(PieceType::Turn, 16, -90, 0)).unwrap();
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        assert!(end.x > 10.0 + 5.0);
        assert_eq!(p.end.heading, -90.0);
    }

    #[test]
    fn a_slope_change_ends_at_the_asked_slope_and_height() {
        let p = build_piece(&head(), &settings(PieceType::SlopeChange, 20, 0, 30)).unwrap();
        assert_eq!(p.end.slope, 30.0);
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        // It climbs, but less than a straight 30 degree climb of 20 blocks (the ease-in costs height).
        assert!(end.y > 70.0 + 2.0 && end.y < 70.0 + 20.0 * 30f64.to_radians().sin());
        assert!((end.x - 10.0).abs() < 1e-9, "no sideways drift");
    }

    #[test]
    fn a_straight_keeps_the_slope_it_starts_with() {
        let h = Head {
            slope: 40.0,
            ..head()
        };
        let p = build_piece(&h, &PieceSettings::straight(10)).unwrap();
        assert_eq!(p.end.slope, 40.0);
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        assert!((end.y - (70.0 + 10.0 * 40f64.to_radians().sin())).abs() < 1e-9);
    }

    #[test]
    fn the_turn_setting_is_ignored_by_a_slope_change_and_the_slope_by_a_turn() {
        let a = build_piece(&head(), &settings(PieceType::SlopeChange, 10, 90, 20)).unwrap();
        assert_eq!(a.end.heading, 0.0);
        let b = build_piece(&head(), &settings(PieceType::Turn, 10, 90, 20)).unwrap();
        assert_eq!(b.end.slope, 0.0);
    }

    #[test]
    fn every_piece_joins_the_next_with_matching_direction_and_slope() {
        // Walk a long, mixed ride and check the pen's direction matches the nodes' direction at
        // every join. The nodes' own exit direction is a chord, so allow a few degrees.
        let mut h = head();
        let list = [
            settings(PieceType::Straight, 8, 0, 0),
            settings(PieceType::SlopeChange, 12, 0, 35),
            settings(PieceType::Straight, 6, 0, 0),
            settings(PieceType::SlopeChange, 14, 0, -55),
            settings(PieceType::Turn, 16, 90, 0),
            settings(PieceType::TurnSlope, 24, -120, 10),
            settings(PieceType::Turn, 12, 45, 0),
            settings(PieceType::Straight, 32, 0, 0),
        ];
        for s in list {
            let p = build_piece(&h, &s).unwrap();
            let chord = exit_direction(&h, &p);
            let a = angle_between(chord, p.end.direction());
            assert!(
                a < 7.0,
                "{:?}: chord is {a:.1} degrees off the pen",
                s.piece
            );
            assert_eq!(
                p.end.pos,
                DVec3::from(p.nodes.last().unwrap().pos),
                "the pen sits on the last node"
            );
            h = p.end;
        }
    }

    #[test]
    fn two_halves_make_the_same_straight_as_one_whole() {
        let one = build_piece(&head(), &PieceSettings::straight(16)).unwrap();
        let half = build_piece(&head(), &PieceSettings::straight(8)).unwrap();
        let two = build_piece(&half.end, &PieceSettings::straight(8)).unwrap();
        assert!(one.end.pos.distance(two.end.pos) < 1e-9);
    }

    #[test]
    fn the_element_goes_on_every_node_but_the_last_and_on_the_node_before() {
        let s = PieceSettings {
            element: Kind::Lift,
            ..settings(PieceType::SlopeChange, 20, 0, 30)
        };
        let p = build_piece(&head(), &s).unwrap();
        assert_eq!(p.start_kind, Kind::Lift);
        let n = p.nodes.len();
        assert!(n >= 2);
        assert!(p.nodes[..n - 1].iter().all(|x| x.kind == Kind::Lift));
        assert_eq!(p.nodes[n - 1].kind, Kind::Track);
    }

    #[test]
    fn bad_settings_are_refused_in_plain_words() {
        let bad = [
            settings(PieceType::Straight, 0, 0, 0),
            settings(PieceType::Straight, 33, 0, 0),
            settings(PieceType::Turn, 8, 200, 0),
            settings(PieceType::Turn, 8, 10, 0),
            settings(PieceType::SlopeChange, 8, 0, 75),
            settings(PieceType::SlopeChange, 8, 0, 7),
            PieceSettings {
                element: Kind::Station,
                ..settings(PieceType::Straight, 8, 0, 0)
            },
        ];
        for s in bad {
            let e = build_piece(&head(), &s).unwrap_err();
            assert!(!e.is_empty(), "{s:?}");
        }
    }

    #[test]
    fn a_one_block_piece_with_a_turn_is_too_short_but_a_straight_is_fine() {
        assert!(build_piece(&head(), &PieceSettings::straight(1)).is_ok());
        let e = build_piece(&head(), &settings(PieceType::Turn, 1, 15, 0)).unwrap_err();
        assert!(e.contains("Too short"));
    }

    #[test]
    fn a_full_half_turn_comes_back_the_other_way_and_ends_level_with_where_it_began() {
        let p = build_piece(&head(), &settings(PieceType::Turn, 32, 180, 0)).unwrap();
        assert_eq!(p.end.heading, 180.0);
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        // The easing is symmetric, so the forward and back movements cancel exactly.
        assert!((end.z - 20.0).abs() < 0.05, "z {}", end.z);
        assert!(end.x < 10.0 - 12.0 && end.x > 10.0 - 26.0, "x {}", end.x);
    }

    #[test]
    fn the_sideways_curvature_is_tiny_at_both_ends_of_a_turn_and_peaks_in_the_middle() {
        let h = head();
        let rate = |u: f64| {
            // Heading change per unit of the piece, by a small difference.
            let (a, _) = at(&h, 90.0, 0.0, u - 1e-4);
            let (b, _) = at(&h, 90.0, 0.0, u + 1e-4);
            (b - a) / 2e-4
        };
        let peak = rate(0.5);
        assert!((peak - 1.5 * std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        assert!(rate(0.02) < 0.1 * peak, "start {}", rate(0.02));
        assert!(rate(0.98) < 0.1 * peak, "end {}", rate(0.98));
    }

    #[test]
    fn two_turns_in_a_row_join_with_the_same_heading_and_slope() {
        let first = build_piece(&head(), &settings(PieceType::TurnSlope, 20, 45, 25)).unwrap();
        let second = build_piece(&first.end, &settings(PieceType::TurnSlope, 20, 45, -10)).unwrap();
        assert_eq!(first.end.heading, 45.0);
        assert_eq!(first.end.slope, 25.0);
        assert_eq!(second.end.heading, 90.0);
        // The second piece leaves in the direction the first arrived in.
        let leave = (DVec3::from(second.nodes[0].pos) - first.end.pos).normalize();
        assert!(angle_between(leave, first.end.direction()) < 8.0);
    }

    fn node_at(x: f64, y: f64, z: f64) -> NodeFile {
        NodeFile {
            pos: [x, y, z],
            roll: 0.0,
            kind: Kind::Track,
        }
    }

    /// A straight old track along +Z from z = 0 to z = 40 at height 70.
    fn old_track() -> Vec<NodeFile> {
        (0..=20)
            .map(|i| node_at(0.0, 70.0, 2.0 * i as f64))
            .collect()
    }

    /// A path that leaves the old end sideways and crosses the old track at z = 20, at `height`.
    fn crossing_path(height: f64) -> Vec<NodeFile> {
        let mut path = vec![node_at(-6.0, 70.0, 44.0), node_at(-9.0, height, 30.0)];
        path.extend((0..=6).map(|i| node_at(-9.0 + 3.0 * i as f64, height, 20.0)));
        path
    }

    #[test]
    fn a_new_stretch_crossing_the_old_track_at_the_same_height_is_refused() {
        let hit = hits_track(&old_track(), &crossing_path(70.0), false);
        // It crosses near z = 20, which is node 11 (nodes are 2 blocks apart, 1 is the first).
        assert!(matches!(hit, Some(10..=12)), "{hit:?}");
    }

    #[test]
    fn the_same_crossing_4_blocks_above_is_allowed() {
        assert_eq!(hits_track(&old_track(), &crossing_path(74.0), false), None);
    }

    #[test]
    fn leaving_straight_on_from_the_end_is_not_a_collision_with_itself() {
        let ahead: Vec<NodeFile> = (1..=8)
            .map(|i| node_at(0.0, 70.0, 40.0 + 2.0 * i as f64))
            .collect();
        assert_eq!(hits_track(&old_track(), &ahead, false), None);
    }

    #[test]
    fn a_pass_beside_the_middle_is_caught_but_arriving_at_the_start_of_the_lap_is_not() {
        // Running back alongside the old track, 2 blocks away: caught.
        let beside: Vec<NodeFile> = (1..=6)
            .map(|i| node_at(2.0, 70.0, 40.0 - 4.0 * i as f64))
            .collect();
        assert!(hits_track(&old_track(), &beside, true).is_some());
        // A lap closed by a path that goes round and arrives at the first node along its line:
        // allowed when it closes the lap (the first 6 blocks are ignored), caught if it did not.
        let around = vec![
            node_at(12.0, 70.0, 44.0),
            node_at(12.0, 70.0, 0.0),
            node_at(12.0, 70.0, -12.0),
            node_at(0.0, 70.0, -12.0),
            node_at(0.0, 70.0, -4.0),
            node_at(0.0, 70.0, -1.0),
        ];
        assert_eq!(hits_track(&old_track(), &around, true), None);
        assert!(hits_track(&old_track(), &around, false).is_some());
    }
}
