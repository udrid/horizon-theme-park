//! What to draw and where: rail pieces, the car and its seats.
//!
//! Everything here is plain maths on a track `Table`. It answers questions such as "how long
//! should each piece of rail be?" and "where, and turned which way, does the car stand when it is
//! `s` blocks along?". `commands.rs` then turns the answers into command text.

use crate::coaster::frame::Frame;
use crate::coaster::table::Table;
use glam::{DMat3, DQuat, DVec3};

/// Longest allowed gap between a straight rail piece and the real curve, in blocks (D29, S3).
pub const MAX_SAG: f64 = 0.03;
/// Rail piece lengths we try, longest first (D29, S3).
pub const PIECE_LENGTHS: [f64; 4] = [4.0, 3.0, 2.0, 1.0];
/// Rail model size: width and height in blocks (the length comes from the piece).
pub const RAIL_WIDTH: f64 = 0.5;
pub const RAIL_HEIGHT: f64 = 0.15;

/// How far the car's model sits above the track point. Tuned in the game (S5).
pub const CAR_LIFT: f64 = 0.5;
/// The car's shell model size (width, height, length). Tuned in the game (S5).
pub const CAR_SCALE: DVec3 = DVec3::new(1.6, 0.9, 3.0);
/// Where each seat sits along the car, in blocks from the car's middle (front, back).
pub const SEAT_OFFSETS: [f64; 2] = [0.7, -0.7];
/// How far a seat (a minecart's bottom) is sent above the track point. Tuned in the game (S5).
/// First try 0.1 left the rider's head inside the car shell (owner's test, 2026-10-31); a rider
/// hangs 1.2125 blocks below the seat, so 1.5 puts their feet just above the car's floor.
pub const SEAT_LIFT: f64 = 1.5;
/// A rider's position relative to their seat's position: NEGATIVE, they hang below it.
/// Measured in the game by the owner on 2026-10-31: seat y 80.3461, rider y 79.1336, same x and z
/// (difference -1.2125). Horizon's own minecart offset is -0.4125; the rest (about -0.8) is
/// probably the cart falling at its top speed between our moves, since we reset its position
/// every tick but not its speed. Mobs (spike R7) were +0.71875.
pub const RIDER_LIFT: f64 = -1.2125;

/// The most a car's straight glide may stray from the track, in blocks (D44).
pub const MAX_GLIDE_SAG: f64 = 0.05;
/// One server tick, in seconds.
pub const TICK_SECONDS: f64 = 0.05;
/// The most ticks between two car moves (D44).
pub const MAX_TELEPORT_TICKS: u32 = 5;

/// A stretch of track covered by one piece of rail: from `s0` to `s1` blocks along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub s0: f64,
    pub s1: f64,
}

/// A rail piece ready to draw: middle point, rotation and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailPose {
    pub pos: DVec3,
    pub rotation: DQuat,
    pub scale: DVec3,
}

/// A place and an orientation for a car or seat, in Minecraft's angles (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: DVec3,
    pub yaw: f64,
    pub pitch: f64,
}

