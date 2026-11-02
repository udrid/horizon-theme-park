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
    /// The radius of the turn in blocks (the screen shows it), or `None` for no turn.
    pub radius: Option<f64>,
}

/// The easing used for slope changes: 0 at the start, 1 at the end, flat at both ends so the
/// slope eases in and out and the climb never kinks (D58).
fn ease(u: f64) -> f64 {
    u * u * (3.0 - 2.0 * u)
}

/// Heading and slope (radians) after walking `u` (0 to 1) of the piece.
fn at(head: &Head, turn: f64, end_slope: f64, u: f64) -> (f64, f64) {
    // The heading changes steadily (a constant-radius arc); the slope eases from the old to the new.
    let heading = (head.heading + turn * u).to_radians();
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
    // A constant-radius arc: length = radius x angle.
    let radius = (turn != 0.0).then(|| length / turn.abs().to_radians());
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
    fn a_right_turn_of_90_degrees_has_the_right_radius_and_ends_facing_west() {
        let p = build_piece(&head(), &settings(PieceType::Turn, 16, 90, 0)).unwrap();
        assert_eq!(p.end.heading, 90.0);
        let r = p.radius.unwrap();
        assert!((r - 16.0 / (std::f64::consts::PI / 2.0)).abs() < 1e-9);
        // Facing south, a right turn ends to the west (-X) and further south (+Z): (r, r) away.
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        assert!((end.x - (10.0 - r)).abs() < 0.01, "x {}", end.x);
        assert!((end.z - (20.0 + r)).abs() < 0.01, "z {}", end.z);
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
    fn a_full_half_circle_comes_back_the_other_way() {
        let p = build_piece(&head(), &settings(PieceType::Turn, 32, 180, 0)).unwrap();
        assert_eq!(p.end.heading, 180.0);
        let end = DVec3::from(p.nodes.last().unwrap().pos);
        let r = p.radius.unwrap();
        assert!((end.z - 20.0).abs() < 0.05, "z {}", end.z);
        assert!((end.x - (10.0 - 2.0 * r)).abs() < 0.05, "x {}", end.x);
    }
}
