//! The realism report (D64): one dry lap of a ride, compared with D58's limits.
//!
//! The lap is run on paper (as `park-sim` does), so nothing here touches the game. Anything past
//! a limit becomes an `Issue` that says what, how far, and where on the track (block numbers),
//! and the ride still opens: it is the builder's own coaster.

use crate::coaster::elements::{BRAKE_DECEL, Course, Element};
use crate::coaster::forces::Felt;
use crate::coaster::physics::{Physics, Train};
use crate::coaster::sim::{self, Sample};
use glam::DVec3;

/// Drops must fall at between these angles, in degrees (D58).
pub const DROP_MIN: f64 = 50.0;
pub const DROP_MAX: f64 = 65.0;
/// A valley may push at most this many g (D58).
pub const VALLEY_MAX_G: f64 = 4.0;
/// An airtime hill may reach at most this far below zero g (D58).
pub const AIRTIME_MIN_G: f64 = -0.5;
/// Sideways push on unbanked track, in g (D58).
pub const LATERAL_MAX_G: f64 = 1.0;
/// Vertical g may change by at most this much per second (D58).
pub const RATE_MAX_G_PER_S: f64 = 3.0;
/// A lift hill is between these heights, in blocks (D58).
pub const LIFT_MIN: f64 = 20.0;
pub const LIFT_MAX: f64 = 30.0;
/// The dry lap gives up after this many seconds.
const MAX_LAP_SECONDS: f64 = 300.0;

/// One limit that was passed.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// A plain sentence: what and how far past, e.g. "Sideways push reaches 2.3 g (limit 1.0 g)".
    pub text: String,
    /// Where it happens, if known.
    pub at: Option<DVec3>,
}

/// What one dry lap showed.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// False when the train never got back to the station.
    pub completes: bool,
    pub seconds: f64,
    pub length: f64,
    /// Blocks per second.
    pub top_speed: f64,
    pub max_vertical: f64,
    pub min_vertical: f64,
    pub max_lateral: f64,
    pub max_vertical_rate: f64,
    /// Lowest vertical g of each airtime dip.
    pub airtime_dips: Vec<f64>,
    /// The steepest fall of each drop, in degrees.
    pub drops: Vec<f64>,
    /// Height of the tallest lift hill, in blocks.
    pub lift_height: Option<f64>,
    pub issues: Vec<Issue>,
}

fn at(p: DVec3) -> String {
    format!("near {} {} {}", p.x.round(), p.y.round(), p.z.round())
}

/// Where the front car is at `sample`, or the car with the extreme value.
fn find_extreme(
    course: &Course,
    samples: &[Sample],
    spacing: f64,
    score: impl Fn(&Felt) -> f64,
) -> DVec3 {
    let mut best = (f64::MIN, 0.0);
    for sample in samples {
        for i in 0..sample.cars {
            let value = score(&sample.felt[i]);
            if value > best.0 {
                best = (value, sample.s - i as f64 * spacing);
            }
        }
    }
    course.table.position(best.1)
}

/// Why a track must not open (D68).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blocker {
    /// The train comes into the station too fast and rolls past the stop point. `s` is the
    /// start of the station.
    Overshoot { s: f64 },
    /// The train runs out of speed outside the station and the lift. `s` is where.
    Stall { s: f64 },
}

/// A stall: the train crawls slower than this...
const STALL_SPEED: f64 = 0.1;
/// ...for this many ticks in a row (2 seconds).
const STALL_TICKS: usize = 40;

