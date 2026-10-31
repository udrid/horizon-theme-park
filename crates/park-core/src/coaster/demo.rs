//! The demo coaster for `/coaster demo` (D54, D58): a station, a chain lift, a first drop, two
//! airtime hills, a turnaround, and brakes, shaped so it rides like a real coaster.
//!
//! How the layout is described (so you can change it): the track's shape seen from ABOVE is a
//! "stadium" (two straights joined by two half circles), fixed by `radius` and `straight`. The
//! HEIGHT along that path is described by a list of grades: "from here on, climb or fall at this
//! angle", and every change of angle is smoothly rounded over a stretch (so there are no sharp
//! corners, D58). The last grade is worked out by the code so the track closes up exactly.
//!
//! Hands-on idea: change a grade in `Shape::demo()` (say the lift's 20 to 25), run
//! `cargo run -p park-sim -- demo`, and watch the top speed and the g-forces change.

use crate::coaster::elements::{Course, Element, Placed};
use crate::coaster::spline::Track;
use crate::coaster::table::Table;
use glam::DVec3;

/// Cars in the demo train (D55).
pub const CARS: usize = 3;
/// Length of the station, in blocks.
pub const STATION_LENGTH: f64 = 12.5;
/// Where the front car stops in the station, blocks from its start.
const STATION_STOP: f64 = 10.0;
/// Distance between nodes along the path seen from above, in blocks.
const NODE_GAP: f64 = 0.6;

/// One change of grade: from about `at` blocks along (seen from above) the track climbs
/// (`degrees` > 0) or falls (< 0) at that angle. The change is rounded over `blend` blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub at: f64,
    pub degrees: f64,
    pub blend: f64,
}

const fn grade(at: f64, degrees: f64, blend: f64) -> Grade {
    Grade { at, degrees, blend }
}

/// Everything that decides the demo's shape. `Shape::demo()` is the layout players get;
/// `park-sim` can try other shapes without rebuilding the game.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// Radius of both half circles seen from above, in blocks.
    pub radius: f64,
    /// Length of both straights seen from above, in blocks.
    pub straight: f64,
    /// The stadium path is walked starting this far along it, so the lap's start (the station)
    /// is at the end of the second straight.
    pub phase: f64,
    /// The height profile. The very last entry's angle is replaced by the code so the lap closes.
    pub grades: Vec<Grade>,
    /// The chain lift: from and to, blocks along the path seen from above.
    pub lift: (f64, f64),
    /// Length of the brake run before the station, in blocks.
    pub brake_len: f64,
}

impl Shape {
    /// The demo layout, worked out on paper with a small model and then tuned with `park-sim`
    /// until it met D58's limits (see docs/milestones/M2.md).
    pub fn demo() -> Shape {
        Shape {
            radius: 15.0,
            straight: 105.401,
            phase: 245.426,
            grades: vec![
                grade(0.00, 0.0, 1.0),
                grade(18.50, 20.0, 10.0), // the lift starts climbing at 20 degrees
                grade(80.93, -52.0, 18.0), // top of the lift (about 21 blocks up), then the first drop
                grade(107.93, 33.82, 44.0), // the valley at the bottom, up into hill 1
                grade(139.43, -5.4, 10.0), // crest of hill 1 (airtime), into the turnaround
                grade(148.96, 0.0, 11.05), // level through the turnaround
                grade(210.52, -56.0, 12.0), // second drop
                grade(227.52, 24.4, 34.0), // valley, up into hill 2
                grade(257.49, -18.0, 14.0), // crest of hill 2 (airtime)
                grade(280.42, 0.0, 18.0),  // level run into the brakes
                grade(293.42, 0.0, 8.0),   // closing grade (worked out by the code)
            ],
            lift: (14.0, 74.932),
            brake_len: 16.0,
        }
    }

