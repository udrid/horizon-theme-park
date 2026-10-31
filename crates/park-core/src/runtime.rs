//! The per-tick loop (D21): `Runtime` is the one object that holds all of the mod's state.
//!
//! The game calls `Runtime::tick` once per server tick. A tick does, in order:
//!   1. say hello (once per world), clean up leftovers of an earlier session, and load the saved
//!      rides from the world's folder (D20);
//!   2. carry out orders (typed commands, and wand clicks queued by the item hooks);
//!   3. send waiting build commands (platform, node markers);
//!   4. for every ride: see who sits where, run the station countdown, and let the physics move the train;
//!   5. park and unpark (D30): remove the displays of track far from every player, and draw them
//!      again when someone comes close (at most 50 pieces a tick);
//!   6. move the cars (every few ticks) and the seats (every tick) of the rides near players;
//!   7. show wand markers and the build preview, and sweep up leftovers every 5 seconds (D66);
//!   8. save the rides that changed.
//!
//! It never touches the game directly: it asks a `Host` (see `host.rs`) to run command text
//! from `commands.rs`. So all of this is checked by `cargo test` with a `FakeHost`.

use crate::builder::{self, Builder, PREVIEW_TICKS};
use crate::coaster::demo;
use crate::coaster::elements::{Course, DWELL};
use crate::coaster::physics::{MAX_CARS, Physics, Train};
use crate::coaster::sim;
use crate::commands;
use crate::host::{Host, PlayerInfo};
use crate::render;
use crate::report;
use crate::save::{self, Kind, ParkFile, RideFile};
use crate::welcome::Welcome;
use glam::DVec3;
use std::collections::VecDeque;
use std::path::PathBuf;

/// The usages the glue registers with the game (they all share the word `coaster`).
pub const COMMAND_USAGES: [&str; 11] = [
    "coaster demo",
    "coaster ride",
    "coaster remove",
    "coaster wand",
    "coaster undo",
    "coaster list",
    "coaster create <name:word>",
    "coaster edit <name:word>",
    "coaster delete <name:word>",
    "coaster set <name:word> cars <count:integer>",
    "coaster help",
];

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
/// Seats per car (D49).
const SEATS_PER_CAR: usize = 2;
/// Riders see their speed (or the station countdown) every this many ticks (D57).
const ACTION_BAR_EVERY: u32 = 5;

/// Rail pieces per parking group (D30).
pub const GROUP_SIZE: usize = 16;
/// A group is removed when no player is within this many blocks (D30).
pub const PARK_DISTANCE: f64 = 96.0;
/// A group is drawn again when a player comes within this many blocks (D30).
pub const DRAW_DISTANCE: f64 = 80.0;
/// At most this many pieces are drawn in one tick (D30).
pub const DRAWS_PER_TICK: usize = 50;
/// The display budget (D27) is counted within this distance of each player...
pub const BUDGET_RADIUS: f64 = 160.0;
/// ...and may not pass this many displays.
pub const DISPLAY_BUDGET: usize = 1500;
/// Leftovers are swept up every this many ticks (5 seconds, D66).
const SCRUB_EVERY: u64 = 100;
/// How long to wait for the world folder to show up before giving up on saving (F5), in ticks.
const WORLD_WAIT_TICKS: u64 = 40;
/// A `delete` must be repeated within this many ticks (10 seconds, S3).
const DELETE_WINDOW: u64 = 200;
/// The preview: a dot every 0.5 block, redrawn every 10 ticks, at most this many dots (S2).
const PREVIEW_STEP: f64 = 0.5;
const PREVIEW_EVERY: u64 = 10;
const PREVIEW_MAX_POINTS: usize = 400;
/// Wand node markers summoned in one tick at most.
const MARKERS_PER_TICK: usize = 50;

/// Who typed a command, and where they stand.
#[derive(Clone, Debug, PartialEq)]
pub struct Caller {
    pub uuid: u128,
    pub name: String,
    pub pos: DVec3,
}

/// A click with the track wand, as the item hooks hear it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WandClick {
    /// Right-click on a block: the block's position.
    Block([i32; 3]),
    /// Right-click on the air.
    Air,
}

/// A job waiting for the next tick (commands can't run inside the command or item hooks).
#[derive(Clone, Debug, PartialEq)]
enum Order {
    Demo(Caller),
    RemoveDemo,
    Give(Caller),
    Wand { uuid: u128, click: WandClick },
    Create { uuid: u128, ride: RideFile },
    Delete { uuid: u128, id: String },
    SetCars { uuid: u128, id: String, cars: usize },
}

/// One seat of a train.
#[derive(Clone, Debug)]
struct Seat {
    occupied: bool,
    rider: Option<u128>,
    /// Where the seat was sent last (so we can tell who sits there).
    pos: DVec3,
}

/// About 16 rail pieces that are drawn and parked together (D30).
#[derive(Clone, Debug)]
struct Group {
    /// The pieces `first..end` of the ride.
    first: usize,
    end: usize,
    center: DVec3,
    radius: f64,
    drawn: bool,
    /// How many times it was drawn; the number is in the entities' tags (D30).
    generation: u32,
}

/// One ride in the world: its track, its train, and what is drawn of it.
#[derive(Clone, Debug)]
struct Ride {
    id: String,
    /// What is saved. `None` for the demo, which is not saved.
    file: Option<RideFile>,
    course: Course,
    physics: Physics,
    train: Train,
    cars: usize,
    /// Seconds the train waits after someone sits (D56).
    dwell: f64,
    /// Ticks between moves of each car (D44), worked out once from a dry lap (S3).
    k: u32,
    pieces: Vec<render::Piece>,
    groups: Vec<Group>,
    /// Are the cars and seats in the world right now?
    train_drawn: bool,
    train_gen: u32,
    /// Set in the tick the train was drawn: it is already where it should be, so no move is sent.
    just_drawn: bool,
    /// The stone platform beside the station (D60), for the demo only.
    platform: Option<render::Platform>,
    /// Ticks since the train last left the station (a car is sent when this divides by `k`).
    phase: u32,
    was_held: bool,
    bar_clock: u32,
    seats: Vec<Seat>,
    /// The car moves worked out for this tick.
    car_moves: Vec<Option<render::Pose>>,
    /// Needs saving.
    dirty: bool,
}

impl Ride {
    fn new(
        id: String,
        file: Option<RideFile>,
        course: Course,
        physics: Physics,
        train: Train,
        dwell: f64,
    ) -> Ride {
        // S3: how often a car may be moved comes from one dry lap (the sharpest speed² × bend).
        let worst = sim::worst_speed_sq_curvature(&course, &train, &physics);
        let k = render::teleport_ticks_for(worst);
        let pieces = render::rail_pieces(&course.table);
        let centres: Vec<DVec3> = pieces
            .iter()
            .map(|&p| render::rail_pose(&course.table, p).pos)
            .collect();
        let groups = centres
            .chunks(GROUP_SIZE)
            .enumerate()
            .map(|(i, chunk)| {
                let sum = chunk.iter().fold(DVec3::ZERO, |a, &b| a + b);
                let center = sum / chunk.len() as f64;
                let radius = chunk.iter().map(|p| p.distance(center)).fold(0.0, f64::max);
                Group {
                    first: i * GROUP_SIZE,
                    end: i * GROUP_SIZE + chunk.len(),
                    center,
                    radius,
                    drawn: false,
                    generation: 0,
                }
            })
            .collect();
        let cars = train.cars;
        Ride {
            id,
            file,
            course,
            physics,
            train,
            cars,
            dwell,
            k,
            pieces,
            groups,
            train_drawn: false,
            train_gen: 0,
            just_drawn: false,
            platform: None,
            phase: 0,
            was_held: true,
            bar_clock: 0,
            seats: vec![
                Seat {
                    occupied: false,
                    rider: None,
                    pos: DVec3::ZERO,
                };
                cars * SEATS_PER_CAR
            ],
            car_moves: vec![None; cars],
            dirty: false,
        }
    }

    /// A ride from its saved file, or the reason it can't be built.
    fn from_file(file: RideFile) -> Result<Ride, String> {
        let course = builder::course_from(&file)?;
        let physics = builder::physics_of(&file);
        let train = builder::train_of(&file, &course);
        let dwell = file.settings.dwell;
        Ok(Ride::new(
            file.id.clone(),
            Some(file),
            course,
            physics,
            train,
            dwell,
        ))
    }

    /// The pose of seat `index` when the front car is at `front`.
    fn seat_pose_at(&self, front: f64, index: usize) -> render::Pose {
        let car = index / SEATS_PER_CAR;
        let car_s = front - car as f64 * self.train.spacing;
        render::seat_pose(&self.course.table, car_s, index % SEATS_PER_CAR)
    }

    fn front_pos(&self) -> DVec3 {
        self.course.table.position(self.train.s)
    }

