//! The per-tick loop (D21): `Runtime` is the one object that holds all of the mod's state.
//!
//! The game calls `Runtime::tick` once per server tick. A tick does, in order:
//!   1. say hello (once per world) and clean up leftovers from an earlier session;
//!   2. carry out orders typed by players (`/coaster demo`, `/coaster remove`);
//!   3. send the next batch of "build" commands (rails, cars, seats);
//!   4. see who sits in which seat, run the station countdown, and let the physics move the train;
//!   5. move the cars (every few ticks), the seats (every tick) and show riders their speed.
//!
//! It never touches the game directly: it asks a `Host` (see `host.rs`) to run command text
//! from `commands.rs`. So all of this is checked by `cargo test` with a `FakeHost`.

use crate::coaster::demo;
use crate::coaster::elements::{Course, DWELL};
use crate::coaster::physics::{Physics, Train};
use crate::coaster::sim;
use crate::commands;
use crate::host::{Host, PlayerInfo};
use crate::render;
use crate::welcome::Welcome;
use glam::DVec3;
use std::collections::VecDeque;

/// The usages the glue registers with the game (they all share the word `coaster`).
pub const COMMAND_USAGES: [&str; 3] = ["coaster demo", "coaster ride", "coaster remove"];

/// The most commands sent in one tick [D21]; the rest wait for the next tick.
pub const MAX_COMMANDS_PER_TICK: usize = 400;

/// A player counts as sitting in a seat when within this distance of its rider point (D49).
/// Generous on purpose: a cart that falls between our moves can shift the rider by a few tenths
/// of a block, and the train must never refuse to start.
pub const RIDER_ON_DISTANCE: f64 = 1.0;
/// A rider is "off" when nobody is within this distance of the rider point (D49, spike R7).
pub const RIDER_OFF_DISTANCE: f64 = 2.0;

/// The station's nearest end is 3 blocks east of the player (D52). The demo's first point is the
/// station's EAST end (the train runs west through it), so it is placed one station length
/// further east.
const DEMO_EAST: f64 = 3.0 + demo::STATION_LENGTH;
/// The rails float this high above the player's feet (D60, raised from 3 by D47's first value).
const DEMO_HEIGHT: f64 = 5.0;
/// Seats per car in the demo (D49).
const SEATS_PER_CAR: usize = 2;
/// Cars in the demo train (D55).
const CARS: usize = demo::CARS;
/// Seats in the whole train.
const SEATS: usize = CARS * SEATS_PER_CAR;
/// Riders see their speed (or the station countdown) every this many ticks (D57).
const ACTION_BAR_EVERY: u32 = 5;

/// Who typed a command, and where they stand and look.
#[derive(Clone, Debug, PartialEq)]
pub struct Caller {
    pub uuid: u128,
    pub pos: DVec3,
}

/// A job waiting for the next tick (commands can't run inside the command hook).
#[derive(Clone, Debug, PartialEq)]
enum Order {
    Demo(Caller),
    Remove,
}

/// The one demo ride that exists in the world.
#[derive(Clone, Debug)]
struct Demo {
    course: Course,
    physics: Physics,
    train: Train,
    /// The stone platform beside the station (D60), so it can be cleared again.
    platform: render::Platform,
    /// Ticks between moves of each car (D44), worked out once from a dry lap (S3).
    k: [u32; CARS],
    /// Ticks since the train last left the station (a car is sent when this divides by its `k`).
    phase: u32,
    /// Was the train standing in the station at the end of the last tick?
    was_held: bool,
    /// Counts ticks for the action bar.
    bar_clock: u32,
    /// Is somebody sitting in each seat, and who?
    occupied: [bool; SEATS],
    rider: [Option<u128>; SEATS],
    /// Where each seat was sent last (so we can tell who sits there).
    seat_pos: [DVec3; SEATS],
}

/// All of the mod's state.
#[derive(Clone, Debug)]
pub struct Runtime {
    welcome: Welcome,
    orders: VecDeque<Order>,
    /// True until the mod's leftover entities from an earlier session were removed (D20).
    cleanup_pending: bool,
    demo: Option<Demo>,
    /// Build commands that did not fit into earlier ticks.
    build_queue: VecDeque<String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime::new()
    }
}

