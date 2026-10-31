//! The physics of one train (D11-D15, ARCHITECTURE §4).
//!
//! The whole train is ONE rigid body: one position `s` (its front car) and one speed `v` (D13).
//! Car `i` rides `i * spacing` blocks behind the front car. Each server tick we run
//! 10 tiny steps ("substeps", 0.005 s each, D12). Each step:
//!
//! 1. gravity pulls every car along its own slope, and we average over the cars (D13);
//! 2. rolling friction and air drag push against the motion (D14);
//! 3. elements (lift, brake, station) act on the train when any car is on them;
//! 4. speed changes first, then the position moves with the NEW speed. That order is called
//!    "semi-implicit Euler" and, unlike the plain order, it does not invent or lose energy on a
//!    loop (D12). It is like how you would walk a rolling ball down a hill in tiny steps.

use crate::coaster::elements::{Course, Element};
use crate::coaster::forces::{self, G};

/// Substeps per server tick (D12).
pub const SUBSTEPS: u32 = 10;
/// Length of one substep in seconds: a tick is 0.05 s.
pub const SUBSTEP_DT: f64 = 0.005;
/// The most cars one train can have.
pub const MAX_CARS: usize = 8;
/// Distance between neighbouring cars' middles: a 3-block car plus a 0.2 gap.
pub const CAR_SPACING: f64 = 3.2;
/// Below this speed a train on a gentle slope is considered stopped (so it does not jitter).
const SNAP_SPEED: f64 = 0.05;
/// The station slows the train to a stop along a curve with this much slowing-down (blocks/s²)...
const STATION_COMFORT_DECEL: f64 = 1.0;
/// ...but never creeps slower than this, so a train always reaches the stop point.
const STATION_CREEP: f64 = 0.05;
/// A train this close to the stop point counts as arrived.
const STATION_ARRIVE: f64 = 0.02;
/// How hard the chain, the station wheels and the push-out accelerate the train, blocks per
/// second squared (about 0.15 g, like a real chain engaging). Nothing ever jumps the speed.
pub const ELEMENT_ACCEL: f64 = 1.5;

/// The numbers the physics needs (D11, D14). Defaults are the approved ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Physics {
    /// 1.0 = real gravity (9.81 blocks/s²).
    pub gravity_scale: f64,
    /// Rolling friction: a fraction of the normal push (D14).
    pub friction: f64,
    /// Air drag per block (D14).
    pub drag: f64,
}

impl Default for Physics {
    fn default() -> Self {
        Physics {
            gravity_scale: 1.0,
            friction: 0.015,
            drag: 0.0015,
        }
    }
}

impl Physics {
    /// No friction and no drag: for tests of pure gravity.
    pub fn frictionless() -> Physics {
        Physics {
            gravity_scale: 1.0,
            friction: 0.0,
            drag: 0.0,
        }
    }
}

/// Is the train held in the station?
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    /// Moving (or waiting for nothing).
    Free,
    /// Standing at the station's stop point. `countdown` is the seconds left until it leaves,
    /// or `None` while it waits for someone to sit (D56).
    Held { countdown: Option<f64> },
}

/// One train: a rigid body on the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Train {
    /// Distance of the FRONT car's middle along the lap, in blocks.
    pub s: f64,
    /// Speed along the track, blocks per second (negative = rolling backwards).
    pub v: f64,
    /// The last substep's speed-up, blocks per second squared (used for the g-forces).
    pub a: f64,
    pub cars: usize,
    pub spacing: f64,
    pub hold: Hold,
}

fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

impl Train {
    /// A moving train at `s` with speed `v`.
    pub fn free(cars: usize, s: f64, v: f64) -> Train {
        Train {
            s,
            v,
            a: 0.0,
            cars: cars.clamp(1, MAX_CARS),
            spacing: CAR_SPACING,
            hold: Hold::Free,
        }
    }

