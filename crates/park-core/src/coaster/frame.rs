//! Frames: at every point of the track, three arrows that say which way is forward, up and
//! right. A car's rotation comes from them.
//!
//! Two kinds of frame exist. These rotation-minimising frames suit loops and inversions, but on a
//! track that climbs while turning they slowly roll the car sideways. Unbanked track (M2) uses
//! `Table::new_level` instead, where `right` always stays horizontal. M5 must choose between the two,
//! or blend them, when banking arrives.
//!
//! The trick is "rotation-minimising" frames: walk along the track and turn the up arrow only as
//! much as the track itself turns, never twisting it for no reason (like carrying a flag
//! along a path without spinning it). On a closed loop the arrow may come back slightly
//! twisted, so we measure that leftover twist and spread it evenly over the whole lap.

use glam::{DQuat, DVec3};

/// Position and orientation at one point of the track. `right` is the rider's right hand:
/// `forward × up`. Facing +Z (south) with +Y up, that is -X (west); facing +X it is +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub pos: DVec3,
    pub forward: DVec3,
    pub up: DVec3,
    pub right: DVec3,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            pos: DVec3::ZERO,
            forward: DVec3::Z,
            up: DVec3::Y,
            right: DVec3::NEG_X,
        }
    }
}

/// Take `v` and remove the part that points along `t`, so it is square to `t`.
fn square_to(v: DVec3, t: DVec3) -> DVec3 {
    (v - t * v.dot(t)).normalize_or_zero()
}

/// The up arrow for every direction in `tangents` (a closed loop of unit directions).
pub fn compute_ups(tangents: &[DVec3]) -> Vec<DVec3> {
    let n = tangents.len();
    if n == 0 {
        return Vec::new();
    }
    // Start: world up, with any part along the track removed. (A vertical start would need
    // another choice; tracks start flat in M1.)
    let mut first = square_to(DVec3::Y, tangents[0]);
    if first == DVec3::ZERO {
        first = square_to(DVec3::X, tangents[0]);
    }
    let mut ups = Vec::with_capacity(n);
    ups.push(first);
    for k in 1..n {
        ups.push(carry(ups[k - 1], tangents[k - 1], tangents[k]));
    }
    // Close the loop: carry the last arrow back to the start and see how far it is twisted.
    let back = carry(ups[n - 1], tangents[n - 1], tangents[0]);
    let twist = tangents[0].dot(first.cross(back)).atan2(first.dot(back));
    // Spread the twist: the arrow k steps along is turned back by its share of it.
    for (k, up) in ups.iter_mut().enumerate() {
        let share = -twist * k as f64 / n as f64;
        *up = DQuat::from_axis_angle(tangents[k], share) * *up;
    }
    ups
}

/// Carry the up arrow from a place where the track points along `from` to one where it points
/// along `to`, turning it by the smallest possible rotation.
pub fn carry(up: DVec3, from: DVec3, to: DVec3) -> DVec3 {
    let turned = DQuat::from_rotation_arc(from, to) * up;
    square_to(turned, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::spline::Track;
    use crate::coaster::table::Table;

    fn angle_deg(a: DVec3, b: DVec3) -> f64 {
        a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// A closed loop with S-bends, hills and a sideways wobble.
    fn s_bend_loop() -> Table {
        let nodes: Vec<DVec3> = (0..24)
            .map(|i| {
                let a = i as f64 / 24.0 * std::f64::consts::TAU;
                DVec3::new(
                    30.0 * a.cos(),
                    6.0 * (3.0 * a).sin(),
                    10.0 * a.sin() + 6.0 * (2.0 * a).sin(),
                )
            })
            .collect();
        Table::new(&Track::closed(&nodes))
    }

    #[test]
    fn right_is_the_riders_right_hand() {
        use crate::coaster::oval::demo_nodes;
        // Facing +Z (yaw 0): right is -X. Facing +X (yaw -90): right is +Z.
        for (yaw, forward, right) in [(0.0, DVec3::Z, -DVec3::X), (-90.0, DVec3::X, DVec3::Z)] {
            let t = Table::new(&Track::closed(&demo_nodes(DVec3::ZERO, yaw)));
            let f = t.frame(0.0);
            assert!(f.forward.dot(forward) > 0.99);
            assert!(f.right.dot(right) > 0.99, "yaw {yaw}: right {:?}", f.right);
            assert!(f.right.dot(f.up).abs() < 1e-9);
        }
    }

    #[test]
    fn no_flip_through_an_s_bend() {
        let t = s_bend_loop();
        let mut prev = t.frame(0.0);
        let mut s = 0.1;
        while s < t.length() {
            let f = t.frame(s);
            // Up never turns upside down and never jumps between neighbouring points. (On a wild 3D
            // loop it may lean gradually: that is the carried-along twist, spread over the lap.)
            // NOTE: `up·Y > 0` is right for M1's tracks only. Loops turn riders upside down, so the
            // M5 plan replaces it with the continuity check (the jump test below) alone.
            assert!(f.up.dot(DVec3::Y) > 0.0, "up flipped at s = {s}");
            assert!(angle_deg(prev.up, f.up) < 1.5, "up jumped at s = {s}");
            assert!(f.up.dot(f.forward).abs() < 1e-9);
            prev = f;
            s += 0.1;
        }
    }

    #[test]
    fn on_a_closed_loop_up_comes_back_to_where_it_started() {
        let t = s_bend_loop();
        // Carry the last arrow back over the closing step: it must meet the first one.
        let last = t.frame(t.length() - 0.05);
        let start = t.frame(0.0);
        let back = carry(last.up, last.forward, start.forward);
        assert!(angle_deg(back, start.up) < 0.1, "leftover twist too big");
    }
}
