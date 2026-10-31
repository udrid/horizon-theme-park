//! Run a whole ride on paper: step a train round a course and write down what happens each tick.
//!
//! The same code is used by tests (does the layout stay within the realism limits?), by
//! `park-sim` (prints the table as CSV) and by the runtime (to see where a car will be a few
//! ticks from now).

use crate::coaster::elements::Course;
use crate::coaster::forces::{self, Felt};
use crate::coaster::physics::{MAX_CARS, Physics, Train};
use crate::coaster::table::Table;

/// One tick of a simulated ride.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Seconds since the start.
    pub t: f64,
    /// Front car's distance along the lap.
    pub s: f64,
    /// Speed, blocks per second.
    pub v: f64,
    /// What each car's rider feels (only the first `cars` entries are real).
    pub felt: [Felt; MAX_CARS],
    /// How many cars the train has.
    pub cars: usize,
}

/// What each car's rider feels right now.
pub fn felt_all(course: &Course, train: &Train, physics: &Physics) -> [Felt; MAX_CARS] {
    let mut out = [Felt::default(); MAX_CARS];
    let positions = train.car_positions();
    for i in 0..train.cars {
        out[i] = forces::felt(
            &course.table,
            positions[i],
            train.v,
            train.a,
            physics.gravity_scale,
        );
    }
    out
}

/// Let the train leave the station and go round once, until it is held in the station again
/// (or `max_seconds` pass). Returns one sample per tick, first sample at t = 0.
pub fn run_lap(
    course: &Course,
    mut train: Train,
    physics: &Physics,
    max_seconds: f64,
) -> Vec<Sample> {
    let mut out = Vec::new();
    train.start_countdown(0.0);
    let mut t = 0.0;
    let mut left = false;
    while t < max_seconds {
        out.push(Sample {
            t,
            s: train.s,
            v: train.v,
            felt: felt_all(course, &train, physics),
            cars: train.cars,
        });
        train.step(course, physics);
        t += 0.05;
        if !train.is_held() {
            left = true;
        } else if left {
            break;
        }
    }
    out
}

/// The numbers that decide whether a ride feels real (D58), taken over every car of the train.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub top_speed: f64,
    /// Slowest speed once the train has left the station.
    pub slowest_speed: f64,
    /// Largest vertical g (the valleys).
    pub max_vertical: f64,
    /// Smallest vertical g (the airtime hills; negative = out of the seat).
    pub min_vertical: f64,
    /// Largest sideways g, ignoring the sign.
    pub max_lateral: f64,
    /// Largest forward or backward push, ignoring the sign.
    pub max_longitudinal: f64,
    /// Biggest change of vertical g over any one second (D58: at most 3 g per second).
    pub max_vertical_rate: f64,
    /// The same measured over a quarter second, scaled to g per second: stricter, for information.
    pub max_vertical_rate_quarter: f64,
    /// How long the lap took, seconds.
    pub seconds: f64,
}

/// Measure a simulated ride.
pub fn stats(samples: &[Sample]) -> Stats {
    let mut st = Stats {
        slowest_speed: f64::MAX,
        min_vertical: f64::MAX,
        max_vertical: f64::MIN,
        ..Stats::default()
    };
    // A second is 20 ticks, a quarter second 5.
    const WINDOW: usize = 20;
    const QUARTER: usize = 5;
    for (k, sample) in samples.iter().enumerate() {
        st.top_speed = st.top_speed.max(sample.v);
        if k > 20 && k + 20 < samples.len() {
            st.slowest_speed = st.slowest_speed.min(sample.v);
        }
        for i in 0..sample.cars {
            let f = sample.felt[i];
            st.max_vertical = st.max_vertical.max(f.vertical);
            st.min_vertical = st.min_vertical.min(f.vertical);
            st.max_lateral = st.max_lateral.max(f.lateral.abs());
            st.max_longitudinal = st.max_longitudinal.max(f.longitudinal.abs());
            if k >= WINDOW {
                let before = samples[k - WINDOW].felt[i].vertical;
                st.max_vertical_rate = st
                    .max_vertical_rate
                    .max((f.vertical - before).abs() / (WINDOW as f64 * 0.05));
            }
            if k >= QUARTER {
                let before = samples[k - QUARTER].felt[i].vertical;
                st.max_vertical_rate_quarter = st
                    .max_vertical_rate_quarter
                    .max((f.vertical - before).abs() / (QUARTER as f64 * 0.05));
            }
        }
    }
    st.seconds = samples.last().map_or(0.0, |s| s.t);
    st
}

/// The sharpest "speed squared times bend" any car meets on one lap (S3, D44): it decides how
/// often a car display may be moved. Found by one dry lap at build time.
pub fn worst_speed_sq_curvature(course: &Course, train: &Train, physics: &Physics) -> f64 {
    let samples = run_lap(course, *train, physics, 300.0);
    // Re-walk the lap to see where each car was (samples keep only the front car's distance).
    let mut worst: f64 = 0.0;
    for sample in &samples {
        for i in 0..train.cars {
            let s = sample.s - i as f64 * train.spacing;
            worst = worst.max(sample.v * sample.v * bend(&course.table, s));
        }
    }
    worst
}

fn bend(table: &Table, s: f64) -> f64 {
    forces::curvature(table, s).length()
}

/// Every airtime dip of the ride: the lowest vertical g (over all cars) in each stretch where it
/// falls below zero. Dips shallower than -0.02 g are not counted.
pub fn airtime_dips(samples: &[Sample]) -> Vec<f64> {
    let mut dips = Vec::new();
    let mut lowest: Option<f64> = None;
    for sample in samples {
        let g = (0..sample.cars)
            .map(|i| sample.felt[i].vertical)
            .fold(f64::MAX, f64::min);
        if g < 0.0 {
            lowest = Some(lowest.map_or(g, |l| l.min(g)));
        } else if g > 0.3
            && let Some(l) = lowest.take()
            && l < -0.02
        {
            dips.push(l);
        } else if g > 0.3 {
            lowest = None;
        }
    }
    dips
}