/// Run one dry lap and say whether the train cannot stop in the station (it overshoots the stop
/// point) or stalls outside the station and the lift (D68). `None` means the track may open.
pub fn blocker(course: &Course, cars: usize, spacing: f64, physics: &Physics) -> Option<Blocker> {
    let (station, stop, _) = course.station()?;
    let (s0, s1) = (station.s0, station.s1);
    let train = Train {
        spacing,
        ..Train::held(cars, stop)
    };
    let samples = sim::run_lap(course, train, physics, MAX_LAP_SECONDS);
    let in_station = |s: f64| s >= s0 && s < s1;
    let on_lift = |s: f64| {
        course
            .placed
            .iter()
            .any(|p| matches!(p.element, Element::Lift { .. }) && s >= p.s0 && s < p.s1)
    };
    // The fastest the train may enter the station: braking at BRAKE_DECEL it must stop before the
    // stop point. Faster than that and it would be stopped by force (or roll on past the stop).
    let safe_entry = (2.0 * BRAKE_DECEL * (stop - s0)).sqrt() + 0.25;
    let mut lapped = false;
    let mut slow = 0;
    for (k, sample) in samples.iter().enumerate() {
        let s = course.table.wrap(sample.s);
        if !in_station(s) {
            lapped = true;
        } else if lapped {
            if sample.v > safe_entry {
                return Some(Blocker::Overshoot { s: s0 });
            }
            lapped = false; // only the speed of entering counts
        }
        if k > 20 && !in_station(s) && !on_lift(s) && sample.v.abs() < STALL_SPEED {
            slow += 1;
            if slow >= STALL_TICKS {
                return Some(Blocker::Stall { s });
            }
        } else {
            slow = 0;
        }
    }
    let completed = samples.last().is_some_and(|s| s.t < MAX_LAP_SECONDS - 0.1);
    if completed {
        return None;
    }
    // It never got back: find where it first crawled to a stop.
    let stuck = samples
        .iter()
        .enumerate()
        .find(|(k, s)| {
            *k > 20 && s.v < STALL_SPEED && !in_station(s.s) && !on_lift(course.table.wrap(s.s))
        })
        .map_or(0.0, |(_, s)| s.s);
    Some(Blocker::Stall {
        s: course.table.wrap(stuck),
    })
}

/// Run one dry lap of `course` with `cars` cars and compare it with D58's limits.
pub fn analyse(course: &Course, cars: usize, spacing: f64, physics: &Physics) -> Report {
    let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
    let train = Train {
        spacing,
        ..Train::held(cars, stop)
    };
    let samples = sim::run_lap(course, train, physics, MAX_LAP_SECONDS);
    let st = sim::stats(&samples);
    let mut issues = Vec::new();

    let completes = st.seconds < MAX_LAP_SECONDS - 0.1;
    if !completes {
        // Where did it get stuck? The slowest moment after the start.
        let stuck = samples
            .iter()
            .enumerate()
            .filter(|(k, _)| *k > 20)
            .min_by(|a, b| a.1.v.total_cmp(&b.1.v))
            .map(|(_, s)| course.table.position(s.s));
        issues.push(Issue {
            text: "The train cannot finish a lap: it runs out of speed and stalls. Make the lift longer or the hills lower".to_string(),
            at: stuck,
        });
    }

    if st.max_vertical > VALLEY_MAX_G {
        issues.push(Issue {
            text: format!(
                "A valley pushes {:.1} g (limit {VALLEY_MAX_G:.1} g). Round the bottom of the drop more",
                st.max_vertical
            ),
            at: Some(find_extreme(course, &samples, spacing, |f| f.vertical)),
        });
    }
    if st.min_vertical < AIRTIME_MIN_G {
        issues.push(Issue {
            text: format!(
                "A hill throws riders out of their seat: {:.1} g (limit {AIRTIME_MIN_G:.1} g). Make the crest rounder or lower",
                st.min_vertical
            ),
            at: Some(find_extreme(course, &samples, spacing, |f| -f.vertical)),
        });
    }
    if st.max_lateral > LATERAL_MAX_G {
        issues.push(Issue {
            text: format!(
                "A turn pushes sideways {:.1} g (limit {LATERAL_MAX_G:.1} g). Make that turn wider",
                st.max_lateral
            ),
            at: Some(find_extreme(course, &samples, spacing, |f| f.lateral.abs())),
        });
    }
    if st.max_vertical_rate > RATE_MAX_G_PER_S {
        issues.push(Issue {
            text: format!(
                "The vertical g changes too fast: {:.1} g per second (limit {RATE_MAX_G_PER_S:.1}). Use gentler transitions",
                st.max_vertical_rate
            ),
            at: rate_spot(course, &samples, spacing),
        });
    }

    // Drops: every stretch steeper than 30 degrees, measured at its steepest.
    let mut drops: Vec<f64> = Vec::new();
    let mut steepest = (0.0_f64, 0.0_f64);
    let mut s = 0.0;
    while s < course.table.length() {
        let fall = (-course.table.frame(s).forward.y)
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        if fall > 30.0 {
            if fall > steepest.0 {
                steepest = (fall, s);
            }
        } else if steepest.0 > 0.0 {
            let (angle, where_s) = steepest;
            drops.push(angle);
            if !(DROP_MIN..=DROP_MAX).contains(&angle) {
                issues.push(Issue {
                    text: format!(
                        "A drop falls at {angle:.0} degrees (it should be {DROP_MIN:.0} to {DROP_MAX:.0})"
                    ),
                    at: Some(course.table.position(where_s)),
                });
            }
            steepest = (0.0, 0.0);
        }
        s += 0.1;
    }

    // Lift hills: from the lift's foot to its highest point, searched a little past its end.
    let mut lift_height = None;
    for placed in &course.placed {
        if matches!(placed.element, Element::Lift { .. }) {
            let foot = course.table.position(placed.s0).y;
            let mut top = f64::MIN;
            let mut s = placed.s0;
            while s < (placed.s1 + 30.0).min(course.table.length()) {
                top = top.max(course.table.position(s).y);
                s += 0.5;
            }
            let height = top - foot;
            lift_height = Some(lift_height.map_or(height, |h: f64| h.max(height)));
            if !(LIFT_MIN..=LIFT_MAX).contains(&height) {
                issues.push(Issue {
                    text: format!(
                        "A lift hill is {height:.0} blocks tall (it should be {LIFT_MIN:.0} to {LIFT_MAX:.0})"
                    ),
                    at: Some(course.table.position(placed.s0)),
                });
            }
        }
    }

    Report {
        completes,
        seconds: st.seconds,
        length: course.table.length(),
        top_speed: st.top_speed,
        max_vertical: st.max_vertical,
        min_vertical: st.min_vertical,
        max_lateral: st.max_lateral,
        max_vertical_rate: st.max_vertical_rate,
        airtime_dips: sim::airtime_dips(&samples),
        drops,
        lift_height,
        issues,
    }
}

