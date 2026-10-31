//! Track elements (D13, D57): the parts of the track that act on the train.
//!
//! * A **lift** is a chain: it drags the train up at a steady speed and never lets it roll back.
//! * A **brake** slows the train down to a target speed.
//! * A **station** is where the train stops, waits for riders, and is pushed out again.
//!
//! An element covers a stretch of track `s0..s1` (blocks along the lap). "On an element" means
//! that ANY car of the train is on that stretch (D13).

use crate::coaster::table::Table;

/// Lift chain speed, blocks per second (D57).
pub const LIFT_SPEED: f64 = 5.0;
/// How hard a brake slows the train, blocks per second squared (D57).
pub const BRAKE_DECEL: f64 = 6.0;
/// The speed a brake slows the train down to, blocks per second (D57).
pub const BRAKE_TARGET: f64 = 3.0;
/// The speed the station pushes the train out at, blocks per second (D57).
pub const STATION_PUSH: f64 = 2.0;
/// How long the train waits in the station after someone sits, seconds (D56, D57).
pub const DWELL: f64 = 5.0;

/// What an element does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    /// A chain at `speed` blocks/s.
    Lift { speed: f64 },
    /// Slows the train to `target_v` at `decel` blocks/s².
    Brake { target_v: f64, decel: f64 },
    /// The train stops with its front car at `stop` (a distance along the lap, inside the
    /// station's stretch), waits, and is pushed out at `push` blocks/s.
    Station { stop: f64, push: f64 },
}

impl Element {
    /// A lift with the default chain speed.
    pub fn lift() -> Element {
        Element::Lift { speed: LIFT_SPEED }
    }
    /// A brake with the default numbers.
    pub fn brake() -> Element {
        Element::Brake {
            target_v: BRAKE_TARGET,
            decel: BRAKE_DECEL,
        }
    }
    /// A station whose stop point is `stop` blocks along the lap.
    pub fn station(stop: f64) -> Element {
        Element::Station {
            stop,
            push: STATION_PUSH,
        }
    }
}

/// One element placed on a stretch of track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub s0: f64,
    pub s1: f64,
    pub element: Element,
}

/// A track together with everything on it: the thing a train rides on.
#[derive(Clone, Debug)]
pub struct Course {
    pub table: Table,
    pub placed: Vec<Placed>,
}

impl Course {
    /// A course with no elements (a plain loop of track).
    pub fn bare(table: Table) -> Course {
        Course {
            table,
            placed: Vec::new(),
        }
    }

    /// Is the point `s` (any distance, wrapped to one lap) inside the stretch `p`?
    pub fn covers(&self, p: &Placed, s: f64) -> bool {
        let s = self.table.wrap(s);
        s >= p.s0 && s < p.s1
    }

    /// The first placed element for which `pick` says yes and that any of `car_s` stands on.
    pub fn on<'a, T>(
        &'a self,
        car_s: &[f64],
        pick: impl Fn(&Element) -> Option<T>,
    ) -> Option<(&'a Placed, T)> {
        self.placed.iter().find_map(|p| {
            let value = pick(&p.element)?;
            car_s
                .iter()
                .any(|&s| self.covers(p, s))
                .then_some((p, value))
        })
    }

    /// The station, if the course has one.
    pub fn station(&self) -> Option<(&Placed, f64, f64)> {
        self.placed.iter().find_map(|p| match p.element {
            Element::Station { stop, push } => Some((p, stop, push)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::spline::Track;
    use glam::DVec3;

    fn loop_course() -> Course {
        // A flat square loop 100 blocks around.
        let nodes = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(25.0, 0.0, 0.0),
            DVec3::new(25.0, 0.0, 25.0),
            DVec3::new(0.0, 0.0, 25.0),
        ];
        Course::bare(Table::new(&Track::closed(&nodes)))
    }

    #[test]
    fn the_default_numbers_are_the_approved_ones() {
        assert_eq!(LIFT_SPEED, 5.0);
        assert_eq!(BRAKE_DECEL, 6.0);
        assert_eq!(BRAKE_TARGET, 3.0);
        assert_eq!(STATION_PUSH, 2.0);
        assert_eq!(DWELL, 5.0);
    }

    #[test]
    fn any_car_on_an_element_counts() {
        let mut course = loop_course();
        course.placed.push(Placed {
            s0: 10.0,
            s1: 20.0,
            element: Element::lift(),
        });
        let pick = |e: &Element| matches!(e, Element::Lift { .. }).then_some(());
        assert!(course.on(&[5.0, 8.0, 12.0], pick).is_some());
        assert!(course.on(&[5.0, 8.0, 9.9], pick).is_none());
        // Distances past the end of the lap wrap around.
        let lap = course.table.length();
        assert!(course.on(&[lap + 15.0], pick).is_some());
    }
}
