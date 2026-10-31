//! The arc-length table: "how far along the track" (`s`, in blocks) to "where is that" (a point
//! and a frame).
//!
//! A curve's own parameter `t` does not move at an even pace: equal steps of `t` can be
//! different distances in the world. So we walk the whole curve once, measure the real distance,
//! and store a point every 0.1 block. Looking up `s` is then one division and a blend between
//! two stored points. Like a ruler laid along the track.

use crate::coaster::frame::{Frame, compute_ups};
use crate::coaster::spline::Track;
use glam::DVec3;

/// Wanted distance between stored points, in blocks.
pub const STEP: f64 = 0.1;

/// How many small straight pieces we cut each curve into per block, to measure it.
const SAMPLES_PER_BLOCK: f64 = 64.0;

/// Stored points, directions and "up" arrows along a closed track.
#[derive(Clone, Debug)]
pub struct Table {
    length: f64,
    step: f64,
    points: Vec<DVec3>,
    tangents: Vec<DVec3>,
    ups: Vec<DVec3>,
}

impl Table {
    /// Measure `track` and store a point every `STEP` blocks. The loop is closed: the last
    /// stored point is followed by the first. Returns an empty table for an empty track.
    pub fn new(track: &Track) -> Table {
        Table::build(track, false)
    }

    /// Like `new`, but the track is NOT banked: "up" is always as close to the sky as the
    /// track's direction allows, so a climbing turn does not roll the car sideways. This is
    /// what M2's track needs (no banking until M5, D24). Only for tracks that never go
    /// straight up or upside down.
    pub fn new_level(track: &Track) -> Table {
        Table::build(track, true)
    }