    /// Length of the whole path seen from above.
    pub fn path_length(&self) -> f64 {
        2.0 * self.straight + 2.0 * std::f64::consts::PI * self.radius
    }

    /// The path seen from above at `u` blocks along: (x, z). The first straight heads +x; the
    /// stadium bends to the right (towards +z).
    fn plan(&self, u: f64) -> (f64, f64) {
        let (r, s) = (self.radius, self.straight);
        let arc = std::f64::consts::PI * r;
        let u = (u + self.phase).rem_euclid(self.path_length());
        if u < s {
            (u, 0.0)
        } else if u < s + arc {
            let a = (u - s) / r;
            (s + r * a.sin(), r - r * a.cos())
        } else if u < 2.0 * s + arc {
            (s - (u - s - arc), 2.0 * r)
        } else {
            let a = (u - 2.0 * s - arc) / r;
            (-r * a.sin(), r + r * a.cos())
        }
    }

    /// The slope (rise per block) at `u`. The ANGLE is eased between grades, so the track's bend
    /// (and with it the g-force) rises and falls smoothly. `last_degrees` replaces the final entry.
    fn slope(&self, last_degrees: f64, u: f64) -> f64 {
        let mut angle = 0.0;
        let mut previous = 0.0;
        for (i, g) in self.grades.iter().enumerate() {
            let degrees = if i + 1 == self.grades.len() {
                last_degrees
            } else {
                g.degrees
            };
            angle += (degrees - previous) * ease((u - g.at) / g.blend + 0.5);
            previous = degrees;
        }
        angle.to_radians().tan()
    }

    /// Heights at every `0.05` blocks along the path, by adding up the slope.
    fn heights(&self, last_degrees: f64) -> Vec<f64> {
        const DU: f64 = 0.05;
        let n = (self.path_length() / DU).ceil() as usize + 1;
        let mut y = Vec::with_capacity(n);
        let mut h = 0.0;
        y.push(h);
        for i in 1..n {
            let u = (i as f64 - 0.5) * DU;
            h += self.slope(last_degrees, u) * DU;
            y.push(h);
        }
        y
    }

    /// The nodes the track passes through, with the path distance `u` of each, relative to the
    /// station's start (the first node, at height 0).
    pub fn nodes(&self) -> (Vec<DVec3>, Vec<f64>) {
        // Close the loop: the last angle is chosen so the track ends as high as it started.
        // (Bisection: try a middle angle, keep the half that still contains the answer.)
        let (mut lo, mut hi) = (-30.0, 30.0);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            // A steeper last angle ends higher.
            if self.heights(mid).last().copied().unwrap_or(0.0) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let y = self.heights(0.5 * (lo + hi));
        let length = self.path_length();
        let count = (length / NODE_GAP).round() as usize;
        let (x0, z0) = self.plan(0.0);
        let mut nodes = Vec::with_capacity(count);
        let mut us = Vec::with_capacity(count);
        for i in 0..count {
            let u = i as f64 * length / count as f64;
            let (x, z) = self.plan(u);
            let f = u / 0.05;
            let (a, b) = (f.floor() as usize, f.floor() as usize + 1);
            let h = y[a] + (y[b.min(y.len() - 1)] - y[a]) * (f - a as f64);
            nodes.push(DVec3::new(x - x0, h, z - z0));
            us.push(u);
        }
        (nodes, us)
    }
}

/// Smooth 0-to-1 step (cosine ease) over x in 0..1.
fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    0.5 - 0.5 * (std::f64::consts::PI * x).cos()
}

/// The demo course with its station's start at `origin`, ready to ride.
pub fn demo_course(origin: DVec3) -> Course {
    course_for(&Shape::demo(), origin)
}

