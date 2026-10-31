// park-sim: runs park-core's physics on paper and prints the result.
//
//   cargo run -p park-sim -- demo > lap.csv    one lap of the demo coaster as a table (CSV)
//
// A CSV file is just rows of numbers separated by commas: open it in a spreadsheet and chart it.
// A short summary of the ride's limits (D58) goes to the screen's error channel, so it does not
// end up inside the file.
//
// For tuning the layout there are two extras:
//   --debug         also prints a line every half second
//   --shape "..."   tries another shape (see `parse_shape`) and prints one `key=value` line

use glam::DVec3;
use park_core::coaster::demo::{self, Grade, Shape};
use park_core::coaster::elements::Course;
use park_core::coaster::physics::{Physics, Train};
use park_core::coaster::sim::{self, Sample};

/// The CSV header line (S4).
const HEADER: &str = "time_s,s_blocks,speed_blocks_s,speed_kmh,vertical_g,lateral_g,longitudinal_g";

/// One lap as CSV text, with the front car's g-forces.
fn lap_csv(samples: &[Sample]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for s in samples {
        let f = s.felt[0];
        out.push_str(&format!(
            "{:.2},{:.2},{:.2},{:.1},{:.2},{:.2},{:.2}\n",
            s.t,
            s.s,
            s.v,
            s.v * 3.6,
            f.vertical,
            f.lateral,
            f.longitudinal
        ));
    }
    out
}

/// One lap of a course, starting from the stop point in the station.
fn run(course: &Course) -> Vec<Sample> {
    let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
    sim::run_lap(
        course,
        Train::held(demo::CARS, stop),
        &Physics::default(),
        300.0,
    )
}

/// A shape from text: `radius,straight,phase,lift_from,lift_to,brake_len;at,degrees,blend;at,degrees,blend;...`
fn parse_shape(text: &str) -> Option<Shape> {
    let mut parts = text.split(';');
    let head: Vec<f64> = parts
        .next()?
        .split(',')
        .map(|v| v.trim().parse().ok())
        .collect::<Option<_>>()?;
    let mut grades = Vec::new();
    for part in parts {
        let g: Vec<f64> = part
            .split(',')
            .map(|v| v.trim().parse().ok())
            .collect::<Option<_>>()?;
        grades.push(Grade {
            at: *g.first()?,
            degrees: *g.get(1)?,
            blend: *g.get(2)?,
        });
    }
    Some(Shape {
        radius: *head.first()?,
        straight: *head.get(1)?,
        phase: *head.get(2)?,
        lift: (*head.get(3)?, *head.get(4)?),
        brake_len: *head.get(5)?,
        grades,
    })
}

/// The numbers that matter, on one line (for scripts).
fn metrics(course: &Course, samples: &[Sample]) -> String {
    let st = sim::stats(samples);
    let table = &course.table;
    let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
    let mut steepest = 0.0_f64;
    let mut drops: Vec<String> = Vec::new();
    let mut this_drop = 0.0_f64;
    let mut s = 0.0;
    while s < table.length() {
        let f = table.frame(s);
        ymin = ymin.min(f.pos.y);
        ymax = ymax.max(f.pos.y);
        let fall = (-f.forward.y).asin().to_degrees();
        steepest = steepest.max(fall);
        // A drop is a stretch steeper than 30 degrees; its angle is the steepest point in it.
        if fall > 30.0 {
            this_drop = this_drop.max(fall);
        } else if this_drop > 0.0 {
            drops.push(format!("{this_drop:.1}"));
            this_drop = 0.0;
        }
        s += 0.1;
    }
    let dips: Vec<String> = sim::airtime_dips(samples)
        .iter()
        .map(|d| format!("{d:.2}"))
        .collect();
    format!(
        "length={:.1} seconds={:.1} top={:.1} slowest={:.2} gmax={:.2} gmin={:.2} lat={:.2} rate1={:.2} rate025={:.2} ymin={:.2} ymax={:.2} steepest={:.1} drops={} dips={}",
        table.length(),
        st.seconds,
        st.top_speed,
        st.slowest_speed,
        st.max_vertical,
        st.min_vertical,
        st.max_lateral,
        st.max_vertical_rate,
        st.max_vertical_rate_quarter,
        ymin,
        ymax,
        steepest,
        drops.join("/"),
        dips.join("/")
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("demo") => {
            let shape = match args.iter().position(|a| a == "--shape") {
                Some(i) => match args.get(i + 1).and_then(|t| parse_shape(t)) {
                    Some(shape) => shape,
                    None => {
                        eprintln!("Could not read the --shape text.");
                        return;
                    }
                },
                None => Shape::demo(),
            };
            let course = demo::course_for(&shape, DVec3::ZERO);
            let samples = run(&course);
            if args.iter().any(|a| a == "--shape") {
                println!("{}", metrics(&course, &samples));
                return;
            }
            print!("{}", lap_csv(&samples));
            eprintln!("{:#?}", sim::stats(&samples));
            eprintln!("{}", metrics(&course, &samples));
            if args.iter().any(|a| a == "--debug") {
                for s in samples.iter().step_by(10) {
                    let p = course.table.position(s.s);
                    eprintln!(
                        "t {:5.1} s {:6.1} y {:5.1} v {:5.1} gv {:5.2} gl {:5.2}",
                        s.t, s.s, p.y, s.v, s.felt[0].vertical, s.felt[0].lateral
                    );
                }
            }
        }
        _ => eprintln!("Usage: cargo run -p park-sim -- demo   (prints one lap as CSV)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_csv_has_the_agreed_header_and_one_row_per_tick() {
        let course = demo::demo_course(DVec3::ZERO);
        let samples = run(&course);
        let csv = lap_csv(&samples);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some("time_s,s_blocks,speed_blocks_s,speed_kmh,vertical_g,lateral_g,longitudinal_g")
        );
        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), samples.len());
        assert!(rows.iter().all(|r| r.split(',').count() == 7));
        assert!(rows[0].starts_with("0.00,"));
    }

    #[test]
    fn a_shape_can_be_written_as_text_and_read_back() {
        let shape = parse_shape("15,100,236.7,14,75,16;0,0,1;21,20,10;80,-52,18").unwrap();
        assert_eq!(shape.radius, 15.0);
        assert_eq!(shape.grades.len(), 3);
        assert_eq!(shape.grades[2].degrees, -52.0);
        assert!(parse_shape("15,100").is_none());
    }
}