    /// Everything drawn of this ride, in the world right now?
    fn anything_drawn(&self) -> bool {
        self.train_drawn || self.groups.iter().any(|g| g.drawn)
    }

    /// The summon commands for rail group `g`; counts as a new drawing (D30).
    fn rail_commands(&mut self, g: usize, session: u64) -> Vec<String> {
        self.groups[g].generation += 1;
        let group = &self.groups[g];
        let tags = commands::piece_tags(&self.id, &g.to_string(), group.generation, session);
        (group.first..group.end)
            .map(|i| {
                let pose = render::rail_pose(&self.course.table, self.pieces[i]);
                commands::summon_rail(&self.id, &tags, pose.pos, pose.rotation, pose.scale)
            })
            .collect()
    }

    /// The summon commands for the cars and seats, where the train stands now.
    fn train_commands(&mut self, session: u64) -> Vec<String> {
        self.train_gen += 1;
        let tags = commands::piece_tags(&self.id, "t", self.train_gen, session);
        let mut out = Vec::new();
        for car in 0..self.cars {
            let pose = render::car_pose(
                &self.course.table,
                self.train.s - car as f64 * self.train.spacing,
            );
            out.push(commands::summon_car(
                &self.id,
                car,
                &tags,
                pose.pos,
                pose.yaw,
                pose.pitch,
                self.k,
                render::CAR_SCALE,
            ));
        }
        for i in 0..self.seats.len() {
            let pose = self.seat_pose_at(self.train.s, i);
            self.seats[i] = Seat {
                occupied: false,
                rider: None,
                pos: pose.pos,
            };
            out.extend(commands::summon_seat(&self.id, i, &tags, pose.pos));
        }
        self.train_drawn = true;
        self.just_drawn = true;
        out
    }

    /// Who sits where, the station, and the physics for one tick (steps 4 of the tick).
    fn step<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo]) {
        if self.course.table.is_empty() {
            return;
        }

        // Who sits in which seat? Close = yes, far = no, in between = no change.
        if self.train_drawn {
            for i in 0..self.seats.len() {
                let point = render::rider_point(self.seats[i].pos);
                let nearest = players
                    .iter()
                    .min_by(|a, b| a.pos.distance(point).total_cmp(&b.pos.distance(point)));
                let distance = nearest.map_or(f64::INFINITY, |p| p.pos.distance(point));
                if distance <= RIDER_ON_DISTANCE {
                    if let Some(p) = nearest {
                        if !self.seats[i].occupied {
                            // Once per boarding: write down where the rider really is, so the heights
                            // can be checked in the log (D49: the cart's fall may move them).
                            let d = p.pos - self.seats[i].pos;
                            host.log(&format!(
                                "Horizon Theme Park: seat {i} boarded, rider minus seat = {} {} {}",
                                commands::num(d.x),
                                commands::num(d.y),
                                commands::num(d.z)
                            ));
                        }
                        self.seats[i].rider = Some(p.uuid);
                    }
                    self.seats[i].occupied = true;
                } else if distance > RIDER_OFF_DISTANCE {
                    self.seats[i].occupied = false;
                    self.seats[i].rider = None;
                }
            }
        }
        let anyone = self.seats.iter().any(|s| s.occupied);

        // The station (D56): the countdown starts when someone sits, and stops if everyone leaves.
        // With nobody aboard the train waits.
        if self.train.is_held() {
            if anyone {
                self.train.start_countdown(self.dwell);
            } else {
                self.train.cancel_countdown();
            }
        }

        // The physics: one tick (ten substeps). It runs even while the track is parked (D30).
        self.train.step(&self.course, &self.physics);
        let held = self.train.is_held();
        if self.was_held && !held {
            self.phase = 0; // just left the station
        }
        let arrived = !self.was_held && held;

        // Each car glides for `k` ticks to where it WILL be `k` ticks later (D32, D44), so it is
        // sent every `k` ticks. Physics is exactly repeatable (D12), so "where it will be" comes
        // from running a copy of the train `k` ticks forward. On arrival it is sent where it stands.
        for slot in self.car_moves.iter_mut() {
            *slot = None;
        }
        let due = (!held && self.phase.is_multiple_of(self.k)) || arrived;
        if self.train_drawn && due {
            let mut ahead = self.train;
            if !arrived {
                for _ in 0..self.k {
                    ahead.step(&self.course, &self.physics);
                }
            }
            for car in 0..self.cars {
                let car_s = ahead.s - car as f64 * ahead.spacing;
                self.car_moves[car] = Some(render::car_pose(&self.course.table, car_s));
            }
        }
        if !held {
            self.phase += 1;
        }
        self.was_held = held;

        // Riders (and only riders) see the speed, or the station countdown, four times a second.
        if self.train_drawn {
            self.bar_clock += 1;
            if self.bar_clock.is_multiple_of(ACTION_BAR_EVERY) {
                let text = match self.train.countdown() {
                    Some(left) => format!("Leaving in {} s", left.ceil() as i64),
                    None if held => "Waiting for riders".to_string(),
                    None => format!("Speed: {} km/h", (self.train.v * 3.6).round() as i64),
                };
                let mut shown: Vec<u128> = Vec::new();
                for seat in &self.seats {
                    let Some(uuid) = seat.rider else { continue };
                    if shown.contains(&uuid) {
                        continue;
                    }
                    shown.push(uuid);
                    show(host, players, uuid, &text);
                }
            }
        }
    }

    /// Send this tick's car and seat moves (step 6). Nothing is sent for a train that is not drawn.
    fn emit_moves<H: Host>(&mut self, host: &mut H) {
        if !self.train_drawn {
            return;
        }
        if self.just_drawn {
            self.just_drawn = false;
            return;
        }
        for car in 0..self.cars {
            if let Some(pose) = self.car_moves[car] {
                host.run(&commands::move_car(
                    &self.id, car, pose.pos, pose.yaw, pose.pitch,
                ));
            }
        }
        // Seats are moved every tick, ridden or not, so an empty minecart never falls or drifts.
        for i in 0..self.seats.len() {
            let pose = self.seat_pose_at(self.train.s, i);
            self.seats[i].pos = pose.pos;
            host.run(&commands::move_seat(
                &self.id, i, pose.pos, pose.yaw, pose.pitch,
            ));
        }
    }

    /// How many commands this ride sends in a tick when it is drawn (an upper bound).
    fn move_cost(&self) -> usize {
        if self.train_drawn {
            self.cars + self.seats.len()
        } else {
            0
        }
    }
}

/// Show `text` on one player's action bar, or by command when the host has no direct way.
fn show<H: Host>(host: &mut H, players: &[PlayerInfo], uuid: u128, text: &str) {
    if !host.action_bar(uuid, text)
        && let Some(p) = players.iter().find(|p| p.uuid == uuid)
    {
        host.run(&commands::action_bar(&p.name, text));
    }
}

/// The smallest distance from `at` to any of `points`, or infinity when there are none.
fn nearest(points: &[DVec3], at: DVec3) -> f64 {
    points
        .iter()
        .map(|p| p.distance(at))
        .fold(f64::INFINITY, f64::min)
}