/// The course for any shape.
pub fn course_for(shape: &Shape, origin: DVec3) -> Course {
    let (nodes, us) = shape.nodes();
    let nodes: Vec<DVec3> = nodes.into_iter().map(|p| p + origin).collect();
    let table = Table::new_level(&Track::closed(&nodes));
    let length = shape.path_length();
    // Turn "u" (blocks seen from above) into "s" (blocks along the real track): between nodes we
    // measure along the table, so slopes are accounted for.
    let to_s = |u: f64| -> f64 { u_to_s(&table, &nodes, &us, length, u) };
    let placed = vec![
        Placed {
            s0: to_s(length - shape.brake_len),
            s1: table.length(),
            element: Element::brake(),
        },
        Placed {
            s0: to_s(0.0),
            s1: to_s(STATION_LENGTH),
            element: Element::station(to_s(STATION_STOP)),
        },
        Placed {
            s0: to_s(shape.lift.0),
            s1: to_s(shape.lift.1),
            element: Element::lift(),
        },
    ];
    Course { table, placed }
}

/// The distance along the table of the point at `u` blocks along the path seen from above.
/// Each node's own `s` is found by walking the table to the node's position.
fn u_to_s(table: &Table, nodes: &[DVec3], us: &[f64], length: f64, u: f64) -> f64 {
    let n = nodes.len();
    let u = u.clamp(0.0, length);
    let i = us.partition_point(|&x| x <= u).saturating_sub(1).min(n - 1);
    let j = (i + 1) % n;
    let polyline = |upto: usize| -> f64 {
        (0..upto)
            .map(|w| nodes[w].distance(nodes[(w + 1) % n]))
            .sum()
    };
    let s_of = |k: usize| -> f64 {
        // Rough guess from the straight-line distances, then refine against the table.
        let guess = polyline(k) * table.length() / polyline(n);
        let mut best = (f64::MAX, guess);
        let mut s = guess - 3.0;
        while s <= guess + 3.0 {
            let d = table.position(s).distance(nodes[k]);
            if d < best.0 {
                best = (d, s);
            }
            s += 0.02;
        }
        best.1
    };
    let (s0, s1) = (s_of(i), if j == 0 { table.length() } else { s_of(j) });
    let u1 = if j == 0 { length } else { us[j] };
    s0 + (s1 - s0) * (u - us[i]) / (u1 - us[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::physics::{Physics, Train};
    use crate::coaster::sim;

    fn course() -> Course {
        demo_course(DVec3::new(100.0, 73.0, -40.0))
    }

    fn lap(course: &Course) -> Vec<sim::Sample> {
        let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
        sim::run_lap(course, Train::held(CARS, stop), &Physics::default(), 300.0)
    }

    #[test]
    fn the_lap_closes_and_is_about_three_hundred_blocks_long() {
        let c = course();
        assert!(
            c.table.length() > 280.0 && c.table.length() < 340.0,
            "{}",
            c.table.length()
        );
        // The first and last nodes meet without a jump: the track is one smooth loop.
        let end = c.table.position(c.table.length() - 0.05);
        assert!(end.distance(c.table.position(0.0)) < 0.2);
    }

    // Test 11: one simulated lap.
    #[test]
    fn the_train_never_stalls_and_finishes_the_lap_in_the_station() {
        let c = course();
        let samples = lap(&c);
        let last = samples.last().unwrap();
        assert!(last.t < 120.0, "the lap took {} s", last.t);
        // It ended held at the stop point (run_lap stops when the train is held again).
        let stop = c.station().unwrap().1;
        assert!(
            (last.s - stop).abs() < 0.3,
            "ended at {} not {stop}",
            last.s
        );
        let st = sim::stats(&samples);
        assert!(st.slowest_speed > 0.9, "slowest {}", st.slowest_speed);
    }

    // Test 12: D58's realism limits.
    #[test]
    fn the_layout_stays_within_the_realism_limits() {
        let c = course();
        let st = sim::stats(&lap(&c));

        // Valleys push at most 4 g; airtime goes out of the seat by at most half a g.
        assert!(st.max_vertical <= 4.0, "valley {} g", st.max_vertical);
        assert!(st.min_vertical >= -0.5, "airtime {} g", st.min_vertical);
        // Flat turns push sideways at most 1 g (no banking until M5).
        assert!(st.max_lateral <= 1.0, "sideways {} g", st.max_lateral);
        // Vertical g changes by at most 3 g over any 1-second window (D58, ruled by the planner;
        // over a quarter second the ride reaches about 4.2 g/s, which park-sim also prints).
        assert!(
            st.max_vertical_rate <= 3.0,
            "g changes {} g/s",
            st.max_vertical_rate
        );
        assert!(st.top_speed <= 30.0, "top speed {}", st.top_speed);

        // Drops: every stretch steeper than 30 degrees falls at between 50 and 65 degrees at its
        // steepest, and there are at least two of them.
        let mut drops: Vec<f64> = Vec::new();
        let mut steepest = 0.0_f64;
        let mut s = 0.0;
        while s < c.table.length() {
            let fall = (-c.table.frame(s).forward.y).asin().to_degrees();
            if fall > 30.0 {
                steepest = steepest.max(fall);
            } else if steepest > 0.0 {
                drops.push(steepest);
                steepest = 0.0;
            }
            s += 0.1;
        }
        assert!(drops.len() >= 2, "drops {drops:?}");
        for d in &drops {
            assert!(
                (50.0..=65.0).contains(d),
                "a drop of {d} degrees in {drops:?}"
            );
        }

        // The lift is 20 to 30 blocks tall.
        let lift = c
            .placed
            .iter()
            .find(|p| matches!(p.element, Element::Lift { .. }))
            .unwrap();
        let top = (0..)
            .map(|k| lift.s0 + k as f64 * 0.5)
            .take_while(|&s| s < c.table.length())
            .map(|s| c.table.position(s).y)
            .fold(f64::MIN, f64::max);
        let height = top - c.table.position(lift.s0).y;
        assert!((20.0..=30.0).contains(&height), "lift height {height}");
    }

    #[test]
    fn two_airtime_hills_lift_the_rider_a_little_out_of_the_seat() {
        let c = course();
        let samples = lap(&c);
        // Count separate stretches where the front rider's g dips below zero (but not below -0.5).
        let mut hills = 0;
        let mut inside = false;
        for s in &samples {
            let g = s.felt[0].vertical;
            if g < 0.0 && !inside {
                hills += 1;
                inside = true;
            } else if g > 0.3 {
                inside = false;
            }
        }
        assert!(hills >= 2, "only {hills} airtime hills");
    }

    #[test]
    fn the_track_never_dips_more_than_half_a_block_below_the_players_feet() {
        // The origin is 3 above the feet (D47), so the lowest rail must stay above feet - 0.5.
        let c = demo_course(DVec3::new(0.0, 3.0, 0.0));
        let mut s = 0.0;
        while s < c.table.length() {
            assert!(c.table.position(s).y > -0.5, "low at s = {s}");
            s += 0.25;
        }
    }

    #[test]
    fn the_elements_are_where_they_should_be() {
        let c = course();
        let len = c.table.length();
        let lift = c
            .placed
            .iter()
            .find(|p| matches!(p.element, Element::Lift { .. }))
            .unwrap();
        let brake = c
            .placed
            .iter()
            .find(|p| matches!(p.element, Element::Brake { .. }))
            .unwrap();
        let (station, stop, _) = c.station().unwrap();
        // Order along the lap: station, then the lift; the brake ends where the lap ends.
        assert!(station.s0 < stop && stop < station.s1 && station.s1 <= lift.s0);
        assert!((brake.s1 - len).abs() < 1e-9 && brake.s1 - brake.s0 > 14.0);
        // The whole train fits inside the station when stopped.
        assert!(stop - 2.0 * crate::coaster::physics::CAR_SPACING > station.s0);
    }
}