/// The pose of seat `index` (0 .. SEATS) when the train's front car is at `front`.
fn seat_pose_at(course: &Course, train: &Train, front: f64, index: usize) -> render::Pose {
    let car = index / SEATS_PER_CAR;
    let car_s = front - car as f64 * train.spacing;
    render::seat_pose(&course.table, car_s, index % SEATS_PER_CAR)
}

impl Runtime {
    /// A fresh runtime. `const`, so the glue can keep one in a `static`.
    pub const fn new() -> Runtime {
        Runtime {
            welcome: Welcome::new(),
            orders: VecDeque::new(),
            cleanup_pending: true,
            demo: None,
            build_queue: VecDeque::new(),
        }
    }

    /// A new world opened: forget everything that belonged to the old one.
    pub fn world_opened(&mut self) {
        self.welcome.world_opened();
        self.orders.clear();
        self.build_queue.clear();
        self.demo = None;
        self.cleanup_pending = true;
    }

    /// A player typed `line` (without the slash). Returns the answer to show them, or `None`
    /// when this is not one of our commands. Real work is queued for the next tick.
    pub fn command(&mut self, line: &str, caller: Caller) -> Option<String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() != Some(&"coaster") {
            return None;
        }
        let answer = match words.get(1).copied() {
            Some("demo") if words.len() == 2 => {
                self.orders.push_back(Order::Demo(caller));
                "Building the demo coaster to your east."
            }
            Some("ride") if words.len() == 2 => {
                if self.demo.is_some() {
                    "Right-click a seat in the car to ride."
                } else {
                    "There is no coaster yet. Type /coaster demo first."
                }
            }
            Some("remove") if words.len() == 2 => {
                self.orders.push_back(Order::Remove);
                "Removing the demo coaster."
            }
            _ => "Usage: /coaster demo, /coaster ride or /coaster remove.",
        };
        Some(answer.to_string())
    }

    /// One server tick.
    pub fn tick<H: Host>(&mut self, host: &mut H) {
        // Reading the player list costs a little, so only do it when something needs it.
        let need_players = self.welcome.waiting() || self.cleanup_pending || self.demo.is_some();
        let players = if need_players {
            host.players()
        } else {
            Vec::new()
        };

        if let Some(line) = self.welcome.tick(!players.is_empty()) {
            host.broadcast(line);
        }
        if self.cleanup_pending && !players.is_empty() {
            // D20: nothing is saved yet, so anything of ours left in the world is stale.
            host.run(&commands::kill_all());
            self.cleanup_pending = false;
        }

        while let Some(order) = self.orders.pop_front() {
            match order {
                Order::Demo(caller) => self.build_demo(host, &caller),
                Order::Remove => {
                    host.run(&commands::kill_ride(commands::DEMO));
                    self.clear_platform(host);
                    self.build_queue.clear();
                    self.demo = None;
                }
            }
        }

        // Build commands first, keeping room for this tick's moves (every seat and every car)
        // and the action-bar fallbacks (at most one per seat).
        let room = MAX_COMMANDS_PER_TICK - (SEATS + CARS + SEATS);
        for _ in 0..room.min(self.build_queue.len()) {
            if let Some(command) = self.build_queue.pop_front() {
                host.run(&command);
            }
        }

        self.move_train(host, &players);
    }

    /// Remove the old demo's stone platform, and only those exact blocks (D60).
    fn clear_platform<H: Host>(&self, host: &mut H) {
        if let Some(demo) = &self.demo {
            host.run(&commands::fill(
                demo.platform.min,
                demo.platform.max,
                "minecraft:air",
            ));
        }
    }

    /// Turn a `/coaster demo` order into the list of build commands.
    fn build_demo<H: Host>(&mut self, host: &mut H, caller: &Caller) {
        host.run(&commands::kill_ride(commands::DEMO));
        self.clear_platform(host);
        self.build_queue.clear();

        // East of the player (+X), so the mod never has to read which way they look (D52).
        let origin = caller.pos + DVec3::new(DEMO_EAST, DEMO_HEIGHT, 0.0);
        let course = demo::demo_course(origin);
        let physics = Physics::default();
        let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
        let train = Train::held(CARS, stop);

        // S3: how often a car may be moved comes from one dry lap (the sharpest speed² × bend).
        let worst = sim::worst_speed_sq_curvature(&course, &train, &physics);
        let k = render::teleport_ticks_for(worst);

        // The station platform first (D60), then the rails, the cars and the seats.
        let platform = render::station_platform(&course.table, stop, CARS, train.spacing);
        self.build_queue.push_back(commands::fill(
            platform.min,
            platform.max,
            commands::PLATFORM_BLOCK,
        ));
        for piece in render::rail_pieces(&course.table) {
            let pose = render::rail_pose(&course.table, piece);
            self.build_queue.push_back(commands::summon_rail(
                commands::DEMO,
                pose.pos,
                pose.rotation,
                pose.scale,
            ));
        }
        for car in 0..CARS {
            let pose = render::car_pose(&course.table, stop - car as f64 * train.spacing);
            self.build_queue.push_back(commands::summon_car(
                commands::DEMO,
                car,
                pose.pos,
                pose.yaw,
                pose.pitch,
                k,
                render::CAR_SCALE,
            ));
        }
        let mut seat_pos = [DVec3::ZERO; SEATS];
        for (i, slot) in seat_pos.iter_mut().enumerate() {
            let pose = seat_pose_at(&course, &train, stop, i);
            *slot = pose.pos;
            self.build_queue
                .extend(commands::summon_seat(commands::DEMO, i, pose.pos));
        }

        self.demo = Some(Demo {
            course,
            physics,
            train,
            platform,
            k: [k; CARS],
            phase: 0,
            was_held: true,
            bar_clock: 0,
            occupied: [false; SEATS],
            rider: [None; SEATS],
            seat_pos,
        });
    }

    /// Steps 4 and 5 of the tick: who sits where, the station, the physics, the cars and seats.
    fn move_train<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo]) {
        let Some(demo) = self.demo.as_mut() else {
            return;
        };
        if demo.course.table.is_empty() {
            return;
        }

        // Who sits in which seat? Close = yes, far = no, in between = no change.
        for i in 0..SEATS {
            let point = render::rider_point(demo.seat_pos[i]);
            let nearest = players
                .iter()
                .min_by(|a, b| a.pos.distance(point).total_cmp(&b.pos.distance(point)));
            let distance = nearest.map_or(f64::INFINITY, |p| p.pos.distance(point));
            if distance <= RIDER_ON_DISTANCE {
                if let Some(p) = nearest {
                    if !demo.occupied[i] {
                        // Once per boarding: write down where the rider really is, so the heights
                        // can be checked in the log (D49: the cart's fall may move them).
                        let d = p.pos - demo.seat_pos[i];
                        host.log(&format!(
                            "Horizon Theme Park: seat {i} boarded, rider minus seat = {} {} {}",
                            commands::num(d.x),
                            commands::num(d.y),
                            commands::num(d.z)
                        ));
                    }
                    demo.rider[i] = Some(p.uuid);
                }
                demo.occupied[i] = true;
            } else if distance > RIDER_OFF_DISTANCE {
                demo.occupied[i] = false;
                demo.rider[i] = None;
            }
        }
        let anyone = demo.occupied.iter().any(|&o| o);

        // The station (D56): the countdown starts when someone sits, and stops if everyone leaves.
        // With nobody aboard the train waits.
        if demo.train.is_held() {
            if anyone {
                demo.train.start_countdown(DWELL);
            } else {
                demo.train.cancel_countdown();
            }
        }

        // The physics: one tick (ten substeps).
        demo.train.step(&demo.course, &demo.physics);
        let held = demo.train.is_held();
        if demo.was_held && !held {
            demo.phase = 0; // just left the station
        }
        let arrived = !demo.was_held && held;

        // Each car glides for `k` ticks to where it WILL be `k` ticks later (D32, D44), so it is
        // sent every `k` ticks. Physics is exactly repeatable (D12), so "where it will be" comes
        // from running a copy of the train `k` ticks forward. On arrival it is sent where it stands.
        for car in 0..CARS {
            let due = (!held && demo.phase % demo.k[car] == 0) || arrived;
            if !due {
                continue;
            }
            let mut ahead = demo.train;
            if !arrived {
                for _ in 0..demo.k[car] {
                    ahead.step(&demo.course, &demo.physics);
                }
            }
            let car_s = ahead.s - car as f64 * ahead.spacing;
            let pose = render::car_pose(&demo.course.table, car_s);
            host.run(&commands::move_car(
                commands::DEMO,
                car,
                pose.pos,
                pose.yaw,
                pose.pitch,
            ));
        }
        if !held {
            demo.phase += 1;
        }
        demo.was_held = held;

        // Seats are moved every tick, ridden or not, so an empty minecart never falls or drifts.
        for i in 0..SEATS {
            let pose = seat_pose_at(&demo.course, &demo.train, demo.train.s, i);
            demo.seat_pos[i] = pose.pos;
            host.run(&commands::move_seat(
                commands::DEMO,
                i,
                pose.pos,
                pose.yaw,
                pose.pitch,
            ));
        }

        // Riders (and only riders) see the speed, or the station countdown, four times a second.
        demo.bar_clock += 1;
        if demo.bar_clock % ACTION_BAR_EVERY == 0 {
            let text = match demo.train.countdown() {
                Some(left) => format!("Leaving in {} s", left.ceil() as i64),
                None if held => "Waiting for riders".to_string(),
                None => format!("Speed: {} km/h", (demo.train.v * 3.6).round() as i64),
            };
            let mut shown: Vec<u128> = Vec::new();
            for uuid in demo.rider.iter().flatten() {
                if shown.contains(uuid) {
                    continue;
                }
                shown.push(*uuid);
                if !host.action_bar(*uuid, &text)
                    && let Some(p) = players.iter().find(|p| p.uuid == *uuid)
                {
                    host.run(&commands::action_bar(&p.name, &text));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, PlayerInfo};

    fn player(uuid: u128, pos: DVec3) -> PlayerInfo {
        PlayerInfo {
            uuid,
            name: format!("Player{uuid}"),
            pos,
            dimension: "minecraft:overworld".to_string(),
        }
    }

    fn far_away() -> DVec3 {
        DVec3::new(500.0, 70.0, 500.0)
    }

    fn caller() -> Caller {
        Caller {
            uuid: 1,
            pos: DVec3::new(0.0, 70.0, 0.0),
        }
    }

    /// A runtime with a built demo, one player far away, and all build commands already sent.
    fn built() -> (Runtime, FakeHost) {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, far_away()));
        rt.tick(&mut host); // greeting + cleanup
        rt.command("coaster demo", caller());
        while rt.demo.is_none() || !rt.build_queue.is_empty() {
            rt.tick(&mut host);
        }
        host.take_commands();
        (rt, host)
    }

    fn demo(rt: &Runtime) -> &Demo {
        rt.demo.as_ref().unwrap()
    }

    fn seat_move_count(commands: &[String]) -> usize {
        commands
            .iter()
            .filter(|c| c.starts_with("tp @e[tag=htp_seat_demo_"))
            .count()
    }

    fn car_moves(commands: &[String]) -> Vec<&String> {
        commands
            .iter()
            .filter(|c| c.starts_with("tp @e[tag=htp_car_demo_"))
            .collect()
    }

    /// Put player 0 exactly where seat 0's rider sits.
    fn sit_in_seat_zero(rt: &Runtime, host: &mut FakeHost) {
        host.players[0].pos = render::rider_point(demo(rt).seat_pos[0]);
    }

    /// Tick with the player riding along in seat 0.
    fn ride_tick(rt: &mut Runtime, host: &mut FakeHost) {
        rt.tick(host);
        sit_in_seat_zero(rt, host);
    }

    #[test]
    fn the_first_tick_with_a_player_greets_and_cleans_up_once() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        rt.tick(&mut host); // nobody here: silence
        assert!(host.broadcasts.is_empty() && host.commands.is_empty());
        host.players.push(player(1, DVec3::ZERO));
        rt.tick(&mut host);
        rt.tick(&mut host);
        assert_eq!(host.broadcasts, vec!["Horizon Theme Park loaded"]);
        assert_eq!(host.commands, vec!["kill @e[tag=htp]"]);
    }

    #[test]
    fn a_new_world_greets_and_cleans_up_again() {
        let (mut rt, mut host) = built();
        rt.world_opened();
        assert!(rt.demo.is_none());
        rt.tick(&mut host);
        assert_eq!(host.broadcasts.len(), 2);
        assert_eq!(host.commands, vec!["kill @e[tag=htp]"]);
    }

    // Test 16: several cars.
    #[test]
    fn demo_builds_rails_then_three_cars_then_six_seats() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, far_away()));
        rt.tick(&mut host);
        host.take_commands();

        let answer = rt.command("coaster demo", caller());
        assert_eq!(
            answer.as_deref(),
            Some("Building the demo coaster to your east.")
        );
        assert!(
            host.commands.is_empty(),
            "nothing runs inside the command hook"
        );
        let mut per_tick = Vec::new();
        for _ in 0..8 {
            rt.tick(&mut host);
            per_tick.push(host.take_commands());
        }
        assert!(per_tick.iter().all(|t| t.len() <= MAX_COMMANDS_PER_TICK));
        let all: Vec<&String> = per_tick.iter().flatten().collect();

        assert_eq!(all[0], "kill @e[tag=htp_demo]");
        let summons: Vec<&&String> = all.iter().filter(|c| c.starts_with("summon")).collect();
        let rails = summons.iter().filter(|c| c.contains("htp:rail")).count();
        assert!(rails >= 100, "only {rails} rail pieces");
        // Rails first, then one car per car, then one minecart per seat.
        assert!(summons[..rails].iter().all(|c| c.contains("htp:rail")));
        for car in 0..CARS {
            assert!(
                summons[rails + car].contains(&format!("\"htp_car_demo_{car}\""))
                    && summons[rails + car].contains("teleport_duration:")
            );
        }
        for seat in 0..SEATS {
            let c = summons[rails + CARS + seat];
            assert!(
                c.starts_with("summon minecraft:minecart")
                    && c.contains(&format!("htp_seat_demo_{seat}"))
            );
        }
        assert_eq!(summons.len(), rails + CARS + SEATS);
    }

    #[test]
    fn the_station_is_three_blocks_east_of_the_player_and_the_rails_float_five_blocks_above_their_feet()
     {
        let (rt, _) = built();
        let table = &demo(&rt).course.table;
        // The player stands at (0, 70, 0). The station's near end is 3 blocks east.
        let (station, stop, _) = demo(&rt).course.station().unwrap();
        let near = table
            .position(station.s1)
            .x
            .min(table.position(station.s0).x);
        assert!((near - 3.0).abs() < 0.3, "station near end at x {near}");
        let at_stop = table.position(stop);
        assert!(
            (at_stop.y - 75.0).abs() < 0.01,
            "the station is 5 up: {}",
            at_stop.y
        );
        assert!(at_stop.z.abs() < 0.01);
        // Nothing dips into the ground (valley bottoms stay at least 0.5 block above the player's feet).
        let mut s = 0.0;
        while s < table.length() {
            assert!(table.position(s).y > 70.5, "low at s = {s}");
            s += 0.5;
        }
    }

    #[test]
    fn never_more_than_400_commands_a_tick() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, far_away()));
        rt.tick(&mut host);
        host.take_commands();
        for i in 0..1000 {
            rt.build_queue.push_back(format!("say {i}"));
        }
        let mut sent = 0;
        for _ in 0..4 {
            rt.tick(&mut host);
            let this_tick = host.take_commands();
            assert!(this_tick.len() <= MAX_COMMANDS_PER_TICK);
            sent += this_tick.len();
        }
        assert_eq!(sent, 1000);
        assert!(rt.build_queue.is_empty());
    }

    // Test 15: station flow.
    #[test]
    fn the_train_waits_while_empty_and_nobody_gets_a_countdown() {
        let (mut rt, mut host) = built();
        for _ in 0..200 {
            rt.tick(&mut host);
            let cmds = host.take_commands();
            assert_eq!(cmds.len(), SEATS, "only the seat moves");
        }
        let d = demo(&rt);
        assert!(d.train.is_held() && d.train.countdown().is_none());
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn the_countdown_starts_when_someone_sits_and_the_train_leaves_after_five_seconds() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        assert!(demo(&rt).train.countdown().is_some());
        let mut ticks = 1;
        while demo(&rt).train.is_held() {
            ride_tick(&mut rt, &mut host);
            ticks += 1;
            assert!(ticks < 300, "never left");
        }
        // 5 s is 100 ticks (the countdown starts in the first tick).
        assert!((99..=102).contains(&ticks), "left after {ticks} ticks");
    }

    #[test]
    fn leaving_the_seat_during_the_countdown_cancels_it() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        for _ in 0..30 {
            ride_tick(&mut rt, &mut host);
        }
        assert!(demo(&rt).train.countdown().is_some());
        host.players[0].pos = far_away();
        rt.tick(&mut host);
        assert!(demo(&rt).train.is_held());
        assert!(demo(&rt).train.countdown().is_none());
    }

    // Test 13: cars aimed k ticks ahead by running the physics forward.
    #[test]
    fn a_cars_move_goes_to_where_the_physics_puts_it_k_ticks_later() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        for _ in 0..140 {
            ride_tick(&mut rt, &mut host);
            host.take_commands();
        }
        // Moving now. Find the next tick on which car 0 is sent.
        for _ in 0..20 {
            ride_tick(&mut rt, &mut host);
            let cmds = host.take_commands();
            let sent = car_moves(&cmds);
            if let Some(first) = sent.first().filter(|c| c.contains("htp_car_demo_0,")) {
                let d = demo(&rt);
                let mut ahead = d.train;
                for _ in 0..d.k[0] {
                    ahead.step(&d.course, &d.physics);
                }
                let want = render::car_pose(&d.course.table, ahead.s);
                assert_eq!(
                    **first,
                    commands::move_car(commands::DEMO, 0, want.pos, want.yaw, want.pitch)
                );
                assert!(
                    d.k.iter()
                        .all(|k| (1..=render::MAX_TELEPORT_TICKS).contains(k))
                );
                return;
            }
        }
        panic!("car 0 was never sent");
    }

    #[test]
    fn every_seat_moves_every_tick_and_the_commands_stay_under_the_limit() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        for tick in 0..260 {
            ride_tick(&mut rt, &mut host);
            let cmds = host.take_commands();
            assert_eq!(seat_move_count(&cmds), SEATS, "tick {tick}");
            assert!(cmds.len() <= MAX_COMMANDS_PER_TICK);
        }
        assert!(!demo(&rt).train.is_held(), "it should be on its way by now");
    }

    // Test 14: the action bar.
    #[test]
    fn riders_and_only_riders_see_the_countdown_and_then_their_speed() {
        let (mut rt, mut host) = built();
        host.players.push(player(2, far_away())); // a bystander
        sit_in_seat_zero(&rt, &mut host);
        for _ in 0..400 {
            ride_tick(&mut rt, &mut host);
        }
        assert!(!host.action_bars.is_empty());
        assert!(
            host.action_bars.iter().all(|(who, _)| *who == 1),
            "only the rider gets lines: {:?}",
            host.action_bars
        );
        assert!(
            host.action_bars
                .iter()
                .any(|(_, t)| t.starts_with("Leaving in "))
        );
        let speeds: Vec<&String> = host
            .action_bars
            .iter()
            .map(|(_, t)| t)
            .filter(|t| t.starts_with("Speed: "))
            .collect();
        assert!(!speeds.is_empty());
        assert!(speeds.iter().all(|t| t.ends_with(" km/h")));
        // Every 5 ticks: 400 ticks give about 80 lines.
        assert!(
            (78..=81).contains(&host.action_bars.len()),
            "{}",
            host.action_bars.len()
        );
    }

    #[test]
    fn without_a_host_action_bar_the_title_command_is_used() {
        // A host that does not know action bars (the real glue, for now) gets the command text.
        struct Plain(FakeHost);
        impl Host for Plain {
            fn players(&self) -> Vec<PlayerInfo> {
                self.0.players()
            }
            fn run(&mut self, command: &str) -> bool {
                self.0.run(command)
            }
            fn broadcast(&mut self, text: &str) {
                self.0.broadcast(text)
            }
            fn tell(&mut self, player: u128, text: &str) {
                self.0.tell(player, text)
            }
        }
        let (mut rt, host) = built();
        let mut plain = Plain(host);
        plain.0.players[0].pos = render::rider_point(demo(&rt).seat_pos[0]);
        for _ in 0..10 {
            rt.tick(&mut plain);
            plain.0.players[0].pos = render::rider_point(demo(&rt).seat_pos[0]);
        }
        assert!(
            plain
                .0
                .commands
                .iter()
                .any(|c| c.starts_with("title Player1 actionbar {\"text\":\"Leaving in"))
        );
    }

    #[test]
    fn a_rider_who_stays_aboard_goes_a_whole_lap_and_the_train_stops_in_the_station_again() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        let mut left = false;
        for tick in 0..1500 {
            ride_tick(&mut rt, &mut host);
            let held = demo(&rt).train.is_held();
            if !held {
                left = true;
            }
            if left && held {
                // Back in the station at the stop point.
                let d = demo(&rt);
                let stop = d.course.station().unwrap().1;
                assert!((d.train.s - stop).abs() < 0.2);
                assert!(tick > 400, "lap took only {tick} ticks");
                return;
            }
        }
        panic!("the lap never ended");
    }

    #[test]
    fn boarding_is_logged_once_with_the_measured_offset_and_a_loose_fit_still_counts() {
        let (mut rt, mut host) = built();
        // A rider hanging 0.9 block off the expected spot still counts as sitting.
        let seat = demo(&rt).seat_pos[0];
        host.players[0].pos = render::rider_point(seat) + DVec3::new(0.0, -0.9, 0.0);
        for _ in 0..5 {
            rt.tick(&mut host);
            host.players[0].pos =
                render::rider_point(demo(&rt).seat_pos[0]) + DVec3::new(0.0, -0.9, 0.0);
        }
        assert!(demo(&rt).occupied[0]);
        assert_eq!(host.logs.len(), 1, "{:?}", host.logs);
        assert_eq!(
            host.logs[0],
            "Horizon Theme Park: seat 0 boarded, rider minus seat = 0.0000 -2.1125 0.0000"
        );
    }

    #[test]
    fn a_player_between_the_two_distances_changes_nothing() {
        let (mut rt, mut host) = built();
        // Standing 1.5 blocks from the rider point: not close enough to count as sitting.
        host.players[0].pos = render::rider_point(demo(&rt).seat_pos[0]) + DVec3::X * 1.5;
        rt.tick(&mut host);
        assert!(!demo(&rt).occupied[0]);
        // Sitting, then stepping 1 block away: still counts as sitting.
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        assert!(demo(&rt).occupied[0]);
        host.players[0].pos += DVec3::X * 1.0;
        rt.tick(&mut host);
        assert!(demo(&rt).occupied[0]);
        // Walking right away (more than 2 blocks): off.
        host.players[0].pos = far_away();
        rt.tick(&mut host);
        assert!(!demo(&rt).occupied[0]);
    }

    #[test]
    fn ride_is_only_a_hint_and_sends_no_commands() {
        let mut empty = Runtime::new();
        let mut host = FakeHost::new();
        let before = empty.command("coaster ride", caller());
        assert_eq!(
            before.as_deref(),
            Some("There is no coaster yet. Type /coaster demo first.")
        );
        let (mut rt, mut host2) = built();
        let after = rt.command("coaster ride", caller());
        assert_eq!(
            after.as_deref(),
            Some("Right-click a seat in the car to ride.")
        );
        rt.tick(&mut host2);
        assert_eq!(host2.take_commands().len(), SEATS, "only the seat moves");
        // And a runtime with no demo sends nothing at all.
        empty.tick(&mut host);
        assert!(host.commands.is_empty());
    }

    #[test]
    fn remove_kills_everything_of_the_demo_and_stops_the_moves() {
        let (mut rt, mut host) = built();
        assert!(rt.command("coaster remove", caller()).is_some());
        rt.tick(&mut host);
        let cmds = host.take_commands();
        assert_eq!(cmds[0], "kill @e[tag=htp_demo]");
        assert!(cmds[1].starts_with("fill ") && cmds[1].ends_with(" minecraft:air"));
        assert_eq!(cmds.len(), 2);
        assert!(rt.demo.is_none());
        rt.tick(&mut host);
        assert!(host.take_commands().is_empty());
    }

    #[test]
    fn other_commands_are_not_ours_and_bad_usage_gets_a_hint() {
        let mut rt = Runtime::new();
        assert_eq!(rt.command("gamemode creative", caller()), None);
        assert_eq!(rt.command("cooking", caller()), None);
        assert!(
            rt.command("coaster", caller())
                .unwrap()
                .starts_with("Usage:")
        );
        assert!(
            rt.command("coaster demo now", caller())
                .unwrap()
                .starts_with("Usage:")
        );
        assert!(rt.orders.is_empty());
        assert!(rt.command("  coaster   demo ", caller()).is_some());
        assert_eq!(rt.orders.len(), 1);
    }

    #[test]
    fn a_second_demo_replaces_the_first() {
        let (mut rt, mut host) = built();
        rt.command("coaster demo", caller());
        rt.tick(&mut host);
        assert_eq!(host.commands[0], "kill @e[tag=htp_demo]");
        // The old platform is cleared before the new one is placed (D60).
        assert!(host.commands[1].ends_with(" minecraft:air"));
        assert!(host.commands.iter().any(|c| c.contains("htp:rail")));
    }

    #[test]
    fn the_demo_places_a_smooth_stone_platform_first_and_remove_clears_exactly_those_blocks() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, far_away()));
        rt.tick(&mut host);
        host.take_commands();
        // The player stands at (0, 70, 0): the station track is at y 75, z 0, x -9.5 .. 3 east of them.
        rt.command("coaster demo", caller());
        let mut all = Vec::new();
        while rt.demo.is_none() || !rt.build_queue.is_empty() {
            rt.tick(&mut host);
            all.extend(host.take_commands());
        }
        let place: Vec<&String> = all.iter().filter(|c| c.starts_with("fill ")).collect();
        assert_eq!(place.len(), 1, "{place:?}");
        let platform = demo(&rt).platform;
        assert_eq!(
            *place[0],
            commands::fill(platform.min, platform.max, "minecraft:smooth_stone")
        );
        // Placed before the first rail.
        let at = |needle: &str| all.iter().position(|c| c.contains(needle)).unwrap();
        assert!(at("smooth_stone") < at("htp:rail"));
        // Two wide and as long as the train (3 cars + gaps = about 10 blocks).
        let width = platform.max[2] - platform.min[2] + 1;
        let length = platform.max[0] - platform.min[0] + 1;
        assert_eq!((width, platform.max[1] == platform.min[1]), (2, true));
        assert!((10..=12).contains(&length), "length {length}");

        rt.command("coaster remove", caller());
        rt.tick(&mut host);
        let cmds = host.take_commands();
        assert_eq!(
            cmds[1],
            commands::fill(platform.min, platform.max, "minecraft:air")
        );
    }

    #[test]
    fn the_platform_stays_clear_of_the_rails_and_the_cars_and_sits_on_the_outer_side() {
        let (rt, _) = built();
        let d = demo(&rt);
        let p = d.platform;
        let (x0, x1) = (p.min[0] as f64, p.max[0] as f64 + 1.0);
        let (z0, z1) = (p.min[2] as f64, p.max[2] as f64 + 1.0);
        let (y0, y1) = (p.min[1] as f64, p.min[1] as f64 + 1.0);
        let mut s = 0.0;
        while s < d.course.table.length() {
            let pos = d.course.table.position(s);
            // A rail is 0.5 wide and 0.15 tall; the car shell is 1.6 wide, from 0.05 above the track.
            let inside_flat =
                pos.x > x0 - 0.8 && pos.x < x1 + 0.8 && pos.z > z0 - 0.8 && pos.z < z1 + 0.8;
            let overlaps_height = pos.y - 0.075 < y1 && pos.y + 0.075 > y0;
            assert!(
                !(inside_flat && overlaps_height),
                "track at s = {s} touches the platform"
            );
            s += 0.1;
        }
        // Left of the direction of travel: the track runs west (-x) here, so left is +z (south).
        let stop = d.course.table.position(d.course.station().unwrap().1);
        assert!(z0 > stop.z, "the platform is on the far side from the turn");
        // The top is below the track's line.
        assert!(y1 < stop.y);
    }
}