    /// A train standing in the station with its front car at `s`, waiting for riders.
    pub fn held(cars: usize, s: f64) -> Train {
        Train {
            hold: Hold::Held { countdown: None },
            ..Train::free(cars, s, 0.0)
        }
    }

    /// The middle of every car, front first. Only the first `self.cars` entries are used.
    pub fn car_positions(&self) -> [f64; MAX_CARS] {
        let mut out = [0.0; MAX_CARS];
        for (i, slot) in out.iter_mut().enumerate().take(self.cars) {
            *slot = self.s - i as f64 * self.spacing;
        }
        out
    }

    /// True while the train stands in the station.
    pub fn is_held(&self) -> bool {
        matches!(self.hold, Hold::Held { .. })
    }

    /// Seconds left of the station countdown, if one is running.
    pub fn countdown(&self) -> Option<f64> {
        match self.hold {
            Hold::Held { countdown } => countdown,
            Hold::Free => None,
        }
    }

    /// Start the station countdown (D56) if the train is waiting and none is running.
    pub fn start_countdown(&mut self, seconds: f64) {
        if let Hold::Held { countdown } = &mut self.hold
            && countdown.is_none()
        {
            *countdown = Some(seconds);
        }
    }

    /// Stop the countdown (everyone got off): the train waits again.
    pub fn cancel_countdown(&mut self) {
        if let Hold::Held { countdown } = &mut self.hold {
            *countdown = None;
        }
    }

    /// One server tick: ten substeps.
    pub fn step(&mut self, course: &Course, physics: &Physics) {
        for _ in 0..SUBSTEPS {
            self.substep(course, physics);
        }
    }