/// How far the real track strays from the straight line between `s0` and `s1`: the "sag".
pub fn sag(table: &Table, s0: f64, s1: f64) -> f64 {
    let a = table.position(s0);
    let b = table.position(s1);
    let chord = b - a;
    let len2 = chord.length_squared();
    let mut worst: f64 = 0.0;
    for i in 1..8 {
        let p = table.position(s0 + (s1 - s0) * i as f64 / 8.0);
        let t = if len2 > 0.0 {
            ((p - a).dot(chord) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        worst = worst.max(p.distance(a + chord * t));
    }
    worst
}

/// Cut the whole lap into rail pieces: each as long as possible (4, 3, 2 or 1 block) while its
/// sag stays within `MAX_SAG`. The pieces touch end to end and cover the lap exactly.
pub fn rail_pieces(table: &Table) -> Vec<Piece> {
    let total = table.length();
    let mut pieces = Vec::new();
    let mut s = 0.0;
    while s < total - 1e-9 {
        let mut chosen = None;
        for &len in &PIECE_LENGTHS {
            let end = (s + len).min(total);
            if sag(table, s, end) <= MAX_SAG {
                chosen = Some(end);
                break;
            }
        }
        // Even 1 block may be too bent on a very tight curve: use 1 block anyway.
        let end = chosen.unwrap_or_else(|| (s + 1.0).min(total));
        pieces.push(Piece { s0: s, s1: end });
        s = end;
    }
    pieces
}

/// Where and how to draw one rail piece: its middle, turned along the chord, with the up arrow
/// from the frame at its middle.
pub fn rail_pose(table: &Table, piece: Piece) -> RailPose {
    let a = table.position(piece.s0);
    let b = table.position(piece.s1);
    let chord = b - a;
    let length = chord.length().max(1e-6);
    let forward = chord / length;
    let mid_frame = table.frame((piece.s0 + piece.s1) / 2.0);
    let up = (mid_frame.up - forward * mid_frame.up.dot(forward)).normalize_or_zero();
    // The model's +X side is the LEFT of the direction of travel (Minecraft's model axes are
    // x = left, y = up, z = forward when facing +Z), so the model's x axis is up × forward, which
    // is `-frame.right`. Using it keeps the matrix a proper rotation (no mirror).
    let model_x = up.cross(forward);
    RailPose {
        pos: (a + b) / 2.0,
        rotation: DQuat::from_mat3(&DMat3::from_cols(model_x, up, forward)),
        scale: DVec3::new(RAIL_WIDTH, RAIL_HEIGHT, length),
    }
}

/// Minecraft's yaw and pitch (degrees) for a direction of travel.
/// Yaw 0 looks towards +Z, yaw -90 towards +X, yaw 180 towards -Z. Pitch is negative going up.
pub fn yaw_pitch(forward: DVec3) -> (f64, f64) {
    let yaw = (-forward.x).atan2(forward.z).to_degrees();
    let pitch = -forward.y.clamp(-1.0, 1.0).asin().to_degrees();
    (yaw, pitch)
}

/// The pose of something that follows the track `lift` blocks above it, at distance `s`.
pub fn pose_at(table: &Table, s: f64, lift: f64) -> Pose {
    pose_from_frame(&table.frame(s), lift)
}

fn pose_from_frame(frame: &Frame, lift: f64) -> Pose {
    let (yaw, pitch) = yaw_pitch(frame.forward);
    Pose {
        pos: frame.pos + frame.up * lift,
        yaw,
        pitch,
    }
}

/// The car's pose when its middle is at `s`.
pub fn car_pose(table: &Table, s: f64) -> Pose {
    pose_at(table, s, CAR_LIFT)
}

/// The pose of seat `index` of a car whose middle is at `s`.
pub fn seat_pose(table: &Table, s: f64, index: usize) -> Pose {
    pose_at(table, s + SEAT_OFFSETS[index], SEAT_LIFT)
}

/// Where a rider's body is when sitting in a seat at `seat` (D49, spike R7).
pub fn rider_point(seat: DVec3) -> DVec3 {
    seat + DVec3::new(0.0, RIDER_LIFT, 0.0)
}

/// How many ticks apart a car's moves are (*k*, D44), worked out once per ride: the largest
/// *k* from 1 to 5 whose straight glide strays at most `MAX_GLIDE_SAG` from the track on its
/// tightest curve at `speed` (blocks per second). A straight chord of length L on a curve of
/// radius R sags about L² / (8R).
pub fn teleport_ticks(table: &Table, speed: f64) -> u32 {
    teleport_ticks_for(speed * speed * table.max_curvature())
}

/// The same choice from the lap's worst `speed² × bend` (S3): a chord of `v · 0.05 · k` blocks on a
/// bend of curvature `c` sags about `chord² · c / 8`.
pub fn teleport_ticks_for(worst_speed_sq_curvature: f64) -> u32 {
    for k in (1..=MAX_TELEPORT_TICKS).rev() {
        let kt = TICK_SECONDS * k as f64;
        if kt * kt * worst_speed_sq_curvature / 8.0 <= MAX_GLIDE_SAG {
            return k;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::oval::{self, demo_nodes};
    use crate::coaster::spline::Track;

    fn oval() -> Table {
        Table::new(&Track::closed(&demo_nodes(DVec3::new(0.0, 70.0, 0.0), 0.0)))
    }

    #[test]
    fn pieces_cover_the_whole_lap_with_no_gap_or_overlap() {
        let t = oval();
        let pieces = rail_pieces(&t);
        assert_eq!(pieces[0].s0, 0.0);
        assert!((pieces.last().unwrap().s1 - t.length()).abs() < 1e-9);
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].s1, pair[1].s0);
        }
        for p in &pieces {
            assert!(p.s1 > p.s0);
        }
    }

    #[test]
    fn every_piece_sags_no_more_than_the_limit() {
        let t = oval();
        for p in rail_pieces(&t) {
            assert!(sag(&t, p.s0, p.s1) <= MAX_SAG + 1e-12, "{p:?}");
        }
    }

    #[test]
    fn straights_use_four_block_pieces_and_curves_shorter_ones() {
        let t = oval();
        let pieces = rail_pieces(&t);
        assert!(
            pieces
                .iter()
                .filter(|p| (p.s1 - p.s0 - 4.0).abs() < 1e-9)
                .count()
                >= 2
        );
        for p in &pieces {
            let mid = t.position((p.s0 + p.s1) / 2.0);
            // Well inside a half circle (the curves are at |z| > 5.5): one-block pieces only.
            if mid.z.abs() > 7.0 {
                assert!(p.s1 - p.s0 <= 1.0 + 1e-9, "{p:?} at z {}", mid.z);
            }
        }
        assert!(pieces.iter().any(|p| p.s1 - p.s0 < 4.0));
    }

    #[test]
    fn a_rail_pose_points_its_long_side_along_the_track() {
        let t = oval();
        let piece = rail_pieces(&t)[0];
        let pose = rail_pose(&t, piece);
        // The model's long side is its local Z: after the rotation it must follow the chord.
        let along = pose.rotation * DVec3::Z;
        let chord = (t.position(piece.s1) - t.position(piece.s0)).normalize();
        assert!(along.dot(chord) > 0.999999);
        // And its up side must be the track's up (the oval is flat).
        assert!((pose.rotation * DVec3::Y).dot(DVec3::Y) > 0.999);
        assert!((pose.scale.z - 4.0).abs() < 1e-6);
    }

    #[test]
    fn yaw_and_pitch_follow_minecrafts_convention() {
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        assert!(close(yaw_pitch(DVec3::Z), (0.0, 0.0)));
        assert!(close(yaw_pitch(DVec3::X), (-90.0, 0.0)));
        assert!(close(yaw_pitch(-DVec3::X), (90.0, 0.0)));
        let back = yaw_pitch(-DVec3::Z);
        assert!((back.0.abs() - 180.0).abs() < 1e-9 && back.1 == 0.0);
        let climbing = DVec3::new(0.0, 1.0, 1.0).normalize();
        assert!(close(yaw_pitch(climbing), (0.0, -45.0)));
    }

    #[test]
    fn the_cars_pose_hovers_above_the_track() {
        let t = oval();
        let p = car_pose(&t, 3.0);
        assert!((p.pos.y - (70.0 + CAR_LIFT)).abs() < 1e-9);
        assert!(p.pitch.abs() < 1e-6);
    }

    #[test]
    fn the_car_glides_fewer_ticks_at_a_time_the_faster_it_goes() {
        // Tightest bend about 5.3 blocks at 8 blocks/s: 3 ticks sag about 0.034, 4 ticks 0.06.
        assert_eq!(teleport_ticks(&oval(), 8.0), 3);
        // At 20 blocks/s (the demo's experiment speed): 2 ticks would sag 0.09, so every tick.
        assert_eq!(teleport_ticks(&oval(), 20.0), 1);
        // A very slow train can use the longest gap, a very fast one must update every tick.
        assert_eq!(teleport_ticks(&oval(), 1.0), 5);
        assert_eq!(teleport_ticks(&oval(), 200.0), 1);
        let _ = oval::SPEED;
    }

    #[test]
    fn the_riders_point_hangs_below_the_seat() {
        let p = rider_point(DVec3::new(1.0, 2.0, 3.0));
        assert!(p.distance(DVec3::new(1.0, 0.7875, 3.0)) < 1e-9);
    }
}