/// All of the mod's state.
#[derive(Clone, Debug)]
pub struct Runtime {
    welcome: Welcome,
    orders: VecDeque<Order>,
    /// True until the mod's leftover entities from an earlier session were removed (D20).
    cleanup_pending: bool,
    /// Build commands that did not fit into earlier ticks.
    build_queue: VecDeque<String>,
    /// Have the saved rides been loaded for this world?
    loaded: bool,
    /// Ticks spent waiting for the world folder.
    waited: u64,
    /// The world folder, when there is one: only then are rides saved.
    world: Option<PathBuf>,
    /// The session number in the tag `htp_s<n>` (D66).
    session: u64,
    rides: Vec<Ride>,
    /// Ride files that exist but could not be used (newer, broken): never overwritten.
    locked: Vec<String>,
    builder: Builder,
    /// Ticks since the world opened.
    now: u64,
    /// The last `delete` that is waiting to be repeated: player, ride, tick (S3).
    pending_delete: Option<(u128, String, u64)>,
    /// `park.json` needs writing.
    park_dirty: bool,
    /// The display budget (D27); a field so a test can lower it.
    display_budget: usize,
    /// The tick of the last budget warning (one a minute).
    last_budget_warning: Option<u64>,
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
            build_queue: VecDeque::new(),
            loaded: false,
            waited: 0,
            world: None,
            session: 0,
            rides: Vec::new(),
            locked: Vec::new(),
            builder: Builder::new(),
            now: 0,
            pending_delete: None,
            park_dirty: false,
            display_budget: DISPLAY_BUDGET,
            last_budget_warning: None,
        }
    }

    /// A new world opened: forget everything that belonged to the old one.
    pub fn world_opened(&mut self) {
        self.welcome.world_opened();
        self.orders.clear();
        self.build_queue.clear();
        self.rides.clear();
        self.locked.clear();
        self.builder.clear_all();
        self.pending_delete = None;
        self.cleanup_pending = true;
        self.loaded = false;
        self.waited = 0;
        self.world = None;
        self.session = 0;
        self.now = 0;
        self.park_dirty = false;
        self.last_budget_warning = None;
    }

    /// The world is closing: write down anything not saved yet (D20).
    pub fn world_closing<H: Host>(&mut self, host: &mut H) {
        host.log(&format!(
            "Horizon Theme Park: world closing, world folder = {:?}",
            host.world_dir()
        ));
        self.save_dirty(host);
    }

    /// The track wand was used (a hook heard it). The work is queued for the next tick: commands
    /// are refused inside player hooks (F1).
    pub fn wand_click(&mut self, uuid: u128, click: WandClick) {
        self.orders.push_back(Order::Wand { uuid, click });
    }

    fn ride_index(&self, id: &str) -> Option<usize> {
        self.rides.iter().position(|r| r.id == id)
    }

    fn name_taken(&self, id: &str) -> bool {
        self.ride_index(id).is_some() || self.locked.iter().any(|l| l == id)
    }

    /// A player typed `line` (without the slash). Returns the answer to show them, or `None`
    /// when this is not one of our commands. Real work is queued for the next tick.
    pub fn command(&mut self, line: &str, caller: Caller) -> Option<String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() != Some(&"coaster") {
            return None;
        }
        let uuid = caller.uuid;
        let answer = match (words.get(1).copied(), words.len()) {
            (Some("demo"), 2) => {
                self.orders.push_back(Order::Demo(caller));
                "Building the demo coaster to your east.".to_string()
            }
            (Some("ride"), 2) => {
                if self.rides.is_empty() {
                    "There is no coaster yet. Type /coaster demo first.".to_string()
                } else {
                    "Right-click a seat in the car to ride.".to_string()
                }
            }
            (Some("remove"), 2) => {
                self.orders.push_back(Order::RemoveDemo);
                "Removing the demo coaster.".to_string()
            }
            (Some("wand"), 2) => {
                self.orders.push_back(Order::Give(caller));
                "Here is the track wand. Right-click a block to place a node. Right-click the air to switch between track, lift, brake and station. /coaster undo removes the last node. /coaster create <name> makes the ride.".to_string()
            }
            (Some("undo"), 2) => match self.builder.undo(uuid, self.now) {
                Some(n) => format!("Removed node {n}."),
                None => "You have no nodes to remove.".to_string(),
            },
            (Some("list"), 2) => self.list_text(),
            (Some("create"), 3) => {
                let name = words[2];
                let exists = |n: &str| self.name_taken(n);
                match self.builder.create(uuid, name, exists) {
                    Err(message) => message,
                    Ok(ride) => {
                        self.orders.push_back(Order::Create { uuid, ride });
                        format!("Checking your track for {name}...")
                    }
                }
            }
            (Some("edit"), 3) => self.edit_text(uuid, words[2]),
            (Some("delete"), 3) => self.delete_text(uuid, words[2]),
            (Some("set"), 5) if words[3] == "cars" => self.set_cars_text(uuid, words[2], words[4]),
            (Some("help"), 2) | (_, _) => {
                "Usage: /coaster wand, undo, create <name>, edit <name>, delete <name>, list, set <name> cars <1-8>, demo, ride or remove.".to_string()
            }
        };
        Some(answer)
    }

    fn list_text(&self) -> String {
        let mut parts: Vec<String> = self
            .rides
            .iter()
            .map(|r| {
                if r.file.is_some() {
                    format!(
                        "{} ({:.0} blocks, {} cars)",
                        r.id,
                        r.course.table.length(),
                        r.cars
                    )
                } else {
                    format!("{} (the demo)", r.id)
                }
            })
            .collect();
        parts.extend(
            self.locked
                .iter()
                .map(|l| format!("{l} (file could not be used)")),
        );
        if parts.is_empty() {
            "There are no rides yet.".to_string()
        } else {
            format!("Rides: {}", parts.join(", "))
        }
    }

    fn edit_text(&mut self, uuid: u128, name: &str) -> String {
        if self.locked.iter().any(|l| l == name) {
            return format!("The file of {name} could not be used, so it can't be edited.");
        }
        let file = self
            .ride_index(name)
            .and_then(|i| self.rides[i].file.clone());
        match file {
            Some(file) => {
                self.builder.load_for_edit(uuid, &file, self.now);
                format!(
                    "Loaded the {} nodes of {name} into your wand. Change them, then /coaster create {name} to save.",
                    file.nodes.len()
                )
            }
            None => format!("There is no ride called {name}."),
        }
    }

    fn delete_text(&mut self, uuid: u128, name: &str) -> String {
        if self
            .ride_index(name)
            .is_none_or(|i| self.rides[i].file.is_none())
        {
            return format!("There is no saved ride called {name}.");
        }
        let again = matches!(
            &self.pending_delete,
            Some((who, id, at)) if *who == uuid && id == name && self.now.saturating_sub(*at) <= DELETE_WINDOW
        );
        if again {
            self.pending_delete = None;
            self.orders.push_back(Order::Delete {
                uuid,
                id: name.to_string(),
            });
            format!("Deleting {name}. Its file is kept as {name}.json.deleted.")
        } else {
            self.pending_delete = Some((uuid, name.to_string(), self.now));
            format!("Type /coaster delete {name} again within 10 seconds to delete it.")
        }
    }

    fn set_cars_text(&mut self, uuid: u128, name: &str, count: &str) -> String {
        let Ok(cars) = count.parse::<usize>() else {
            return "Cars must be a number from 1 to 8.".to_string();
        };
        if !(1..=MAX_CARS).contains(&cars) {
            return format!("Cars must be a number from 1 to {MAX_CARS}.");
        }
        let Some(file) = self
            .ride_index(name)
            .and_then(|i| self.rides[i].file.clone())
        else {
            return format!("There is no ride called {name}.");
        };
        // The station must still be long enough for the new train.
        let mut changed = file;
        if let Some(train) = changed.trains.first_mut() {
            train.cars = cars as u32;
        }
        if let Err(why) = builder::course_from(&changed) {
            return why;
        }
        self.orders.push_back(Order::SetCars {
            uuid,
            id: name.to_string(),
            cars,
        });
        format!("Giving {name} {cars} cars.")
    }

    /// One server tick.
    pub fn tick<H: Host>(&mut self, host: &mut H) {
        self.now += 1;
        // Reading the player list costs a little, so only do it when something needs it.
        let need_players = self.welcome.waiting()
            || self.cleanup_pending
            || !self.loaded
            || !self.rides.is_empty()
            || !self.orders.is_empty()
            || !self.builder.sessions().is_empty();
        let players = if need_players {
            host.players()
        } else {
            Vec::new()
        };

        if let Some(line) = self.welcome.tick(!players.is_empty()) {
            host.broadcast(line);
        }
        if self.cleanup_pending && !players.is_empty() {
            // D20: anything of ours left in the world is stale.
            host.run(&commands::kill_all());
            self.cleanup_pending = false;
        }
        if !self.cleanup_pending && !self.loaded {
            self.load_world(host);
        }

        self.run_orders(host, &players);

        // Build commands first, keeping room for this tick's moves.
        let reserved: usize = self.rides.iter().map(Ride::move_cost).sum();
        let room = MAX_COMMANDS_PER_TICK.saturating_sub(reserved);
        for _ in 0..room.min(self.build_queue.len()) {
            if let Some(command) = self.build_queue.pop_front() {
                host.run(&command);
            }
        }

        for ride in &mut self.rides {
            ride.step(host, &players);
        }
        self.park_and_draw(host, &players, reserved);
        for ride in &mut self.rides {
            ride.emit_moves(host);
        }
        self.wand_display(host, &players);
        self.sweep(host, &players);
        self.save_dirty(host);
    }

    // ---- loading (step 1) ----

    /// Read the saved rides from the world folder (D20), once per world.
    fn load_world<H: Host>(&mut self, host: &mut H) {
        let Some(dir) = host.world_dir() else {
            self.waited += 1;
            if self.waited >= WORLD_WAIT_TICKS {
                host.log("Horizon Theme Park: no world folder was found; rides will not be saved");
                host.broadcast(
                    "Horizon Theme Park: this world has no save folder, so rides cannot be saved.",
                );
                self.loaded = true;
            }
            return;
        };
        host.log(&format!(
            "Horizon Theme Park: world folder {} (after {} ticks)",
            dir.display(),
            self.waited
        ));
        self.loaded = true;
        match save::load_park(&dir) {
            Err(error) => {
                host.broadcast(&format!(
                    "Horizon Theme Park: park.json: {error}. Rides will not be saved this time."
                ));
                return;
            }
            Ok(park) => {
                self.session = park.session + 1;
                self.world = Some(dir.clone());
                self.park_dirty = true;
            }
        }
        let found = save::load_all(&dir);
        for problem in &found.problems {
            host.broadcast(&format!("Horizon Theme Park: {problem}"));
        }
        self.locked = found.locked;
        // The demo (if one was typed in the first moments) was drawn with the old session tag.
        for ride in &mut self.rides {
            if ride.anything_drawn() {
                host.run(&commands::kill_ride(&ride.id));
                ride.train_drawn = false;
                for group in &mut ride.groups {
                    group.drawn = false;
                }
            }
        }
        for file in found.rides {
            let id = file.id.clone();
            match Ride::from_file(file) {
                Ok(ride) => self.rides.push(ride),
                Err(why) => {
                    host.broadcast(&format!(
                        "Horizon Theme Park: ride {id} can't be built: {why}"
                    ));
                    self.locked.push(id);
                }
            }
        }
        host.log(&format!(
            "Horizon Theme Park: session {}, {} ride(s) loaded",
            self.session,
            self.rides.len()
        ));
    }

    // ---- orders (step 2) ----

    fn run_orders<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo]) {
        while let Some(order) = self.orders.pop_front() {
            // Orders that touch the saved rides wait until they are loaded (the demo and the
            // wand do not need them). Order is kept.
            let needs_world = matches!(
                order,
                Order::Create { .. } | Order::Delete { .. } | Order::SetCars { .. }
            );
            if needs_world && !self.loaded {
                self.orders.push_front(order);
                break;
            }
            match order {
                Order::Demo(caller) => {
                    self.remove_demo(host);
                    self.build_demo(&caller);
                }
                Order::RemoveDemo => self.remove_demo(host),
                Order::Give(caller) => {
                    host.run(&commands::give(&caller.name, commands::WAND_ITEM));
                }
                Order::Wand { uuid, click } => {
                    let text = match click {
                        WandClick::Block(block) => {
                            let n = self.builder.click_block(uuid, block, self.now);
                            let mode = self.builder.session(uuid).map_or(Kind::Track, |s| s.mode);
                            format!("Node {n} placed ({})", builder::mode_name(mode))
                        }
                        WandClick::Air => {
                            let mode = self.builder.click_air(uuid, self.now);
                            format!("Wand mode: {}", builder::mode_name(mode))
                        }
                    };
                    show(host, players, uuid, &text);
                }
                Order::Create { uuid, ride } => self.finish_create(host, uuid, ride),
                Order::Delete { uuid, id } => self.finish_delete(host, uuid, &id),
                Order::SetCars { uuid, id, cars } => self.finish_set_cars(host, uuid, &id, cars),
            }
        }
    }

    fn remove_demo<H: Host>(&mut self, host: &mut H) {
        host.run(&commands::kill_ride(commands::DEMO));
        if let Some(i) = self.ride_index(commands::DEMO) {
            if let Some(platform) = self.rides[i].platform {
                host.run(&commands::fill(platform.min, platform.max, "minecraft:air"));
            }
            self.rides.remove(i);
        }
    }

    /// Turn a `/coaster demo` order into a ride; its rails, car and seats are drawn by parking.
    fn build_demo(&mut self, caller: &Caller) {
        // East of the player (+X), so the mod never has to read which way they look (D52).
        let origin = caller.pos + DVec3::new(DEMO_EAST, DEMO_HEIGHT, 0.0);
        let course = demo::demo_course(origin);
        let physics = Physics::default();
        let stop = course.station().map_or(0.0, |(_, stop, _)| stop);
        let train = Train::held(demo::CARS, stop);
        let platform = render::station_platform(&course.table, stop, demo::CARS, train.spacing);
        self.build_queue.push_back(commands::fill(
            platform.min,
            platform.max,
            commands::PLATFORM_BLOCK,
        ));
        let mut ride = Ride::new(
            commands::DEMO.to_string(),
            None,
            course,
            physics,
            train,
            DWELL,
        );
        ride.platform = Some(platform);
        self.rides.push(ride);
    }

    /// Put `file`'s ride in the world, replacing a ride of the same name. Returns the ride.
    fn install<H: Host>(&mut self, host: &mut H, file: RideFile) -> Result<usize, String> {
        let mut ride = Ride::from_file(file)?;
        if let Some(i) = self.ride_index(&ride.id) {
            host.run(&commands::kill_ride(&ride.id));
            self.rides.remove(i);
        }
        ride.dirty = true;
        self.park_dirty = true;
        self.rides.push(ride);
        Ok(self.rides.len() - 1)
    }

    fn finish_create<H: Host>(&mut self, host: &mut H, uuid: u128, mut file: RideFile) {
        let editing = self.builder.session(uuid).and_then(|s| s.editing.clone());
        if self.name_taken(&file.id) && editing.as_deref() != Some(file.id.as_str()) {
            host.tell(
                uuid,
                &format!(
                    "A ride called {} already exists. Use /coaster edit {} to change it.",
                    file.id, file.id
                ),
            );
            return;
        }
        // Editing keeps the old ride's train and settings.
        if let Some(old) = self
            .ride_index(&file.id)
            .and_then(|i| self.rides[i].file.as_ref())
        {
            file.settings = old.settings.clone();
            file.trains = old.trains.clone();
        }
        let id = file.id.clone();
        match self.install(host, file) {
            Err(why) => host.tell(uuid, &why),
            Ok(i) => {
                self.builder.reset(uuid);
                let ride = &self.rides[i];
                let report =
                    report::analyse(&ride.course, ride.cars, ride.train.spacing, &ride.physics);
                host.tell(
                    uuid,
                    &format!("Ride {id} is built. The train waits in the station."),
                );
                for line in report.lines(&id) {
                    host.tell(uuid, &line);
                }
                if self.world.is_none() {
                    host.tell(
                        uuid,
                        "This world has no save folder, so this ride will not be saved.",
                    );
                }
            }
        }
    }

    fn finish_delete<H: Host>(&mut self, host: &mut H, uuid: u128, id: &str) {
        let Some(i) = self.ride_index(id) else {
            host.tell(uuid, &format!("There is no ride called {id}."));
            return;
        };
        host.run(&commands::kill_ride(id));
        self.rides.remove(i);
        self.park_dirty = true;
        if let Some(dir) = &self.world
            && let Err(why) = save::delete_ride(dir, id)
        {
            host.tell(uuid, &why);
            return;
        }
        host.tell(
            uuid,
            &format!("Ride {id} is deleted (its file is kept as {id}.json.deleted)."),
        );
    }

    fn finish_set_cars<H: Host>(&mut self, host: &mut H, uuid: u128, id: &str, cars: usize) {
        let Some(mut file) = self.ride_index(id).and_then(|i| self.rides[i].file.clone()) else {
            host.tell(uuid, &format!("There is no ride called {id}."));
            return;
        };
        match file.trains.first_mut() {
            Some(train) => train.cars = cars as u32,
            None => file.trains.push(save::TrainFile {
                cars: cars as u32,
                spacing: crate::coaster::physics::CAR_SPACING,
            }),
        }
        match self.install(host, file) {
            Ok(_) => host.tell(uuid, &format!("{id} now has {cars} cars.")),
            Err(why) => host.tell(uuid, &why),
        }
    }

    // ---- parking (step 5) ----

    /// Remove the displays of track far from everyone, and draw track near someone (D30).
    fn park_and_draw<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo], reserved: usize) {
        let here: Vec<DVec3> = players
            .iter()
            .filter(|p| p.dimension == "minecraft:overworld")
            .map(|p| p.pos)
            .collect();
        let session = self.session;

        // Park first, so the freed room counts for this tick's drawing.
        for ride in &mut self.rides {
            let riding = ride.seats.iter().any(|s| s.occupied);
            if ride.train_drawn && !riding && nearest(&here, ride.front_pos()) > PARK_DISTANCE {
                host.run(&commands::kill_tag(&commands::tag_group(&ride.id, "t")));
                ride.train_drawn = false;
                for seat in &mut ride.seats {
                    seat.occupied = false;
                    seat.rider = None;
                }
            }
            for (g, group) in ride.groups.iter_mut().enumerate() {
                if group.drawn && nearest(&here, group.center) - group.radius > PARK_DISTANCE {
                    host.run(&commands::kill_tag(&commands::tag_group(
                        &ride.id,
                        &g.to_string(),
                    )));
                    group.drawn = false;
                }
            }
        }

        // How many displays each player has near them now (D27).
        let mut load: Vec<usize> = here
            .iter()
            .map(|&p| {
                self.rides
                    .iter()
                    .map(|r| {
                        let rails: usize = r
                            .groups
                            .iter()
                            .filter(|g| g.drawn && g.center.distance(p) - g.radius <= BUDGET_RADIUS)
                            .map(|g| g.end - g.first)
                            .sum();
                        let cars = if r.train_drawn && r.front_pos().distance(p) <= BUDGET_RADIUS {
                            r.cars
                        } else {
                            0
                        };
                        rails + cars
                    })
                    .sum()
            })
            .collect();

        let mut budget = DRAWS_PER_TICK.min(MAX_COMMANDS_PER_TICK.saturating_sub(reserved));
        let mut over_budget = false;
        let budget_limit = self.display_budget;
        for ride in &mut self.rides {
            // The train first, so riders have their seats.
            if !ride.train_drawn {
                let cost = ride.cars + ride.seats.len();
                let near = nearest(&here, ride.front_pos()) <= DRAW_DISTANCE;
                if near && cost <= budget {
                    for command in ride.train_commands(session) {
                        host.run(&command);
                    }
                    budget -= cost;
                }
            }
            for g in 0..ride.groups.len() {
                let (center, radius, len, drawn) = {
                    let group = &ride.groups[g];
                    (
                        group.center,
                        group.radius,
                        group.end - group.first,
                        group.drawn,
                    )
                };
                if drawn || nearest(&here, center) - radius > DRAW_DISTANCE || len > budget {
                    continue;
                }
                // Past the display budget near a player: wait (the player is warned).
                let affected: Vec<usize> = (0..here.len())
                    .filter(|&i| here[i].distance(center) - radius <= BUDGET_RADIUS)
                    .collect();
                if affected.iter().any(|&i| load[i] + len > budget_limit) {
                    over_budget = true;
                    continue;
                }
                for command in ride.rail_commands(g, session) {
                    host.run(&command);
                }
                ride.groups[g].drawn = true;
                budget -= len;
                for i in affected {
                    load[i] += len;
                }
            }
        }
        if over_budget
            && self
                .last_budget_warning
                .is_none_or(|at| self.now.saturating_sub(at) >= 1200)
        {
            self.last_budget_warning = Some(self.now);
            host.broadcast("Horizon Theme Park: too many rides near you; some track is not drawn until you move away.");
        }
    }

    // ---- wand markers and preview (step 7) ----

    fn wand_display<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo]) {
        let now = self.now;
        let session = self.session;
        let mut markers_left = MARKERS_PER_TICK;
        for s in self.builder.sessions_mut() {
            if s.reload {
                if s.markers > 0 {
                    host.run(&commands::kill_wand(s.uuid));
                }
                s.markers = 0;
                s.reload = false;
                s.preview.clear();
                s.preview_version = u64::MAX;
            }
            let live = now.saturating_sub(s.last_use) <= PREVIEW_TICKS && !s.nodes.is_empty();
            if !live {
                if s.markers > 0 {
                    host.run(&commands::kill_wand(s.uuid));
                    s.markers = 0;
                }
                s.preview.clear();
                s.preview_version = u64::MAX;
                continue;
            }
            while s.markers > s.nodes.len() {
                s.markers -= 1;
                host.run(&commands::kill_node(s.uuid, s.markers));
            }
            while s.markers < s.nodes.len() && markers_left > 0 {
                let p = s.nodes[s.markers].pos;
                host.run(&commands::summon_node(
                    s.uuid,
                    s.markers,
                    session,
                    DVec3::new(p[0], p[1], p[2]),
                ));
                s.markers += 1;
                markers_left -= 1;
            }
            if s.preview_version != s.version {
                s.preview = builder::preview_points(&s.nodes, PREVIEW_STEP, PREVIEW_MAX_POINTS);
                s.preview_version = s.version;
            }
            // The dots are spread over the 10 ticks between redraws, and only the player who built sees them.
            if let Some(player) = players.iter().find(|p| p.uuid == s.uuid) {
                let phase = (now % PREVIEW_EVERY) as usize;
                for (i, point) in s.preview.iter().enumerate() {
                    if i % PREVIEW_EVERY as usize == phase {
                        host.run(&commands::particle(&player.name, *point));
                    }
                }
            }
        }
    }

    // ---- sweeping (step 7) ----

    /// Every 5 seconds, remove leftovers near each player: entities of an earlier session (D66),
    /// and entities of an older drawing of a group (D30). Chunks that were unloaded when we
    /// removed things bring their old entities back (spike R8).
    fn sweep<H: Host>(&mut self, host: &mut H, players: &[PlayerInfo]) {
        if !self.loaded || !self.now.is_multiple_of(SCRUB_EVERY) {
            return;
        }
        for p in players
            .iter()
            .filter(|p| p.dimension == "minecraft:overworld")
        {
            host.run(&commands::scrub_session(p.pos, self.session));
            for ride in &self.rides {
                for (g, group) in ride.groups.iter().enumerate() {
                    if group.drawn && group.center.distance(p.pos) - group.radius <= 128.0 {
                        host.run(&commands::scrub_generation(
                            p.pos,
                            &ride.id,
                            &g.to_string(),
                            group.generation,
                        ));
                    }
                }
                if ride.train_drawn && ride.front_pos().distance(p.pos) <= 128.0 {
                    host.run(&commands::scrub_generation(
                        p.pos,
                        &ride.id,
                        "t",
                        ride.train_gen,
                    ));
                }
            }
        }
    }

    // ---- saving (step 8) ----

    fn save_dirty<H: Host>(&mut self, host: &mut H) {
        let Some(dir) = self.world.clone() else {
            for ride in &mut self.rides {
                ride.dirty = false;
            }
            self.park_dirty = false;
            return;
        };
        for ride in self.rides.iter_mut().filter(|r| r.dirty) {
            ride.dirty = false;
            if let Some(file) = &ride.file
                && let Err(why) = save::save_ride(&dir, file)
            {
                host.broadcast(&format!("Horizon Theme Park: {why}"));
            }
        }
        if self.park_dirty {
            self.park_dirty = false;
            let mut ids: Vec<String> = self
                .rides
                .iter()
                .filter(|r| r.file.is_some())
                .map(|r| r.id.clone())
                .chain(self.locked.iter().cloned())
                .collect();
            ids.sort();
            let park = ParkFile {
                format: save::FORMAT,
                session: self.session,
                rides: ids,
            };
            if let Err(why) = save::save_park(&dir, &park) {
                host.broadcast(&format!("Horizon Theme Park: {why}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, PlayerInfo};
    use std::fs;

    const SEATS: usize = demo::CARS * SEATS_PER_CAR;

    fn player(uuid: u128, pos: DVec3) -> PlayerInfo {
        PlayerInfo {
            uuid,
            name: format!("Player{uuid}"),
            pos,
            dimension: "minecraft:overworld".to_string(),
        }
    }

    /// Far from everything: nothing is drawn near this player.
    fn far_away() -> DVec3 {
        DVec3::new(5000.0, 70.0, 5000.0)
    }

    /// Standing beside the demo's station: the train and the nearest track are drawn.
    fn bystander() -> DVec3 {
        DVec3::new(10.0, 70.0, 0.0)
    }

    fn caller() -> Caller {
        Caller {
            uuid: 1,
            name: "Player1".to_string(),
            pos: DVec3::new(0.0, 70.0, 0.0),
        }
    }

    /// An empty scratch world folder for one test.
    fn world(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("htp-test-runtime-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Tick until a tick draws nothing new (no summons).
    fn settle(rt: &mut Runtime, host: &mut FakeHost) {
        for _ in 0..200 {
            rt.tick(host);
            let sent = host.take_commands();
            if !sent
                .iter()
                .any(|c| c.starts_with("summon") || c.starts_with("fill"))
                && rt.now > 3
            {
                return;
            }
        }
        panic!("never settled");
    }

    /// A runtime with a built demo, one player beside the station, and everything near drawn.
    fn built() -> (Runtime, FakeHost) {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, bystander()));
        rt.tick(&mut host); // greeting + cleanup
        rt.command("coaster demo", caller());
        rt.tick(&mut host);
        settle(&mut rt, &mut host);
        host.take_commands();
        (rt, host)
    }

    fn demo_ride(rt: &Runtime) -> &Ride {
        &rt.rides[rt.ride_index("demo").unwrap()]
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
        host.players[0].pos = render::rider_point(demo_ride(rt).seats[0].pos);
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
        assert!(rt.rides.is_empty());
        rt.tick(&mut host);
        assert_eq!(host.broadcasts.len(), 2);
        assert_eq!(host.commands, vec!["kill @e[tag=htp]"]);
    }

    #[test]
    fn demo_draws_the_train_and_the_nearby_rails_and_the_ride_command_is_only_a_hint() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, bystander()));
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
        for _ in 0..10 {
            rt.tick(&mut host);
            per_tick.push(host.take_commands());
        }
        assert!(per_tick.iter().all(|t| t.len() <= MAX_COMMANDS_PER_TICK));
        let all: Vec<&String> = per_tick.iter().flatten().collect();

        assert_eq!(all[0], "kill @e[tag=htp_demo]");
        let summons: Vec<&&String> = all.iter().filter(|c| c.starts_with("summon")).collect();
        let cars: Vec<&&&String> = summons
            .iter()
            .filter(|c| c.contains("\"htp_car_demo_"))
            .collect();
        assert_eq!(cars.len(), demo::CARS);
        for (i, c) in cars.iter().enumerate() {
            assert!(
                c.contains(&format!("\"htp_car_demo_{i}\"")) && c.contains("teleport_duration:")
            );
            assert!(c.contains("\"htp_s0\"") || c.contains("\"htp_s1\""), "{c}");
            assert!(c.contains("\"htp_g_demo_t\"") && c.contains("\"htp_gen_1\""));
        }
        let seats = summons
            .iter()
            .filter(|c| c.starts_with("summon minecraft:minecart"))
            .count();
        assert_eq!(seats, SEATS);
        let rails = summons.iter().filter(|c| c.contains("htp:rail")).count();
        // Only the track near the player is drawn: some, but not the whole 300 blocks.
        let all_pieces = demo_ride(&rt).pieces.len();
        assert!(
            rails >= GROUP_SIZE && rails < all_pieces,
            "{rails} of {all_pieces}"
        );
        assert!(
            rt.command("coaster ride", caller())
                .unwrap()
                .starts_with("Right-click a seat")
        );
    }

    #[test]
    fn the_station_is_three_blocks_east_of_the_player_and_the_rails_float_five_blocks_above_their_feet()
     {
        let (rt, _) = built();
        let table = &demo_ride(&rt).course.table;
        // The player stands at (0, 70, 0). The station's near end is 3 blocks east.
        let (station, stop, _) = demo_ride(&rt).course.station().unwrap();
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
            // Only the seats move (and the 5-second sweep, which has no leftovers to report here).
            assert_eq!(seat_move_count(&cmds), SEATS);
            assert!(car_moves(&cmds).is_empty());
        }
        let d = demo_ride(&rt);
        assert!(d.train.is_held() && d.train.countdown().is_none());
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn the_countdown_starts_when_someone_sits_and_the_train_leaves_after_five_seconds() {
        let (mut rt, mut host) = built();
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        assert!(demo_ride(&rt).train.countdown().is_some());
        let mut ticks = 1;
        while demo_ride(&rt).train.is_held() {
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
        assert!(demo_ride(&rt).train.countdown().is_some());
        host.players[0].pos = bystander();
        rt.tick(&mut host);
        assert!(demo_ride(&rt).train.is_held());
        assert!(demo_ride(&rt).train.countdown().is_none());
    }

    // Test 13 (M2): cars aimed k ticks ahead by running the physics forward.
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
                let d = demo_ride(&rt);
                // `step` ran before this tick's moves, so the copy of the train is one tick on.
                let mut ahead = d.train;
                for _ in 0..d.k {
                    ahead.step(&d.course, &d.physics);
                }
                let want = render::car_pose(&d.course.table, ahead.s);
                assert_eq!(
                    **first,
                    commands::move_car(commands::DEMO, 0, want.pos, want.yaw, want.pitch)
                );
                assert!((1..=render::MAX_TELEPORT_TICKS).contains(&d.k));
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
        assert!(
            !demo_ride(&rt).train.is_held(),
            "it should be on its way by now"
        );
    }

    // Test 14: the action bar.
    #[test]
    fn riders_and_only_riders_see_the_countdown_and_then_their_speed() {
        let (mut rt, mut host) = built();
        host.players.push(player(2, bystander())); // a bystander
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
        plain.0.players[0].pos = render::rider_point(demo_ride(&rt).seats[0].pos);
        for _ in 0..10 {
            rt.tick(&mut plain);
            plain.0.players[0].pos = render::rider_point(demo_ride(&rt).seats[0].pos);
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
            let held = demo_ride(&rt).train.is_held();
            if !held {
                left = true;
            }
            if left && held {
                // Back in the station at the stop point.
                let d = demo_ride(&rt);
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
        let seat = demo_ride(&rt).seats[0].pos;
        host.players[0].pos = render::rider_point(seat) + DVec3::new(0.0, -0.9, 0.0);
        for _ in 0..5 {
            rt.tick(&mut host);
            host.players[0].pos =
                render::rider_point(demo_ride(&rt).seats[0].pos) + DVec3::new(0.0, -0.9, 0.0);
        }
        assert!(demo_ride(&rt).seats[0].occupied);
        let boarded: Vec<&String> = host.logs.iter().filter(|l| l.contains("boarded")).collect();
        assert_eq!(boarded.len(), 1, "{:?}", host.logs);
        assert_eq!(
            boarded[0],
            "Horizon Theme Park: seat 0 boarded, rider minus seat = 0.0000 -2.1125 0.0000"
        );
    }

    #[test]
    fn a_player_between_the_two_distances_changes_nothing() {
        let (mut rt, mut host) = built();
        // Standing 1.5 blocks from the rider point: not close enough to count as sitting.
        host.players[0].pos = render::rider_point(demo_ride(&rt).seats[0].pos) + DVec3::X * 1.5;
        rt.tick(&mut host);
        assert!(!demo_ride(&rt).seats[0].occupied);
        // Sitting, then stepping 1 block away: still counts as sitting.
        sit_in_seat_zero(&rt, &mut host);
        rt.tick(&mut host);
        assert!(demo_ride(&rt).seats[0].occupied);
        host.players[0].pos += DVec3::X * 1.0;
        rt.tick(&mut host);
        assert!(demo_ride(&rt).seats[0].occupied);
        // Walking right away (more than 2 blocks): off.
        host.players[0].pos = bystander();
        rt.tick(&mut host);
        assert!(!demo_ride(&rt).seats[0].occupied);
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
        // And a runtime with no ride sends nothing at all.
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
        assert!(rt.rides.is_empty());
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
        assert_eq!(rt.rides.len(), 1);
    }

    #[test]
    fn the_demo_places_a_smooth_stone_platform_first_and_remove_clears_exactly_those_blocks() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, bystander()));
        rt.tick(&mut host);
        host.take_commands();
        // The player stands at (0, 70, 0): the station track is at y 75, z 0, x -9.5 .. 3 east of them.
        rt.command("coaster demo", caller());
        let mut all = Vec::new();
        for _ in 0..10 {
            rt.tick(&mut host);
            all.extend(host.take_commands());
        }
        let place: Vec<&String> = all.iter().filter(|c| c.starts_with("fill ")).collect();
        assert_eq!(place.len(), 1, "{place:?}");
        let platform = demo_ride(&rt).platform.unwrap();
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
        let d = demo_ride(&rt);
        let p = d.platform.unwrap();
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

    // ---- M3 ----

    /// A file for a ride made from the builder test's loop, moved to `at`.
    fn ride_file(id: &str, at: DVec3) -> RideFile {
        let mut nodes = builder::tests::loop_nodes();
        for n in &mut nodes {
            n.pos = [n.pos[0] + at.x, n.pos[1] + at.y - 70.0, n.pos[2] + at.z];
        }
        RideFile::new(id, nodes)
    }

    /// A runtime in a scratch world with `rides` saved on disk, one player, loaded and settled.
    fn world_with(
        name: &str,
        rides: &[(&str, DVec3)],
        player_at: DVec3,
    ) -> (Runtime, FakeHost, PathBuf) {
        let dir = world(name);
        for (id, at) in rides {
            save::save_ride(&dir, &ride_file(id, *at)).unwrap();
        }
        let mut host = FakeHost::new();
        host.world = Some(dir.clone());
        host.players.push(player(1, player_at));
        (Runtime::new(), host, dir)
    }

    // Test 9.
    #[test]
    fn on_world_open_one_kill_then_rides_load_and_only_the_near_ones_are_drawn() {
        let (mut rt, mut host, _) = world_with(
            "open",
            &[
                ("near", DVec3::new(0.0, 70.0, 0.0)),
                ("far", DVec3::new(2000.0, 70.0, 0.0)),
            ],
            DVec3::new(10.0, 70.0, 10.0),
        );
        rt.world_opened();
        rt.tick(&mut host);
        let first = host.take_commands();
        assert_eq!(first[0], "kill @e[tag=htp]");
        assert_eq!(first.iter().filter(|c| c.starts_with("kill ")).count(), 1);
        assert_eq!(rt.rides.len(), 2);
        assert_eq!(rt.session, 1);
        let summons: Vec<&String> = first.iter().filter(|c| c.starts_with("summon")).collect();
        assert!(!summons.is_empty());
        assert!(
            summons
                .iter()
                .all(|c| c.contains("\"htp_near\"") && c.contains("\"htp_s1\"")),
            "{summons:?}"
        );
        // The new session number was written down, and the train waits in its station.
        assert_eq!(
            save::load_park(&host.world.clone().unwrap())
                .unwrap()
                .session,
            1
        );
        assert!(rt.rides.iter().all(|r| r.train.is_held()));
        // A second opening bumps the session.
        rt.world_opened();
        rt.tick(&mut host);
        assert_eq!(rt.session, 2);
    }

    #[test]
    fn a_world_with_no_folder_waits_a_little_then_says_rides_cannot_be_saved() {
        let mut rt = Runtime::new();
        let mut host = FakeHost::new();
        host.players.push(player(1, bystander()));
        for _ in 0..WORLD_WAIT_TICKS + 2 {
            rt.tick(&mut host);
        }
        assert!(rt.loaded && rt.world.is_none());
        assert!(
            host.broadcasts
                .iter()
                .any(|b| b.contains("cannot be saved"))
        );
    }

    // Test 10.
    #[test]
    fn wand_clicks_are_queued_and_run_on_the_next_tick_in_order() {
        let (mut rt, mut host, _) = world_with("wand-orders", &[], bystander());
        rt.tick(&mut host);
        host.take_commands();
        rt.wand_click(1, WandClick::Block([10, 64, 10]));
        rt.wand_click(1, WandClick::Air);
        rt.wand_click(1, WandClick::Block([12, 64, 10]));
        assert!(host.commands.is_empty() && host.action_bars.is_empty());
        assert!(
            rt.builder.session(1).is_none(),
            "nothing happens inside the hook"
        );
        rt.tick(&mut host);
        let s = rt.builder.session(1).unwrap();
        assert_eq!(s.nodes.len(), 2);
        assert_eq!(s.nodes[0].kind, Kind::Track);
        assert_eq!(s.nodes[1].kind, Kind::Lift);
        let texts: Vec<&str> = host.action_bars.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Node 1 placed (track)",
                "Wand mode: lift",
                "Node 2 placed (lift)"
            ]
        );
    }

    #[test]
    fn the_wand_shows_markers_and_dots_only_to_the_builder_and_stops_after_thirty_seconds() {
        let (mut rt, mut host, _) = world_with("wand-show", &[], bystander());
        rt.tick(&mut host);
        host.take_commands();
        for i in 0..5 {
            rt.wand_click(1, WandClick::Block([i * 3, 64, 0]));
        }
        host.players.push(player(2, bystander()));
        rt.tick(&mut host);
        let sent = host.take_commands();
        assert_eq!(sent.iter().filter(|c| c.contains("htp:node")).count(), 5);
        // Over the next 10 ticks every preview dot is sent once, to player 1 only.
        let mut dots = Vec::new();
        for _ in 0..10 {
            rt.tick(&mut host);
            dots.extend(
                host.take_commands()
                    .into_iter()
                    .filter(|c| c.starts_with("particle")),
            );
        }
        assert!(dots.len() > 20);
        assert!(dots.iter().all(|c| c.ends_with(" force Player1")));
        // Undo takes the last marker away.
        assert_eq!(
            rt.command("coaster undo", caller()).unwrap(),
            "Removed node 5."
        );
        rt.tick(&mut host);
        assert!(
            host.take_commands()
                .iter()
                .any(|c| c == "kill @e[tag=htp_wn_1_4]")
        );
        // After 30 s without the wand everything is removed.
        for _ in 0..PREVIEW_TICKS + 5 {
            rt.tick(&mut host);
        }
        let later = host.take_commands();
        assert!(later.iter().any(|c| c == "kill @e[tag=htp_wand_1]"));
        let before = host.commands.len();
        rt.tick(&mut host);
        assert_eq!(host.commands.len(), before, "nothing more is sent");
    }

    #[test]
    fn create_builds_a_ride_saves_it_tells_the_report_and_edit_brings_the_nodes_back() {
        let (mut rt, mut host, dir) = world_with("create", &[], bystander());
        rt.tick(&mut host);
        host.take_commands();
        // Place the loop with the wand: mode changes to station first.
        let nodes = builder::tests::loop_nodes();
        for (i, n) in nodes.iter().enumerate() {
            let wanted = n.kind;
            while rt.builder.session(1).map_or(Kind::Track, |s| s.mode) != wanted
                || (i == 0 && rt.builder.session(1).is_none())
            {
                rt.builder.click_air(1, 0);
                if wanted == Kind::Track {
                    break;
                }
            }
            // Click a block so the node lands at the wanted spot (x, y - 1.5, z) block centre.
            rt.builder.click_block(
                1,
                [
                    (n.pos[0] - 0.5) as i32,
                    (n.pos[1] - 1.5) as i32,
                    (n.pos[2] - 0.5) as i32,
                ],
                0,
            );
        }
        // Nodes use the modes they were placed in: fix the kinds exactly as the loop wants.
        for (i, n) in nodes.iter().enumerate() {
            rt.builder.sessions_mut()[0].nodes[i].kind = n.kind;
        }
        let answer = rt.command("coaster create thunder", caller()).unwrap();
        assert!(
            answer.starts_with("Checking your track for thunder"),
            "{answer}"
        );
        rt.tick(&mut host);
        let told: Vec<&str> = host.tells.iter().map(|(_, t)| t.as_str()).collect();
        assert!(told[0].starts_with("Ride thunder is built"), "{told:?}");
        assert!(
            told.iter()
                .any(|t| t.starts_with("Realism report for thunder"))
        );
        assert!(dir.join("horizon-theme-park/rides/thunder.json").exists());
        assert_eq!(save::load_park(&dir).unwrap().rides, vec!["thunder"]);
        assert!(rt.builder.session(1).unwrap().nodes.is_empty());
        // The same name is refused until you ask to edit it.
        rt.builder.click_block(1, [0, 0, 0], 0);
        assert!(
            rt.command("coaster create thunder", caller())
                .unwrap()
                .contains("/coaster edit thunder")
        );
        let edit = rt.command("coaster edit thunder", caller()).unwrap();
        assert!(edit.starts_with("Loaded the 8 nodes of thunder"), "{edit}");
        assert!(
            rt.command("coaster create thunder", caller())
                .unwrap()
                .starts_with("Checking")
        );
        // `list` knows it; cars change through `set`.
        assert!(
            rt.command("coaster list", caller())
                .unwrap()
                .contains("thunder (")
        );
        assert!(
            rt.command("coaster set thunder cars 9", caller())
                .unwrap()
                .contains("1 to 8")
        );
        assert!(
            rt.command("coaster set thunder cars 2", caller())
                .unwrap()
                .starts_with("Giving thunder 2 cars")
        );
        rt.tick(&mut host);
        assert_eq!(rt.rides[0].cars, 2);
        assert_eq!(
            save::load_ride(&dir.join("horizon-theme-park/rides/thunder.json"))
                .unwrap()
                .trains[0]
                .cars,
            2
        );
    }

    #[test]
    fn saved_rides_come_back_after_the_world_is_closed_and_opened_again() {
        let (mut rt, mut host, dir) = world_with(
            "reload",
            &[("keep", DVec3::new(0.0, 70.0, 0.0))],
            bystander(),
        );
        rt.tick(&mut host);
        assert_eq!(rt.rides.len(), 1);
        rt.command("coaster set keep cars 2", caller());
        rt.tick(&mut host);
        rt.world_closing(&mut host);
        let mut again = Runtime::new();
        again.tick(&mut host);
        assert_eq!(again.rides.len(), 1);
        assert_eq!(again.rides[0].cars, 2);
        assert!(again.rides[0].train.is_held());
        drop(dir);
    }

    #[test]
    fn delete_needs_two_commands_and_keeps_the_file() {
        let (mut rt, mut host, dir) = world_with(
            "delete",
            &[("old", DVec3::new(0.0, 70.0, 0.0))],
            bystander(),
        );
        rt.tick(&mut host);
        let first = rt.command("coaster delete old", caller()).unwrap();
        assert!(first.contains("again within 10 seconds"));
        assert!(dir.join("horizon-theme-park/rides/old.json").exists());
        rt.command("coaster delete old", caller());
        rt.tick(&mut host);
        assert!(!dir.join("horizon-theme-park/rides/old.json").exists());
        assert!(
            dir.join("horizon-theme-park/rides/old.json.deleted")
                .exists()
        );
        assert!(rt.rides.is_empty());
        // Too slow a second command starts again.
        let (mut rt, mut host, _) = world_with(
            "delete2",
            &[("old", DVec3::new(0.0, 70.0, 0.0))],
            bystander(),
        );
        rt.tick(&mut host);
        rt.command("coaster delete old", caller());
        for _ in 0..DELETE_WINDOW + 5 {
            rt.tick(&mut host);
        }
        assert!(
            rt.command("coaster delete old", caller())
                .unwrap()
                .contains("again within")
        );
    }

    #[test]
    fn a_newer_or_broken_ride_file_is_reported_kept_and_never_overwritten() {
        let (mut rt, mut host, dir) = world_with(
            "locked",
            &[("good", DVec3::new(0.0, 70.0, 0.0))],
            bystander(),
        );
        let newer = r#"{"format":9,"id":"future"}"#;
        fs::write(save::ride_path(&dir, "future"), newer).unwrap();
        fs::write(save::ride_path(&dir, "bad"), "{{{").unwrap();
        rt.tick(&mut host);
        assert_eq!(rt.rides.len(), 1);
        assert!(
            host.broadcasts
                .iter()
                .any(|b| b.contains("future") && b.contains("newer version"))
        );
        assert!(
            host.broadcasts
                .iter()
                .any(|b| b.contains("bad") && b.contains("damaged"))
        );
        // Their names are taken.
        assert!(
            rt.command("coaster edit future", caller())
                .unwrap()
                .contains("can't be edited")
        );
        rt.builder.click_block(1, [0, 0, 0], 0);
        assert!(
            rt.command("coaster create future", caller())
                .unwrap()
                .contains("already exists")
        );
        assert_eq!(
            fs::read_to_string(save::ride_path(&dir, "future")).unwrap(),
            newer
        );
        assert_eq!(
            save::load_park(&dir).unwrap().rides,
            vec!["bad", "future", "good"]
        );
    }

    // Test 11.
    #[test]
    fn track_is_parked_past_96_blocks_and_drawn_again_within_80_at_most_50_pieces_a_tick() {
        let (mut rt, mut host, _) = world_with(
            "parking",
            &[("a", DVec3::new(0.0, 70.0, 0.0))],
            DVec3::new(10.0, 70.0, 10.0),
        );
        let mut drawn_per_tick = Vec::new();
        for _ in 0..12 {
            rt.tick(&mut host);
            let pieces = host
                .take_commands()
                .iter()
                .filter(|c| c.contains("htp:rail"))
                .count();
            drawn_per_tick.push(pieces);
        }
        assert!(
            drawn_per_tick.iter().all(|&n| n <= DRAWS_PER_TICK),
            "{drawn_per_tick:?}"
        );
        let all_drawn = rt.rides[0].groups.iter().all(|g| g.drawn);
        assert!(all_drawn, "the whole small ride is near the player");
        // The player walks away: every group is removed.
        host.players[0].pos = DVec3::new(500.0, 70.0, 0.0);
        rt.tick(&mut host);
        let cmds = host.take_commands();
        let kills = cmds
            .iter()
            .filter(|c| c.starts_with("kill @e[tag=htp_g_a_"))
            .count();
        assert_eq!(kills, rt.rides[0].groups.len() + 1, "groups and the train");
        assert!(rt.rides[0].groups.iter().all(|g| !g.drawn) && !rt.rides[0].train_drawn);
        // A player still far away: nothing is drawn.
        host.players[0].pos = DVec3::new(10.0, 70.0, 10.0 + 150.0);
        rt.tick(&mut host);
        assert!(rt.rides[0].groups.iter().all(|g| !g.drawn));
        // Back within 80: drawn again.
        host.players[0].pos = DVec3::new(10.0, 70.0, 10.0);
        for _ in 0..4 {
            rt.tick(&mut host);
        }
        assert!(rt.rides[0].groups.iter().all(|g| g.drawn) && rt.rides[0].train_drawn);
    }

    #[test]
    fn a_group_between_the_two_distances_keeps_its_state() {
        let (mut rt, mut host, _) = world_with(
            "hysteresis",
            &[("a", DVec3::new(0.0, 70.0, 0.0))],
            DVec3::new(10.0, 70.0, 10.0),
        );
        for _ in 0..12 {
            rt.tick(&mut host);
        }
        // Move to a spot where the nearest group is 80 to 96 away: still drawn.
        let g = &rt.rides[0].groups[0];
        let away = g.center + DVec3::new(0.0, 0.0, g.radius + 88.0);
        host.players[0].pos = away;
        rt.tick(&mut host);
        assert!(rt.rides[0].groups[0].drawn);
    }

    // Test 12.
    #[test]
    fn a_redraw_bumps_the_generation_and_old_generations_near_a_player_are_swept() {
        let (mut rt, mut host, _) = world_with(
            "generation",
            &[("a", DVec3::new(0.0, 70.0, 0.0))],
            DVec3::new(10.0, 70.0, 10.0),
        );
        for _ in 0..12 {
            rt.tick(&mut host);
        }
        assert_eq!(rt.rides[0].groups[0].generation, 1);
        host.players[0].pos = DVec3::new(900.0, 70.0, 0.0);
        rt.tick(&mut host);
        host.players[0].pos = DVec3::new(10.0, 70.0, 10.0);
        for _ in 0..6 {
            rt.tick(&mut host);
        }
        let g = &rt.rides[0].groups[0];
        assert_eq!(g.generation, 2);
        let cmds = host.take_commands();
        assert!(
            cmds.iter()
                .any(|c| c.contains("htp_gen_2") && c.starts_with("summon"))
        );
        // On the next sweep, anything of this group that is not generation 2 is killed near the player.
        while rt.now % SCRUB_EVERY != 0 {
            rt.tick(&mut host);
        }
        let sweep = host.take_commands();
        assert!(
            sweep
                .iter()
                .any(|c| c.contains("kill @e[tag=htp_g_a_0,tag=!htp_gen_2,distance=..128]")),
            "{sweep:?}"
        );
        assert!(
            sweep
                .iter()
                .any(|c| c.contains("kill @e[tag=htp,tag=!htp_s1,distance=..128]"))
        );
    }

    // Test 13.
    #[test]
    fn two_rides_and_two_players_far_apart_never_pass_the_display_budget() {
        let (mut rt, mut host, _) = world_with(
            "budget",
            &[
                ("one", DVec3::new(0.0, 70.0, 0.0)),
                ("two", DVec3::new(3000.0, 70.0, 0.0)),
            ],
            DVec3::new(10.0, 70.0, 10.0),
        );
        host.players.push(player(2, DVec3::new(3010.0, 70.0, 10.0)));
        let count_near = |rt: &Runtime, p: DVec3| -> usize {
            rt.rides
                .iter()
                .map(|r| {
                    r.groups
                        .iter()
                        .filter(|g| g.drawn && g.center.distance(p) - g.radius <= BUDGET_RADIUS)
                        .map(|g| g.end - g.first)
                        .sum::<usize>()
                })
                .sum()
        };
        for _ in 0..30 {
            rt.tick(&mut host);
            for p in &host.players {
                assert!(count_near(&rt, p.pos) <= DISPLAY_BUDGET);
            }
        }
        // Each player has their own ride drawn, and not the other one.
        assert!(rt.rides[0].groups.iter().all(|g| g.drawn));
        assert!(rt.rides[1].groups.iter().all(|g| g.drawn));
        // With a tiny budget the draw stops and the player is warned once.
        let (mut rt, mut host, _) = world_with(
            "budget2",
            &[("one", DVec3::new(0.0, 70.0, 0.0))],
            DVec3::new(10.0, 70.0, 10.0),
        );
        rt.display_budget = 20;
        for _ in 0..30 {
            rt.tick(&mut host);
        }
        let drawn: usize = rt.rides[0]
            .groups
            .iter()
            .filter(|g| g.drawn)
            .map(|g| g.end - g.first)
            .sum();
        assert!(drawn <= 20, "{drawn}");
        assert_eq!(
            host.broadcasts
                .iter()
                .filter(|b| b.contains("too many rides"))
                .count(),
            1
        );
    }

    #[test]
    fn a_parked_ride_keeps_running_its_physics() {
        let (mut rt, mut host, _) = world_with(
            "parked-physics",
            &[("a", DVec3::new(0.0, 70.0, 0.0))],
            far_away(),
        );
        rt.tick(&mut host);
        rt.rides[0].train.start_countdown(0.0);
        for _ in 0..200 {
            rt.tick(&mut host);
        }
        assert!(!rt.rides[0].train.is_held() || rt.rides[0].train.s > 0.0);
        assert!(rt.rides[0].train.v > 0.0 || rt.rides[0].train.s > 0.0);
        assert!(
            host.take_commands()
                .iter()
                .all(|c| !c.starts_with("summon") && !c.starts_with("tp "))
        );
    }

    #[test]
    fn the_wand_command_gives_the_item_and_the_usages_cover_every_command() {
        let (mut rt, mut host, _) = world_with("give", &[], bystander());
        rt.tick(&mut host);
        host.take_commands();
        assert!(
            rt.command("coaster wand", caller())
                .unwrap()
                .contains("track wand")
        );
        rt.tick(&mut host);
        assert_eq!(host.take_commands()[0], "give Player1 htp:wand");
        assert_eq!(COMMAND_USAGES.len(), 11);
        assert!(COMMAND_USAGES.iter().all(|u| u.starts_with("coaster ")));
    }
}