/// Where the vertical g changes fastest over one second.
fn rate_spot(course: &Course, samples: &[Sample], spacing: f64) -> Option<DVec3> {
    let mut best = (0.0, None);
    for k in 20..samples.len() {
        for i in 0..samples[k].cars {
            let change = (samples[k].felt[i].vertical - samples[k - 20].felt[i].vertical).abs();
            if change > best.0 {
                best = (change, Some(samples[k].s - i as f64 * spacing));
            }
        }
    }
    best.1.map(|s| course.table.position(s))
}

impl Report {
    /// The chat lines (D64). Passed limits are written in red with `§c`; the rest is plain.
    pub fn lines(&self, name: &str) -> Vec<String> {
        let mut out = vec![format!(
            "Realism report for {name}: {:.0} blocks of track, a lap of {:.0} s, top speed {:.0} km/h.",
            self.length,
            self.seconds,
            self.top_speed * 3.6
        )];
        out.push(format!(
            "Up-down push {:.1} to {:.1} g, sideways up to {:.1} g, {} airtime hill(s).",
            self.min_vertical,
            self.max_vertical,
            self.max_lateral,
            self.airtime_dips.len()
        ));
        if !self.drops.is_empty() {
            let drops: Vec<String> = self.drops.iter().map(|d| format!("{d:.0}")).collect();
            out.push(format!("Drops: {} degrees.", drops.join(", ")));
        }
        if let Some(height) = self.lift_height {
            out.push(format!("Lift hill: {height:.0} blocks tall."));
        }
        if self.issues.is_empty() {
            out.push("All within the realism limits.".to_string());
        } else {
            for issue in &self.issues {
                let place = issue.at.map_or(String::new(), |p| format!(" ({})", at(p)));
                out.push(format!("§c{}{place}.§r", issue.text));
            }
            out.push("The ride still opens: it is your coaster.".to_string());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::demo::{self, Shape};

    fn report_for(shape: &Shape) -> (Course, Report) {
        let course = demo::course_for(shape, DVec3::new(100.0, 73.0, -40.0));
        let report = analyse(
            &course,
            demo::CARS,
            crate::coaster::physics::CAR_SPACING,
            &Physics::default(),
        );
        (course, report)
    }

    // Test 8, first half.
    #[test]
    fn the_m2_demo_passes_every_limit() {
        let (_, report) = report_for(&Shape::demo());
        assert!(report.completes);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert!(report.drops.len() >= 2);
        let lift = report.lift_height.unwrap();
        assert!((20.0..=30.0).contains(&lift), "{lift}");
        let lines = report.lines("demo");
        assert!(
            lines
                .last()
                .unwrap()
                .contains("All within the realism limits")
        );
        assert!(lines.iter().all(|l| !l.contains('§')));
    }

    // Test 8, second half: the same layout with much tighter half circles throws riders sideways.
    #[test]
    fn a_sharp_hand_made_track_fails_lateral_g_with_the_right_numbers() {
        let sharp = Shape {
            radius: 6.0,
            ..Shape::demo()
        };
        let (course, report) = report_for(&sharp);
        // The report's number is exactly what the simulation measures...
        let stop = course.station().unwrap().1;
        let samples = sim::run_lap(
            &course,
            Train::held(demo::CARS, stop),
            &Physics::default(),
            300.0,
        );
        let st = sim::stats(&samples);
        assert_eq!(report.max_lateral, st.max_lateral);
        assert!(st.max_lateral > 1.0, "{}", st.max_lateral);
        // ...and it is reported in red, with the number and a place.
        let issue = report
            .issues
            .iter()
            .find(|i| i.text.contains("sideways"))
            .expect("a lateral issue");
        assert!(
            issue.text.contains(&format!("{:.1} g", st.max_lateral)),
            "{}",
            issue.text
        );
        assert!(issue.at.is_some());
        let lines = report.lines("sharp");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("§c") && l.contains("sideways") && l.contains("near "))
        );
        assert!(lines.last().unwrap().contains("still opens"));
    }

