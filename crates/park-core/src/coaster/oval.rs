//! The M1 oval (D47), kept as a flat test track now that `/coaster demo` builds the hills (D54): two straights joined by two half circles.
//!
//! Hands-on idea: change `RADIUS` below, run `cargo test -p park-core`, and watch the test
//! that checks the oval's length fail, then fix the number it compares with.

use glam::DVec3;

/// Length of each straight, in blocks.
pub const STRAIGHT: f64 = 11.0;
/// Radius of each half circle, in blocks.
pub const RADIUS: f64 = 6.0;
/// The oval's exact length in blocks: both straights plus a full circle.
pub const EXACT_LENGTH: f64 = 2.0 * STRAIGHT + std::f64::consts::TAU * RADIUS;
/// Constant speed of the demo train, in blocks per second.
/// EXPERIMENT (2026-10-31): was 8.0 (about 29 km/h, D47). 20.0 is 72 km/h, to feel how the
/// rider's stepping camera (D23) behaves at speed. Set it back to 8.0 to undo.
pub const SPEED: f64 = 20.0;

/// Spacing of nodes along the straights, in blocks.
const STRAIGHT_NODE_GAP: f64 = 2.2;
/// Angle between nodes along the curves, in degrees.
const CURVE_NODE_DEGREES: f64 = 10.0;

/// The oval's nodes in its own flat coordinates `(x, z)`, going round once, starting on a
/// straight.
fn local_nodes() -> Vec<(f64, f64)> {
    let half = STRAIGHT / 2.0;
    let mut nodes = Vec::new();
    // First straight (x = +radius), going towards +z. Its two end nodes belong to the curves.
    let mut z = -half + STRAIGHT_NODE_GAP;
    while z < half - 1e-9 {
        nodes.push((RADIUS, z));
        z += STRAIGHT_NODE_GAP;
    }
    // The half circle at +z, from angle 0 to 180.
    let steps = (180.0 / CURVE_NODE_DEGREES) as usize;
    for i in 0..=steps {
        let a = (i as f64 * CURVE_NODE_DEGREES).to_radians();
        nodes.push((RADIUS * a.cos(), half + RADIUS * a.sin()));
    }
    // Second straight (x = -radius), going towards -z.
    let mut z = half - STRAIGHT_NODE_GAP;
    while z > -half + 1e-9 {
        nodes.push((-RADIUS, z));
        z -= STRAIGHT_NODE_GAP;
    }
    // The half circle at -z, from angle 180 to 360.
    for i in 0..=steps {
        let a = (180.0 + i as f64 * CURVE_NODE_DEGREES).to_radians();
        nodes.push((RADIUS * a.cos(), -half + RADIUS * a.sin()));
    }
    nodes
}

/// The oval's nodes in the world: centred on `center`, flat, with its long side along the
/// direction a player facing `yaw_degrees` looks (Minecraft's yaw: 0 looks towards +Z).
pub fn demo_nodes(center: DVec3, yaw_degrees: f64) -> Vec<DVec3> {
    let yaw = yaw_degrees.to_radians();
    let forward = DVec3::new(-yaw.sin(), 0.0, yaw.cos());
    let side = DVec3::new(yaw.cos(), 0.0, yaw.sin());
    local_nodes()
        .into_iter()
        .map(|(x, z)| center + side * x + forward * z)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::spline::Track;
    use crate::coaster::table::Table;

    fn table(yaw: f64) -> Table {
        Table::new(&Track::closed(&demo_nodes(
            DVec3::new(100.0, 70.0, -40.0),
            yaw,
        )))
    }

    #[test]
    fn the_exact_length_is_59_7_blocks() {
        assert!((EXACT_LENGTH - 59.70).abs() < 0.01);
    }

    #[test]
    fn the_ovals_table_length_is_within_a_tenth_of_a_percent() {
        for yaw in [0.0, 37.0, -90.0, 180.0] {
            let t = table(yaw);
            let off = (t.length() - EXACT_LENGTH).abs() / EXACT_LENGTH;
            assert!(off < 0.001, "yaw {yaw}: length {} off by {off}", t.length());
        }
    }

    #[test]
    fn the_oval_is_flat_and_its_up_arrow_points_up() {
        let t = table(0.0);
        let mut s = 0.0;
        while s < t.length() {
            let f = t.frame(s);
            assert!((f.pos.y - 70.0).abs() < 1e-9);
            assert!(f.up.dot(DVec3::Y) > 0.9999);
            s += 0.37;
        }
    }

    #[test]
    fn the_tightest_bend_is_a_little_tighter_than_the_six_block_curve() {
        // Where a straight meets a curve the spline tightens slightly (about 5.3 blocks).
        let t = table(0.0);
        let radius = 1.0 / t.max_curvature();
        assert!(radius > 4.8 && radius < RADIUS + 0.3, "radius {radius}");
    }

    #[test]
    fn yaw_zero_puts_the_long_side_along_plus_z() {
        let t = table(0.0);
        // The start is on a straight that runs towards +z.
        assert!(t.frame(0.0).forward.dot(DVec3::Z) > 0.99);
    }
}
