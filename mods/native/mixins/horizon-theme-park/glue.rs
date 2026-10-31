// htp_glue: the thin layer between Horizon and park-core.
// It is added to the game as the module `crate::game::htp_glue`.
//
// The glue only PASSES THINGS ALONG. Everything that decides what happens (the greeting, the
// `/coaster` commands, the train, the exact command texts) lives in park-core, where
// `cargo test` checks it. Here we do three small jobs:
//   1. keep ONE `Runtime` for the whole game (behind a lock);
//   2. show park-core the game through the `Host` "door" (players, run a command, chat);
//   3. hand the game's four news items (world opened, tick, register commands, a command
//      was typed) to the runtime.
//
// RULE: nothing in here may panic (crash). A panic inside a hook can take the whole world down.
// So there is no `unwrap()` and no `expect()`.

use crate::game::Game;
use crate::game::htp_park::host::{Host, PlayerInfo};
use crate::game::htp_park::runtime::{COMMAND_USAGES, Caller, Runtime};
use crate::game::native_lifecycle::{ServerTickPhase, WorldPhase};
use crate::game::native_server::{NativeCommand, NativeServerPlayer};
use glam::DVec3;
use horizon_client::text::Text;
use std::sync::{Mutex, MutexGuard};

// One `Runtime` for the whole game, behind a lock (a "Mutex"): only one piece of code can use it
// at a time. `Runtime::new()` is a `const fn`, so it can start out right here.
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime::new());

/// Take the lock. If some other code panicked while holding it (a "poisoned" lock), use the
/// data anyway instead of crashing: a train is not worth bringing the world down.
fn runtime() -> MutexGuard<'static, Runtime> {
    RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The game, seen through park-core's `Host` door.
struct GameHost<'g, 'a>(&'g mut Game<'a>);

impl Host for GameHost<'_, '_> {
    fn players(&self) -> Vec<PlayerInfo> {
        // "Placed" players have fully arrived in the world; only they receive chat.
        self.0
            .native_server_players()
            .into_iter()
            .map(|p| PlayerInfo {
                uuid: p.uuid,
                name: p.name,
                pos: DVec3::new(p.position[0], p.position[1], p.position[2]),
                dimension: p.dimension,
            })
            .collect()
    }

    fn run(&mut self, command: &str) -> bool {
        // Runs as the server's console, at the start of the next tick. True = it did not fail.
        self.0.native_run_command(command)
    }

    fn broadcast(&mut self, text: &str) {
        // Chat for every placed player, and the log.
        self.0.native_broadcast(Text::literal(text.to_string()));
    }

    fn tell(&mut self, player: u128, text: &str) {
        let _ = self.0.native_tell(player, Text::literal(text.to_string()));
    }

    fn log(&mut self, text: &str) {
        // The game's log only (logs/latest.log); nothing in chat.
        log::info!("{text}");
    }
}

/// Called from the world-lifecycle hook (see on_world.rs).
pub(super) fn world(phase: WorldPhase) {
    if matches!(phase, WorldPhase::WorldOpenReady) {
        // In single-player nobody is "placed" in the world yet at this moment, so a chat line
        // would not arrive. We only log it, and tell park-core a new world has started.
        log::info!("Horizon Theme Park: world open ready");
        runtime().world_opened();
    }
}

/// Called from the server-tick hook (see on_tick.rs).
pub(super) fn tick(game: &mut Game<'_>, phase: ServerTickPhase) {
    // The hook fires at the start and at the end of a tick; we only act at the end.
    if !matches!(phase, ServerTickPhase::ServerTickEnd { .. }) {
        return;
    }
    runtime().tick(&mut GameHost(game));
}

/// Called when the game asks which commands mods add (see on_commands.rs).
pub(super) fn register(commands: &mut Vec<NativeCommand>) {
    for usage in COMMAND_USAGES {
        // `NativeCommand::new` is a usage every player may run.
        commands.push(NativeCommand::new(usage));
    }
}

/// Called when a player typed a command a mod registered (see on_command.rs).
/// Commands cannot run inside this hook, so park-core only queues the order for the next tick;
/// we just answer the player.
pub(super) fn command(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    line: &str,
    ran: &mut bool,
) {
    // Who typed it and where they stand. (The demo is built to their east, so we never need
    // to read which way they look: that would be a private Horizon field, D52.)
    let caller = Caller {
        uuid: player.uuid,
        pos: DVec3::new(player.position[0], player.position[1], player.position[2]),
    };
    if let Some(answer) = runtime().command(line, caller) {
        *ran = true;
        game.native_feedback(Text::literal(answer));
    }
}
