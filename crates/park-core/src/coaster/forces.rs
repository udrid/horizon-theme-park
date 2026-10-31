//! G-forces (D15): what a rider feels, worked out from the shape of the track.
//!
//! Plain words: your body is pushed by the seat. On flat ground standing still, the seat pushes
//! up with exactly your weight: that is "1 g". Going over a hilltop fast, the track curves away
//! under you, the seat pushes less, and you feel light (less than 1 g, or even a negative g:
//! "airtime"). Going through a valley the seat pushes harder and you feel heavy.
//!
//! The maths (D15): the rider's acceleration is `a · forward + v² · K` (speeding up along the
//! track, plus the sideways change from turning), where `K` is how fast the direction of the track
//! changes per block. The seat's push is that acceleration minus gravity.

use crate::coaster::frame::Frame;
use crate::coaster::table::Table;
use glam::DVec3;

/// Real gravity, blocks per second squared (D11).
pub const G: f64 = 9.81;
/// Half of the distance over which the direction change is measured: ±0.25 block (D15).
pub const H: f64 = 0.25;

/// What a rider feels, in real g, along the three directions of their car (D15).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Felt {
    /// Up from the seat: +1 at rest on flat track, 0 floating, negative = lifted out of the seat.
    pub vertical: f64,
    /// Towards the rider's right: positive when the track turns right.
    pub lateral: f64,
    /// Forwards: positive when the seat pushes you forward (speeding up).
    pub longitudinal: f64,
}

/// How fast the direction of the track turns at `s`: a vector pointing to the inside of the bend,
/// as large as `1 / radius` (D15: direction change over ±0.25 block).
pub fn curvature(table: &Table, s: f64) -> DVec3 {
    let ahead = table.frame(s + H).forward;
    let behind = table.frame(s - H).forward;
    (ahead - behind) / (2.0 * H)
}

/// The seat's push on a rider at `s`, in blocks per second squared, plus the frame it is
/// measured in. `v` is the train's speed and `a` its speed-up along the track.
/// `gravity` is the (scaled) gravity in blocks per second squared.
pub fn push(table: &Table, s: f64, v: f64, a: f64, gravity: f64) -> (Frame, DVec3) {
    let frame = table.frame(s);
    let rider = frame.forward * a + curvature(table, s) * (v * v);
    // The seat must cancel gravity (which pulls down) as well: push = acceleration - (0, -G, 0).
    (frame, rider + DVec3::new(0.0, gravity, 0.0))
}

/// The "normal push" (D14): the part of the seat's push that is square to the track, in blocks
/// per second squared. Friction is proportional to it.
pub fn normal_push(frame: &Frame, push: DVec3) -> f64 {
    let along = frame.forward * push.dot(frame.forward);
    (push - along).length()
}

/// What a rider at `s` feels, in real g (D15), for a train going `v` and speeding up by `a`.
pub fn felt(table: &Table, s: f64, v: f64, a: f64, gravity_scale: f64) -> Felt {
    let (frame, f) = push(table, s, v, a, G * gravity_scale);
    Felt {
        vertical: f.dot(frame.up) / G,
        lateral: f.dot(frame.right) / G,
        longitudinal: f.dot(frame.forward) / G,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::spline::Track;

    fn circle(radius: f64) -> Table {
        let nodes: Vec<DVec3> = (0..72)
            .map(|i| {
                let a = i as f64 / 72.0 * std::f64::consts::TAU;
                DVec3::new(radius * a.cos(), 0.0, radius * a.sin())
            })
            .collect();
        Table::new(&Track::closed(&nodes))
    }

    // Test 4 (ARCHITECTURE §4): a flat unbanked circle, radius 10, at 10 blocks/s.
    #[test]
    fn a_flat_circle_of_radius_10_at_10_blocks_a_second_gives_1_02_lateral_and_1_vertical() {
        let t = circle(10.0);
        for k in 0..8 {
            let s = k as f64 * 7.3;
            let g = felt(&t, s, 10.0, 0.0, 1.0);
            // The circle is traced clockwise seen from above or not: the push is sideways either way.
            assert!((g.lateral.abs() - 1.0194).abs() < 0.0102, "{g:?}");
            assert!((g.vertical - 1.0).abs() < 0.01, "{g:?}");
            assert!(g.longitudinal.abs() < 1e-3, "{g:?}");
        }
    }

    #[test]
    fn at_rest_on_flat_track_the_rider_feels_one_g_up_scaled_by_gravity_scale() {
        let t = circle(10.0);
        assert!((felt(&t, 3.0, 0.0, 0.0, 1.0).vertical - 1.0).abs() < 1e-9);
        assert!((felt(&t, 3.0, 0.0, 0.0, 0.5).vertical - 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_normal_push_of_a_flat_straight_is_gravity() {
        let t = circle(1000.0);
        let (frame, f) = push(&t, 5.0, 0.0, 0.0, G);
        assert!((normal_push(&frame, f) - G).abs() < 1e-6);
    }
}