    fn build(track: &Track, level: bool) -> Table {
        // 1. Cut every segment into many tiny straight pieces and add up their lengths.
        //    Each entry is (distance so far, point).
        let mut dense: Vec<(f64, DVec3)> = Vec::new();
        let mut total = 0.0;
        for seg in track.segments() {
            let chord = seg.p0.distance(seg.p3);
            let n = ((chord * SAMPLES_PER_BLOCK).ceil() as usize).max(16);
            for i in 0..=n {
                let p = seg.point(i as f64 / n as f64);
                if let Some(&(_, prev)) = dense.last() {
                    if i == 0 {
                        // The first point of a segment is the last point of the one before it.
                        continue;
                    }
                    total += p.distance(prev);
                }
                dense.push((total, p));
            }
        }
        if dense.len() < 2 || total <= 0.0 {
            return Table {
                length: 0.0,
                step: STEP,
                points: Vec::new(),
                tangents: Vec::new(),
                ups: Vec::new(),
            };
        }
        // 2. Resample at an even distance. We pick the step so a whole number of steps fits the
        //    loop exactly, which makes the wrap-around seamless.
        let count = ((total / STEP).round() as usize).max(3);
        let step = total / count as f64;
        let mut points = Vec::with_capacity(count);
        let mut j = 0;
        for k in 0..count {
            let s = k as f64 * step;
            while j + 2 < dense.len() && dense[j + 1].0 < s {
                j += 1;
            }
            let (s0, p0) = dense[j];
            let (s1, p1) = dense[j + 1];
            let f = if s1 > s0 {
                ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            points.push(p0.lerp(p1, f));
        }
        // 3. Directions (neighbour to neighbour) and "up" arrows.
        let tangents: Vec<DVec3> = (0..count)
            .map(|k| {
                let a = points[(k + count - 1) % count];
                let b = points[(k + 1) % count];
                (b - a).normalize_or_zero()
            })
            .collect();
        let ups = if level {
            tangents
                .iter()
                .map(|t| {
                    let up = (DVec3::Y - *t * t.y).normalize_or_zero();
                    if up == DVec3::ZERO { DVec3::X } else { up }
                })
                .collect()
        } else {
            compute_ups(&tangents)
        };
        Table {
            length: total,
            step,
            points,
            tangents,
            ups,
        }
    }

    /// Length of one lap, in blocks (0 for an empty table).
    pub fn length(&self) -> f64 {
        self.length
    }

    /// True when the track had nothing to measure.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Bring any `s` back into one lap: 0 up to (not including) the length. The track is a loop.
    pub fn wrap(&self, s: f64) -> f64 {
        if self.length <= 0.0 {
            0.0
        } else {
            s.rem_euclid(self.length)
        }
    }

    /// Which stored point `s` is just after, the next one, and how far (0..1) between them.
    fn locate(&self, s: f64) -> (usize, usize, f64) {
        let n = self.points.len();
        let x = self.wrap(s) / self.step;
        let i = (x.floor() as usize).min(n - 1);
        (i, (i + 1) % n, x - i as f64)
    }

    /// The point on the track at distance `s` (wraps around the loop).
    pub fn position(&self, s: f64) -> DVec3 {
        if self.is_empty() {
            return DVec3::ZERO;
        }
        let (i, j, f) = self.locate(s);
        self.points[i].lerp(self.points[j], f)
    }

    /// The full frame (position, forward, up, right) at distance `s`.
    pub fn frame(&self, s: f64) -> Frame {
        if self.is_empty() {
            return Frame::default();
        }
        let (i, j, f) = self.locate(s);
        let pos = self.points[i].lerp(self.points[j], f);
        let forward = self.tangents[i]
            .lerp(self.tangents[j], f)
            .normalize_or_zero();
        // Blend the up arrows, then make sure "up" is exactly square to "forward".
        let raw_up = self.ups[i].lerp(self.ups[j], f);
        let up = (raw_up - forward * raw_up.dot(forward)).normalize_or_zero();
        Frame {
            pos,
            forward,
            up,
            right: forward.cross(up),
        }
    }

    /// The tightest bend on the track: the largest change of direction per block, measured over
    /// about ±0.25 block (D15). 1 divided by this is the smallest curve radius.
    pub fn max_curvature(&self) -> f64 {
        let n = self.points.len();
        if n < 8 {
            return 0.0;
        }
        let half = ((0.25 / self.step).round() as usize).max(1);
        let mut best: f64 = 0.0;
        for k in 0..n {
            let a = self.tangents[(k + n - half % n) % n];
            let b = self.tangents[(k + half) % n];
            let curvature = a.distance(b) / (2.0 * half as f64 * self.step);
            best = best.max(curvature);
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loop that goes up and down a bit, so the maths is tested in 3D.
    fn wobbly_loop() -> Track {
        let nodes: Vec<DVec3> = (0..16)
            .map(|i| {
                let a = i as f64 / 16.0 * std::f64::consts::TAU;
                DVec3::new(20.0 * a.cos(), 3.0 * (2.0 * a).sin(), 12.0 * a.sin())
            })
            .collect();
        Track::closed(&nodes)
    }

    #[test]
    fn same_track_twice_gives_identical_tables() {
        let a = Table::new(&wobbly_loop());
        let b = Table::new(&wobbly_loop());
        assert_eq!(a.length(), b.length());
        for k in 0..50 {
            let s = k as f64 * 1.37;
            assert_eq!(a.frame(s).pos, b.frame(s).pos);
            assert_eq!(a.frame(s).up, b.frame(s).up);
        }
    }

    #[test]
    fn a_lookup_at_s_and_at_s_plus_length_is_the_same_point() {
        let t = Table::new(&wobbly_loop());
        for k in 0..40 {
            let s = k as f64 * 2.3;
            assert!(t.position(s).distance(t.position(s + t.length())) < 1e-9);
            assert!(t.position(s).distance(t.position(s - t.length())) < 1e-9);
        }
    }

    #[test]
    fn stored_points_are_about_a_tenth_of_a_block_apart() {
        let t = Table::new(&wobbly_loop());
        let d = t.position(5.0).distance(t.position(5.1));
        assert!((d - 0.1).abs() < 0.01, "got {d}");
    }

    #[test]
    fn a_level_table_keeps_up_in_the_vertical_plane_through_a_climbing_turn() {
        // A spiral that climbs while it turns: a carried-along "up" would roll sideways here.
        let nodes: Vec<DVec3> = (0..48)
            .map(|i| {
                let a = i as f64 / 48.0 * std::f64::consts::TAU;
                DVec3::new(15.0 * a.cos(), 3.0 * (a * 2.0).sin(), 15.0 * a.sin())
            })
            .collect();
        let t = Table::new_level(&Track::closed(&nodes));
        let mut s = 0.0;
        while s < t.length() {
            let f = t.frame(s);
            // "right" is flat (no sideways roll) and up leans only forwards or backwards.
            assert!(f.right.y.abs() < 1e-3, "rolled at s = {s}");
            assert!(f.up.y > 0.9);
            s += 0.7;
        }
    }

    #[test]
    fn an_empty_track_is_harmless() {
        let t = Table::new(&Track::closed(&[]));
        assert!(t.is_empty());
        assert_eq!(t.position(3.0), DVec3::ZERO);
        assert_eq!(t.length(), 0.0);
    }
}
