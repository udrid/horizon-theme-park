//! The per-tick loop (D21): `Runtime` is the one object that holds all of the mod's state.
//!
//! The game calls `Runtime::tick` once per server tick. A tick does, in order:
//!   1. say hello (once per world) and clean up leftovers from an earlier session;
//!   2. carry out orders typed by players (`/coaster demo`, `/coaster remove`);
//!   3. send the next batch of "build" commands (rails, car, seats);
//!   4. see who sits in which seat, then move the train;
//!   5. move the car (every few ticks) and the seats (every tick).
//!
//! It never touches the game directly: it asks a `Host` (see `host.rs`) to run command text
//! from `commands.rs`. So all of this is checked by `cargo test` with a `FakeHost`.

use crate::coaster::demo::{self, demo_nodes};
use crate::coaster::spline::Track;
use crate::coaster::table::Table;
use crate::commands;
use crate::host::Host;
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

/// The demo oval's middle is this far east of the player (3 blocks of space + half its length).
const DEMO_EAST: f64 = 3.0 + demo::STRAIGHT / 2.0 + demo::RADIUS;
/// The oval's long side runs east-west: Minecraft's yaw -90 looks towards +X (D52).
const DEMO_YAW: f64 = -90.0;
/// The rails float this high above the player's feet (D47).
const DEMO_HEIGHT: f64 = 3.0;
/// Seats per car in the demo (D49).
const SEATS: usize = 2;

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
    table: Table,
    /// Ticks between car moves (D44).
    k: u32,
    /// Distance of the car's middle along the track.
    s: f64,
    /// True while the train is going round.
    moving: bool,
    /// Ticks since the train last started or the car was last sent.
    phase: u32,
    /// Is somebody sitting in each seat?
    occupied: [bool; SEATS],
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
                    self.build_queue.clear();
                    self.demo = None;
                }
            }
        }

        // Build commands first, keeping room for this tick's moves.
        let room = MAX_COMMANDS_PER_TICK - (SEATS + 1);
        for _ in 0..room.min(self.build_queue.len()) {
            if let Some(command) = self.build_queue.pop_front() {
                host.run(&command);
            }
        }

        self.move_train(host, &players);
    }

    /// Turn a `/coaster demo` order into the list of build commands.
    fn build_demo<H: Host>(&mut self, host: &mut H, caller: &Caller) {
        host.run(&commands::kill_ride(commands::DEMO));
        self.build_queue.clear();

        // East of the player (+X), so the mod never has to read which way they look (D52).
        let center = caller.pos + DVec3::new(DEMO_EAST, DEMO_HEIGHT, 0.0);
        let table = Table::new(&Track::closed(&demo_nodes(center, DEMO_YAW)));
        let k = render::teleport_ticks(&table, demo::SPEED);

        // Rails, then the car, then the seats.
        for piece in render::rail_pieces(&table) {
            let pose = render::rail_pose(&table, piece);
            self.build_queue.push_back(commands::summon_rail(
                commands::DEMO,
                pose.pos,
                pose.rotation,
                pose.scale,
            ));
        }
        let car = render::car_pose(&table, 0.0);
        self.build_queue.push_back(commands::summon_car(
            commands::DEMO,
            0,
            car.pos,
            car.yaw,
            car.pitch,
            k,
            render::CAR_SCALE,
        ));
        let mut seat_pos = [DVec3::ZERO; SEATS];
        for (i, slot) in seat_pos.iter_mut().enumerate() {
            let pose = render::seat_pose(&table, 0.0, i);
            *slot = pose.pos;
            self.build_queue
                .extend(commands::summon_seat(commands::DEMO, i, pose.pos));
        }

        self.demo = Some(Demo {
            table,
            k,
            s: 0.0,
            moving: false,
            phase: 0,
            occupied: [false; SEATS],
            seat_pos,
        });
    }

    /// Steps 4 and 5 of the tick: who sits where, the train, the car and the seats.
    fn move_train<H: Host>(&mut self, host: &mut H, players: &[crate::host::PlayerInfo]) {
        let Some(demo) = self.demo.as_mut() else {
            return;
        };
        let length = demo.table.length();
        if length <= 0.0 {
            return;
        }

        // Who sits in which seat? Close = yes, far = no, in between = no change.
        for i in 0..SEATS {
            let point = render::rider_point(demo.seat_pos[i]);
            let nearest = players
                .iter()
                .map(|p| p.pos.distance(point))
                .fold(f64::INFINITY, f64::min);
            if nearest <= RIDER_ON_DISTANCE {
                if !demo.occupied[i] {
                    // Once per boarding: write down where the rider really is, so the heights can
                    // be checked in the log (D49: the cart's fall may move them).
                    if let Some(p) = players
                        .iter()
                        .min_by(|a, b| a.pos.distance(point).total_cmp(&b.pos.distance(point)))
                    {
                        let d = p.pos - demo.seat_pos[i];
                        host.log(&format!(
                            "Horizon Theme Park: seat {i} boarded, rider minus seat = {} {} {}",
                            commands::num(d.x),
                            commands::num(d.y),
                            commands::num(d.z)
                        ));
                    }
                }
                demo.occupied[i] = true;
            } else if nearest > RIDER_OFF_DISTANCE {
                demo.occupied[i] = false;
            }
        }
        let anyone = demo.occupied.iter().any(|&o| o);

        // The train waits at the start while empty, runs while anyone rides, and when the last
        // rider is off it finishes the lap and stops at the start (D49).
        let step = demo::SPEED * render::TICK_SECONDS;
        let mut stopped_now = false;
        if anyone && !demo.moving {
            demo.moving = true;
            demo.phase = 0;
        }
        if demo.moving {
            demo.s += step;
            if demo.s >= length && !anyone {
                demo.s = 0.0;
                demo.moving = false;
                stopped_now = true;
            } else if demo.s >= length {
                demo.s -= length;
            }
        }

        // The car glides for `k` ticks to where it will be `k` ticks later, so it is sent every
        // `k` ticks. When the train is about to stop it is aimed at the start, not past it.
        let car_due = (demo.moving && demo.phase % demo.k == 0) || stopped_now;
        if car_due {
            let aim = if stopped_now {
                length
            } else if anyone {
                demo.s + step * demo.k as f64
            } else {
                (demo.s + step * demo.k as f64).min(length)
            };
            let pose = render::car_pose(&demo.table, aim);
            host.run(&commands::move_car(
                commands::DEMO,
                0,
                pose.pos,
                pose.yaw,
                pose.pitch,
            ));
        }
        if demo.moving {
            demo.phase += 1;
        }

        // Seats are moved every tick, ridden or not, so an empty minecart never falls or drifts.
        for i in 0..SEATS {
            let pose = render::seat_pose(&demo.table, demo.s, i);
            demo.seat_pos[i] = pose.pos;
            host.run(&commands::move_seat(
                commands::DEMO,
                i,
                pose.pos,
                pose.yaw,
                pose.pitch,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, PlayerInfo};

    fn player(pos: DVec3) -> PlayerInfo {
        PlayerInfo {
            uuid: 1,
            name: "Alex".to_string(),
            pos,
            dimension: "minecraft:overworld".to_string(),
        }
    }

    fn caller() -> Caller {
        Caller {
            uuid: 1,
            pos: DVec3::new(0.0, 70.0, 0.0),
        }
    }

    /// A runtime with a built demo, one player far away, and the build commands already sent.
    fn built() -> (Runtime, FakeHost) {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(DVec3::new(500.0, 70.0, 500.0)));
        rt.tick(&mut host); // greeting + cleanup
        rt.command("coaster demo", caller());
        rt.tick(&mut host);
        host.take_commands();
        (rt, host)
    }

    fn seat_move_count(commands: &[String]) -> usize {
        commands
            .iter()
            .filter(|c| c.starts_with("tp @e[tag=htp_seat_demo_"))
            .count()
    }

    /// Put the player exactly where seat 0's rider sits.
    fn sit_in_seat_zero(rt: &Runtime, host: &mut FakeHost) {
        let point = render::rider_point(rt.demo.as_ref().unwrap().seat_pos[0]);
        host.players[0].pos = point;
    }

    #[test]
    fn the_first_tick_with_a_player_greets_and_cleans_up_once() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        rt.tick(&mut host); // nobody here: silence
        assert!(host.broadcasts.is_empty() && host.commands.is_empty());
        host.players.push(player(DVec3::ZERO));
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

    #[test]
    fn demo_builds_rails_then_the_car_then_the_seats() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(DVec3::new(500.0, 70.0, 500.0)));
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
        rt.tick(&mut host);

        let cmds = &host.commands;
        assert!(cmds.len() <= MAX_COMMANDS_PER_TICK);
        assert_eq!(cmds[0], "kill @e[tag=htp_demo]");
        let rails = cmds.iter().take_while(|c| !c.contains("htp:car")).count() - 1;
        assert!(rails >= 15, "only {rails} rail pieces");
        assert!(cmds[1..=rails].iter().all(|c| c.contains("htp:rail")));
        assert!(
            cmds[rails + 1].contains("\"htp_car_demo_0\"")
                && cmds[rails + 1].contains("teleport_duration:")
        );
        assert!(
            cmds[rails + 2].starts_with("summon minecraft:minecart")
                && cmds[rails + 2].contains("htp_seat_demo_0")
        );
        assert!(
            cmds[rails + 3].starts_with("summon minecraft:minecart")
                && cmds[rails + 3].contains("htp_seat_demo_1")
        );
        // Then this tick's two seat moves (the train waits, so no car move).
        assert_eq!(cmds.len(), rails + 4 + 2);
        assert_eq!(seat_move_count(&cmds[rails + 4..]), 2);
    }

    #[test]
    fn the_rails_are_placed_to_the_east_of_the_player_and_above_their_feet() {
        let (rt, _) = built();
        let table = &rt.demo.as_ref().unwrap().table;
        // The player stands at (0, 70, 0). The oval spans x from 3 to 26 (3 blocks of space, then
        // 23 blocks long), is 12 wide in z, and floats 3 blocks up.
        let mut s = 0.0;
        while s < table.length() {
            let p = table.position(s);
            assert!((p.y - 73.0).abs() < 1e-9);
            assert!(p.x > 2.9 && p.x < 26.1, "x {}", p.x);
            assert!(p.z.abs() < 6.1, "z {}", p.z);
            s += 0.5;
        }
    }

    #[test]
    fn never_more_than_400_commands_a_tick() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(DVec3::new(500.0, 70.0, 500.0)));
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

    #[test]
    fn with_a_rider_every_tick_moves_both_seats_and_car_ticks_send_the_car_first() {
        let (mut rt, mut host) = built();
        let k = rt.demo.as_ref().unwrap().k;
        assert!((1..=render::MAX_TELEPORT_TICKS).contains(&k));
        sit_in_seat_zero(&rt, &mut host);

        for tick in 0..7u32 {
            rt.tick(&mut host);
            let cmds = host.take_commands();
            assert_eq!(seat_move_count(&cmds), 2, "tick {tick}");
            if tick % k == 0 {
                assert!(
                    cmds[0].starts_with("tp @e[tag=htp_car_demo_0,limit=1]"),
                    "tick {tick}"
                );
                assert_eq!(cmds.len(), 3);
            } else {
                assert_eq!(cmds.len(), 2, "tick {tick}");
            }
            sit_in_seat_zero(&rt, &mut host); // the rider goes along with the seat
        }
    }

    #[test]
    fn the_car_is_aimed_k_ticks_ahead_of_the_train() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        let cmds = host.take_commands();
        // After the first step the train is one step along; the car glides to where it will be
        // k ticks later.
        let table = rt.demo.as_ref().unwrap().table.clone();
        let k = rt.demo.as_ref().unwrap().k as f64;
        let step = demo::SPEED * render::TICK_SECONDS;
        let want = render::car_pose(&table, step + step * k);
        assert_eq!(
            cmds[0],
            commands::move_car(commands::DEMO, 0, want.pos, want.yaw, want.pitch)
        );
    }

    #[test]
    fn the_train_waits_while_empty() {
        let (mut rt, mut host) = built();
        for _ in 0..40 {
            rt.tick(&mut host);
            let cmds = host.take_commands();
            assert_eq!(cmds.len(), 2, "only the two seat moves");
        }
        let demo = rt.demo.as_ref().unwrap();
        assert_eq!(demo.s, 0.0);
        assert!(!demo.moving);
    }

    #[test]
    fn a_player_between_the_two_distances_changes_nothing() {
        let (mut rt, mut host) = built();
        // Standing 1.5 blocks from the rider point: not close enough to count as sitting.
        host.players[0].pos =
            render::rider_point(rt.demo.as_ref().unwrap().seat_pos[0]) + DVec3::X * 1.5;
        rt.tick(&mut host);
        assert!(!rt.demo.as_ref().unwrap().occupied[0]);
        // Sitting, then stepping 1 block away: still counts as sitting.
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        assert!(rt.demo.as_ref().unwrap().occupied[0]);
        host.players[0].pos += DVec3::X * 1.0;
        rt.tick(&mut host);
        assert!(rt.demo.as_ref().unwrap().occupied[0]);
        // Walking right away (more than 2 blocks): off.
        host.players[0].pos = DVec3::new(500.0, 70.0, 500.0);
        rt.tick(&mut host);
        assert!(!rt.demo.as_ref().unwrap().occupied[0]);
    }

    #[test]
    fn boarding_is_logged_once_with_the_measured_offset_and_a_loose_fit_still_counts() {
        let (mut rt, mut host) = built();
        // A rider hanging 0.9 block off the expected spot still counts as sitting.
        let seat = rt.demo.as_ref().unwrap().seat_pos[0];
        host.players[0].pos = render::rider_point(seat) + DVec3::new(0.0, -0.9, 0.0);
        for _ in 0..5 {
            rt.tick(&mut host);
            host.players[0].pos = render::rider_point(rt.demo.as_ref().unwrap().seat_pos[0])
                + DVec3::new(0.0, -0.9, 0.0);
        }
        assert!(rt.demo.as_ref().unwrap().moving);
        assert_eq!(host.logs.len(), 1, "{:?}", host.logs);
        assert_eq!(
            host.logs[0],
            "Horizon Theme Park: seat 0 boarded, rider minus seat = 0.0000 -2.1125 0.0000"
        );
    }

    #[test]
    fn when_the_rider_gets_off_the_train_finishes_the_lap_and_stops_at_the_start() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        for _ in 0..10 {
            rt.tick(&mut host);
            sit_in_seat_zero(&rt, &mut host);
        }
        assert!(rt.demo.as_ref().unwrap().moving);
        host.take_commands();

        // The rider leaves.
        host.players[0].pos = DVec3::new(500.0, 70.0, 500.0);
        let mut ticks = 0;
        while rt.demo.as_ref().unwrap().moving {
            rt.tick(&mut host);
            ticks += 1;
            assert!(ticks < 400, "the train never stopped");
        }
        // The lap is about 59.7 blocks, one step per tick: about length / step ticks in all.
        let lap = (rt.demo.as_ref().unwrap().table.length() / (demo::SPEED * render::TICK_SECONDS))
            as i32;
        assert!((ticks - lap).abs() < 12, "ticks {ticks}, lap {lap}");
        assert_eq!(rt.demo.as_ref().unwrap().s, 0.0);

        // The final car move sent it to the start, and afterwards only seat moves remain.
        let table = rt.demo.as_ref().unwrap().table.clone();
        let start = render::car_pose(&table, table.length());
        let last_car = host
            .commands
            .iter()
            .rev()
            .find(|c| c.starts_with("tp @e[tag=htp_car_demo_0"))
            .unwrap()
            .clone();
        assert_eq!(
            last_car,
            commands::move_car(commands::DEMO, 0, start.pos, start.yaw, start.pitch)
        );
        host.take_commands();
        rt.tick(&mut host);
        assert_eq!(host.take_commands().len(), 2);
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
        assert_eq!(host2.take_commands().len(), 2, "only the seat moves");
        // And a runtime with no demo sends nothing at all.
        empty.tick(&mut host);
        assert!(host.commands.is_empty());
    }

    #[test]
    fn remove_kills_everything_of_the_demo_and_stops_the_moves() {
        let (mut rt, mut host) = built();
        assert!(rt.command("coaster remove", caller()).is_some());
        rt.tick(&mut host);
        assert_eq!(host.take_commands(), vec!["kill @e[tag=htp_demo]"]);
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
        assert!(host.commands.iter().any(|c| c.contains("htp:car")));
    }
}