    // D68: the demo may open; a fast station approach and a hill too high are refused.
    #[test]
    fn the_demo_has_no_blocker() {
        let (course, _) = report_for(&Shape::demo());
        let spacing = crate::coaster::physics::CAR_SPACING;
        assert_eq!(blocker(&course, 3, spacing, &Physics::default()), None);
    }

    #[test]
    fn without_the_brakes_the_train_overshoots_the_station() {
        let (mut course, _) = report_for(&Shape::demo());
        course
            .placed
            .retain(|p| !matches!(p.element, Element::Brake { .. }));
        let spacing = crate::coaster::physics::CAR_SPACING;
        // Frictionless, so the train arrives fast enough to roll past the stop point.
        let found = blocker(&course, 3, spacing, &Physics::frictionless());
        let station_start = course.station().unwrap().0.s0;
        assert_eq!(found, Some(Blocker::Overshoot { s: station_start }));
    }

    #[test]
    fn without_the_lift_the_train_stalls_and_the_place_is_reported() {
        let (mut course, _) = report_for(&Shape::demo());
        course
            .placed
            .retain(|p| !matches!(p.element, Element::Lift { .. }));
        let spacing = crate::coaster::physics::CAR_SPACING;
        match blocker(&course, 3, spacing, &Physics::default()) {
            Some(Blocker::Stall { s }) => assert!(s > 0.0 && s < course.table.length()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_train_that_cannot_climb_is_reported_as_stalling() {
        // Without a lift, the flat demo-like loop has nothing to push the train round.
        let (mut course, _) = report_for(&Shape::demo());
        course
            .placed
            .retain(|p| !matches!(p.element, Element::Lift { .. }));
        let report = analyse(
            &course,
            3,
            crate::coaster::physics::CAR_SPACING,
            &Physics::default(),
        );
        assert!(!report.completes);
        assert!(report.issues[0].text.contains("cannot finish a lap"));
        assert!(report.issues[0].at.is_some());
    }
}
