//! The track curve: a closed "centripetal Catmull-Rom" spline [D42].
//!
//! Think of nodes as pins on a board and the track as a string pulled through every pin.
//! "Centripetal" is the flavour that never overshoots a pin or makes a little loop.
//! Each stretch between two pins is turned into a cubic Bézier curve (four points:
//! the two pins and two "handles"), which is easy to evaluate.

use glam::DVec3;

/// How uneven spacing is handled. 0.5 is the "centripetal" choice.
const ALPHA: f64 = 0.5;

/// One stretch of track between two nodes, as a cubic Bézier curve.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub p0: DVec3,
    pub p1: DVec3,
    pub p2: DVec3,
    pub p3: DVec3,
}

impl Segment {
    /// The point at `t` (0 = first node, 1 = second node).
    pub fn point(&self, t: f64) -> DVec3 {
        let u = 1.0 - t;
        self.p0 * (u * u * u)
            + self.p1 * (3.0 * u * u * t)
            + self.p2 * (3.0 * u * t * t)
            + self.p3 * (t * t * t)
    }
}

/// A closed loop of segments through every node.
#[derive(Clone, Debug)]
pub struct Track {
    segments: Vec<Segment>,
}

/// Distance between two points to the power `ALPHA`, never zero (so we never divide by zero).
fn knot_gap(a: DVec3, b: DVec3) -> f64 {
    a.distance(b).max(1e-9).powf(ALPHA)
}

impl Track {
    /// Build a closed track through `nodes` (at least 3). The last node connects back to the first.
    pub fn closed(nodes: &[DVec3]) -> Track {
        let n = nodes.len();
        let mut segments = Vec::with_capacity(n);
        if n < 3 {
            return Track { segments };
        }
        for i in 0..n {
            // The four nodes around this stretch; the loop wraps around the ends.
            let a = nodes[(i + n - 1) % n];
            let b = nodes[i];
            let c = nodes[(i + 1) % n];
            let d = nodes[(i + 2) % n];
            let d1 = knot_gap(a, b);
            let d2 = knot_gap(b, c);
            let d3 = knot_gap(c, d);
            // Catmull-Rom to Bézier handles (the standard centripetal formulas).
            let h1 = (b * (2.0 * d1 * d1 + 3.0 * d1 * d2 + d2 * d2) - a * (d2 * d2) + c * (d1 * d1))
                / (3.0 * d1 * (d1 + d2));
            let h2 = (c * (2.0 * d3 * d3 + 3.0 * d3 * d2 + d2 * d2) - d * (d2 * d2) + b * (d3 * d3))
                / (3.0 * d3 * (d3 + d2));
            segments.push(Segment {
                p0: b,
                p1: h1,
                p2: h2,
                p3: c,
            });
        }
        Track { segments }
    }

    /// The segments, in track order.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<DVec3> {
        vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 10.0),
            DVec3::new(0.0, 0.0, 10.0),
        ]
    }

    #[test]
    fn passes_exactly_through_every_node() {
        let nodes = square();
        let track = Track::closed(&nodes);
        assert_eq!(track.segments().len(), 4);
        for (i, seg) in track.segments().iter().enumerate() {
            assert!(seg.point(0.0).distance(nodes[i]) < 1e-9);
            assert!(seg.point(1.0).distance(nodes[(i + 1) % 4]) < 1e-9);
        }
    }

    #[test]
    fn too_few_nodes_give_an_empty_track() {
        assert!(Track::closed(&[DVec3::ZERO, DVec3::X]).segments().is_empty());
    }
}