    /// One substep of 0.005 s.
    fn substep(&mut self, course: &Course, physics: &Physics) {
        let dt = SUBSTEP_DT;
        let table = &course.table;
        if table.is_empty() {
            return;
        }

        // Standing in the station: nothing moves until the countdown runs out.
        if let Hold::Held { countdown } = &mut self.hold {
            self.v = 0.0;
            self.a = 0.0;
            match countdown {
                Some(t) if *t > dt => {
                    *t -= dt;
                    return;
                }
                Some(_) => self.hold = Hold::Free,
                None => return,
            }
        }

        let n = self.cars;
        let positions = self.car_positions();
        let cars = &positions[..n];
        let g = G * physics.gravity_scale;

        // 1-2. Gravity, friction and drag (D13, D14).
        let mut slope = 0.0; // sum of the cars' `forward.y`
        let mut normal = 0.0; // sum of the cars' normal push
        for &c in cars {
            let (frame, push) = forces::push(table, c, self.v, self.a, g);
            slope += frame.forward.y;
            normal += forces::normal_push(&frame, push);
        }
        let a_gravity = -g * slope / n as f64;
        let friction_limit = physics.friction * normal / n as f64;
        let a_friction = -sign(self.v) * friction_limit;
        let a_drag = -sign(self.v) * physics.drag * self.v * self.v;
        let mut a = a_gravity + a_friction + a_drag;

        // 3. Elements. Which ones is any car on?
        let lift = course.on(cars, |e| match e {
            Element::Lift { speed } => Some(*speed),
            _ => None,
        });
        let brake = course.on(cars, |e| match e {
            Element::Brake { target_v, decel } => Some((*target_v, *decel)),
            _ => None,
        });
        // The station: only the front car decides if we are arriving or leaving.
        let front = table.wrap(self.s);
        let station = course
            .station()
            .filter(|(p, _, _)| cars.iter().any(|&c| course.covers(p, c)));
        let arriving = station.filter(|(p, stop, _)| front >= p.s0 && front < *stop);
        let leaving = station.filter(|(_, stop, _)| front >= *stop);

        let mut braking = false;
        if let Some((_, (target, decel))) = brake
            && self.v > target
        {
            a -= decel;
            braking = true;
        }
        // Some elements pull a slow train UP to a speed: the chain, the station's push-out and
        // the station's wheels. They never jump the speed: they accelerate the train gently
        // (`ELEMENT_ACCEL`), cancelling gravity and friction first, like a chain engaging.
        let chain_speed = match (lift, leaving) {
            (Some((_, speed)), Some((_, _, push))) => Some(speed.max(push)),
            (Some((_, speed)), None) => Some(speed),
            (None, Some((_, _, push))) => Some(push),
            (None, None) => None,
        };
        let mut pull_to: Option<f64> = None;
        let mut raise = |target: f64| pull_to = Some(pull_to.map_or(target, |t| t.max(target)));
        if let Some((_, speed)) = lift {
            raise(speed);
        }
        if let Some((_, _, push)) = leaving {
            raise(push);
        }
        // Arriving: follow a gentle slowing-down curve that ends exactly at the stop point.
        let mut station_speed = None;
        if let Some((_, stop, push)) = arriving {
            let d = (stop - front).max(0.0);
            let wanted = (2.0 * STATION_COMFORT_DECEL * d).sqrt().clamp(
                STATION_CREEP,
                crate::coaster::elements::BRAKE_TARGET.max(push),
            );
            station_speed = Some(wanted);
            if self.v > wanted {
                // Brakes do not stack: a train already being braked is not braked twice as hard.
                if !braking {
                    a -= crate::coaster::elements::BRAKE_DECEL;
                }
            } else {
                raise(wanted);
            }
        }
        if let Some(target) = pull_to
            && self.v < target
        {
            a = ELEMENT_ACCEL;
        }

        // 4. Speed first...
        let v_before = self.v;
        self.v += a * dt;

        // ...then the limits of the elements.
        if let Some((_, (target, _))) = brake
            && v_before > target
            && self.v < target
        {
            self.v = target; // the brake never takes more than it must
        }
        if let Some(wanted) = station_speed
            && v_before > wanted
            && self.v < wanted
        {
            self.v = wanted; // the curve is a floor for a train that is too fast
        }
        if let Some(target) = pull_to
            && v_before < target
            && self.v > target
        {
            self.v = target; // the pull stops at its speed
        }
        // The chain (and the push-out) HOLD their speed against gravity: a train that has just
        // reached it is not allowed to slip back below it.
        if let Some(target) = chain_speed
            && v_before >= target
            && self.v < target
            && self.v > target - 0.5
        {
            self.v = target;
        }
        if lift.is_some() && self.v < 0.0 {
            self.v = 0.0; // safety net: the chain never lets the train roll back
        }
        // A train on a gentle slope that is nearly stopped stays stopped (no jitter).
        if lift.is_none()
            && station.is_none()
            && self.v.abs() < SNAP_SPEED
            && a_gravity.abs() <= friction_limit
        {
            self.v = 0.0;
        }

        self.a = (self.v - v_before) / dt;

        // ...then the position, with the NEW speed (D12).
        self.s = table.wrap(self.s + self.v * dt);

        // Arrived at the stop point? Stop exactly there and wait.
        if let Some((p, stop, _)) = arriving {
            let now = table.wrap(self.s);
            if now >= stop - STATION_ARRIVE && now < stop + 1.0 && now >= p.s0 {
                self.s = stop;
                self.v = 0.0;
                self.a = 0.0;
                self.hold = Hold::Held { countdown: None };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaster::demo;
    use crate::coaster::elements::Placed;
    use crate::coaster::spline::Track;
    use crate::coaster::table::Table;
    use glam::DVec3;

    /// A stadium: two straights `len` long, joined by half circles of radius `r`, in the flat
    /// plane at height `y`. Starts at the beginning of the first straight, heading +x.
    fn stadium_nodes(len: f64, r: f64, y: f64) -> Vec<DVec3> {
        let mut n = Vec::new();
        let mut x = 0.0;
        while x < len - 1e-9 {
            n.push(DVec3::new(x, y, 0.0));
            x += 2.0;
        }
        for i in 0..=18 {
            let a = (i as f64 * 10.0).to_radians();
            n.push(DVec3::new(len + r * a.sin(), y, r - r * a.cos()));
        }
        let mut x = len - 2.0;
        while x > 1e-9 {
            n.push(DVec3::new(x, y, 2.0 * r));
            x -= 2.0;
        }
        for i in 0..=18 {
            let a = (i as f64 * 10.0).to_radians();
            n.push(DVec3::new(-r * a.sin(), y, r + r * a.cos()));
        }
        // The last node duplicates the first one: drop it.
        n.pop();
        n
    }

    fn flat_course() -> Course {
        Course::bare(Table::new(&Track::closed(&stadium_nodes(
            120.0, 15.0, 10.0,
        ))))
    }

    /// A bumpy loop: a smooth hill 40 blocks tall on a long oval.
    fn tall_hill_course() -> Course {
        let nodes: Vec<DVec3> = (0..96)
            .map(|i| {
                let a = i as f64 / 96.0 * std::f64::consts::TAU;
                DVec3::new(90.0 * a.cos(), 20.0 * a.cos() + 20.0, 25.0 * a.sin())
            })
            .collect();
        Course::bare(Table::new(&Track::closed(&nodes)))
    }

    // Test 1: no friction, many laps: no speed invented or lost.
    #[test]
    fn a_frictionless_loop_keeps_its_speed_within_one_percent_after_100_laps() {
        let course = Course::bare(Table::new(&Track::closed(&{
            // A loop that goes up and down.
            (0..48)
                .map(|i| {
                    let a = i as f64 / 48.0 * std::f64::consts::TAU;
                    DVec3::new(40.0 * a.cos(), 6.0 * (2.0 * a).sin(), 20.0 * a.sin())
                })
                .collect::<Vec<_>>()
        })));
        let mut train = Train::free(1, 0.0, 12.0);
        let h = |s: f64| course.table.position(s).y;
        let e0 = 0.5 * 12.0 * 12.0 + G * h(0.0);
        let mut ticks = 0u32;
        let mut laps_done = 0.0;
        let mut last = train.s;
        while laps_done < 100.0 {
            train.step(&course, &Physics::frictionless());
            if train.s < last {
                laps_done += 1.0;
            }
            last = train.s;
            ticks += 1;
            assert!(ticks < 2_000_000, "never finished 100 laps");
        }
        // Same energy means same speed at the same place: compare via energy at the current spot.
        let v_expected = (2.0 * (e0 - G * h(train.s))).sqrt();
        assert!(
            (train.v - v_expected).abs() / v_expected < 0.01,
            "speed {} vs {}",
            train.v,
            v_expected
        );
    }

    // Test 2: the same ride twice gives the same numbers.
    #[test]
    fn the_same_ride_twice_is_identical_at_every_tick() {
        let course = tall_hill_course();
        let mut a = Train::free(3, 1.0, 4.0);
        let mut b = Train::free(3, 1.0, 4.0);
        for _ in 0..600 {
            a.step(&course, &Physics::default());
            b.step(&course, &Physics::default());
            assert_eq!(a, b);
        }
    }

    // Test 3: a frictionless drop: v = sqrt(2 g h).
    #[test]
    fn a_frictionless_40_block_drop_from_rest_ends_at_28_01_blocks_a_second() {
        let course = tall_hill_course();
        // Find the top of the hill and start a hair past it.
        let table = &course.table;
        let mut s_top = 0.0;
        let mut y_top = f64::MIN;
        let mut s = 0.0;
        while s < table.length() {
            let y = table.position(s).y;
            if y > y_top {
                y_top = y;
                s_top = s;
            }
            s += 0.05;
        }
        let mut train = Train::free(1, s_top + 0.3, 0.0);
        let y_start = table.position(train.s).y;
        let mut best: f64 = 0.0;
        let mut y_bottom = f64::MAX;
        for _ in 0..4000 {
            train.step(&course, &Physics::frictionless());
            if train.v > best {
                best = train.v;
                y_bottom = table.position(train.s).y;
            }
            if train.v < best - 1.0 {
                break;
            }
        }
        let expected = (2.0 * G * (y_start - y_bottom)).sqrt();
        assert!(y_start - y_bottom > 39.5, "drop was {}", y_start - y_bottom);
        assert!(
            (best - expected).abs() / expected < 0.005,
            "bottom speed {best}, expected {expected}"
        );
        assert!((expected - 28.01).abs() < 0.3, "expected {expected}");
    }

    // Test 5: friction and drag on a flat straight.
    #[test]
    fn a_flat_straight_at_20_blocks_a_second_loses_speed_as_worked_out_by_hand() {
        let course = flat_course();
        let mut train = Train::free(1, 10.0, 20.0);
        let start = train.s;
        while train.s - start < 100.0 {
            train.step(&course, &Physics::default());
        }
        // We may overshoot 100 blocks by up to one tick (1 block): use the exact hand formula
        // at the distance reached. ds = -v dv / (mu G + k v^2).
        let (mu_g, k) = (0.015 * G, 0.0015);
        let d = train.s - start;
        let v_hand = (((mu_g + k * 400.0) * (-2.0 * k * d).exp() - mu_g) / k).sqrt();
        assert!((train.v - v_hand).abs() < 0.02, "{} vs {}", train.v, v_hand);
        // And at exactly 100 blocks the hand formula gives 16.46.
        let at_100 = (((mu_g + k * 400.0) * (-2.0 * k * 100.0_f64).exp() - mu_g) / k).sqrt();
        assert!((at_100 - 16.46).abs() < 0.1, "{at_100}");
    }

    // Test 6: a long train over an uneven hill: gentle up, steep down.
    #[test]
    fn on_an_uneven_hill_the_back_car_is_faster_at_the_top_than_the_front_car_was() {
        // A hill: slope +15 degrees up, then -45 degrees down, rounded at the top.
        let mut nodes = Vec::new();
        let (up, down) = (15f64.to_radians().tan(), -(45f64.to_radians().tan()));
        let mut y = 0.0_f64;
        let mut x = 0.0_f64;
        while x < 60.0 {
            // 5 block rounded crest around x = 30.
            let w = ((x - 27.5) / 5.0).clamp(0.0, 1.0);
            let m = up + (down - up) * (w * w * (3.0 - 2.0 * w));
            nodes.push(DVec3::new(x, y, 0.0));
            y += m * 1.5;
            x += 1.5;
        }
        // Run-out and return: a loop closing back to the start at a lower level.
        let end_y = y;
        for i in 0..24 {
            let a = (i as f64 / 24.0) * std::f64::consts::PI;
            nodes.push(DVec3::new(
                60.0 + 20.0 * a.sin(),
                end_y,
                20.0 - 20.0 * a.cos(),
            ));
        }
        // Flat return straight at the lower level, then lift back to the start height.
        for i in 0..40 {
            let x = 60.0 - i as f64 * 1.5;
            let t = i as f64 / 40.0;
            nodes.push(DVec3::new(x, end_y * (1.0 - t), 40.0));
        }
        let _ = nodes.len();
        let course = Course::bare(Table::new(&Track::closed(&nodes)));
        let mut train = Train::free(6, 5.0 * CAR_SPACING + 1.0, 14.0);

        // Crest = the highest point of the track.
        let table = &course.table;
        let (mut s_top, mut y_top, mut s) = (0.0, f64::MIN, 0.0);
        while s < table.length() {
            let y = table.position(s).y;
            if y > y_top {
                y_top = y;
                s_top = s;
            }
            s += 0.05;
        }
        let mut v_front = None;
        let mut v_back = None;
        let back_offset = 5.0 * CAR_SPACING;
        for _ in 0..2000 {
            let before = train;
            train.step(&course, &Physics::default());
            if v_front.is_none() && before.s < s_top && train.s >= s_top {
                v_front = Some(train.v);
            }
            if v_back.is_none() && before.s - back_offset < s_top && train.s - back_offset >= s_top
            {
                v_back = Some(train.v);
            }
        }
        let (front, back) = (v_front.unwrap(), v_back.unwrap());
        assert!(back > front, "front {front}, back {back}");
    }

    fn course_with(element: Element, s0: f64, s1: f64) -> Course {
        let mut course = flat_course();
        course.placed.push(Placed { s0, s1, element });
        course
    }

    // Test 8: the lift.
    #[test]
    fn on_the_lift_the_train_never_rolls_back_and_leaves_at_the_chain_speed() {
        // A track that climbs 30 degrees up the lift, then flat.
        let mut nodes = Vec::new();
        for i in 0..60 {
            let x = i as f64 * 2.0;
            let climb = (x - 20.0).clamp(0.0, 60.0);
            nodes.push(DVec3::new(x, climb * 0.5, 0.0));
        }
        let top = nodes.last().unwrap().y;
        for i in 0..24 {
            let a = (i as f64 / 24.0) * std::f64::consts::PI;
            nodes.push(DVec3::new(
                118.0 + 10.0 * a.sin(),
                top,
                10.0 - 10.0 * a.cos(),
            ));
        }
        for i in 0..59 {
            nodes.push(DVec3::new(
                116.0 - i as f64 * 2.0,
                top * (1.0 - i as f64 / 59.0),
                20.0,
            ));
        }
        let table = Table::new(&Track::closed(&nodes));
        let mut course = Course::bare(table);
        course.placed.push(Placed {
            s0: 25.0,
            s1: 110.0,
            element: Element::lift(),
        });
        // A train stopped on the lift: the chain takes it up to its speed, gently.
        let mut train = Train::free(3, 30.0, 0.0);
        let mut at_speed = false;
        let mut left = false;
        for _ in 0..2000 {
            for _ in 0..SUBSTEPS {
                train.substep(&course, &Physics::default());
                assert!(train.v >= 0.0, "rolled back: {}", train.v);
                assert!(train.a.abs() <= ELEMENT_ACCEL + 1e-6, "jolt: {}", train.a);
            }
            if train.v >= 5.0 - 1e-9 {
                at_speed = true;
            }
            if at_speed && train.s - 2.0 * CAR_SPACING < 110.0 {
                assert!(train.v >= 5.0 - 1e-9, "slower than the chain: {}", train.v);
            }
            if train.s - 2.0 * CAR_SPACING >= 111.0 {
                left = true;
                break;
            }
        }
        assert!(at_speed && left, "never got going or never left the lift");
        assert!(
            (train.v - 5.0).abs() < 0.5,
            "leaves at about the chain speed: {}",
            train.v
        );
    }

    // Test 9: the brake.
    #[test]
    fn the_train_leaves_the_brake_run_at_the_target_speed() {
        // A brake run 20 blocks long: enough to slow from 15 to 3 (it needs about 18). Friction
        // is switched off so the test measures the brake alone: a brake only ever takes speed
        // away, so on real track the train would keep slowing a little after reaching the target.
        let course = course_with(Element::brake(), 20.0, 40.0);
        let mut train = Train::free(1, 5.0, 15.0);
        let mut guard = 0;
        while train.s < 40.5 {
            train.step(&course, &Physics::frictionless());
            guard += 1;
            assert!(guard < 2000);
        }
        assert!((train.v - 3.0).abs() < 0.1, "left at {}", train.v);
    }

    // Test 10: the station.
    #[test]
    fn the_train_stops_at_the_stop_point_and_holds_for_the_dwell_time() {
        let mut course = course_with(Element::station(40.0), 28.0, 44.0);
        // A brake run before it, as in the demo.
        course.placed.push(Placed {
            s0: 10.0,
            s1: 28.0,
            element: Element::brake(),
        });
        let mut train = Train::free(3, 15.0, 9.0);
        let mut guard = 0;
        while !train.is_held() {
            train.step(&course, &Physics::default());
            guard += 1;
            assert!(guard < 2000, "never stopped");
        }
        assert!((train.s - 40.0).abs() < 0.2, "stopped at {}", train.s);
        assert_eq!(train.v, 0.0);
        // It waits for riders...
        for _ in 0..200 {
            train.step(&course, &Physics::default());
        }
        assert!(train.is_held() && train.s == 40.0);
        // ...then holds for the whole dwell after the countdown starts.
        train.start_countdown(5.0);
        for _ in 0..99 {
            train.step(&course, &Physics::default());
            assert!(train.is_held(), "left early");
        }
        for _ in 0..3 {
            train.step(&course, &Physics::default());
        }
        assert!(!train.is_held(), "still held after the dwell");
        // ...and is pushed out gently, up to the push speed.
        for _ in 0..4 {
            train.step(&course, &Physics::default());
        }
        assert!(train.v > 0.0 && train.v < 2.0 && train.s > 40.0);
        for _ in 0..40 {
            train.step(&course, &Physics::default());
        }
        assert!((train.v - 2.0).abs() < 0.05, "pushed to {}", train.v);
    }

    /// The demo course with `cars` cars, released from the station.
    fn demo_run(cars: usize) -> (Course, Train) {
        let course = demo::demo_course(DVec3::ZERO);
        let stop = course.station().unwrap().1;
        let mut train = Train::held(cars, stop);
        train.start_countdown(0.0);
        (course, train)
    }

    // The planner's realism fix: nothing jumps the speed.
    #[test]
    fn nothing_on_the_demo_lap_jolts_the_train() {
        let (course, mut train) = demo_run(3);
        let mut left = false;
        let mut worst_a: f64 = 0.0;
        for _ in 0..4000 {
            for _ in 0..SUBSTEPS {
                train.substep(&course, &Physics::default());
                worst_a = worst_a.max(train.a.abs());
            }
            if !train.is_held() {
                left = true;
            } else if left {
                break;
            }
        }
        assert!(left && train.is_held(), "the lap should end in the station");
        assert!(worst_a <= 15.0, "worst acceleration {worst_a} blocks/s^2");
    }

    // Test 6 for real (D13): over the lift crest the back car goes faster than the front car did.
    #[test]
    fn over_the_lift_crest_the_back_car_is_clearly_faster_than_the_front_car_was() {
        let (course, mut train) = demo_run(6);
        let lift = course
            .placed
            .iter()
            .find(|p| matches!(p.element, Element::Lift { .. }))
            .unwrap();
        // The crest is the highest point of the lift.
        let (mut crest, mut best, mut s) = (lift.s0, f64::MIN, lift.s0);
        while s < lift.s1 {
            let y = course.table.position(s).y;
            if y > best {
                best = y;
                crest = s;
            }
            s += 0.05;
        }
        let back = 5.0 * CAR_SPACING;
        let (mut v_front, mut v_back) = (None, None);
        for _ in 0..3000 {
            let before = train;
            train.step(&course, &Physics::default());
            if v_front.is_none() && before.s < crest && train.s >= crest {
                v_front = Some(train.v);
            }
            if v_back.is_none() && before.s - back < crest && train.s - back >= crest {
                v_back = Some(train.v);
            }
        }
        let (front, back) = (v_front.unwrap(), v_back.unwrap());
        assert!(
            (front - 5.0).abs() < 0.3,
            "front car crosses at chain speed: {front}"
        );
        assert!(back > front + 1.0, "front {front}, back {back}");
    }
}
