// htp_glue: the thin layer between Horizon and park-core.
// It is added to the game as the module `crate::game::htp_glue`.
//
// The glue only PASSES THINGS ALONG. Everything that decides what happens (the greeting, the
// `/coaster` commands, the train, the exact command texts) lives in park-core, where
// `cargo test` checks it. Here we do three small jobs:
//   1. keep ONE `Runtime` for the whole game (behind a lock);
//   2. show park-core the game through the `Host` "door" (players, run a command, chat);
//   3. hand the game's news items (world opened or closing, tick, register commands, a command
//      was typed, the track wand used) to the runtime.
//
// RULE: nothing in here may panic (crash). A panic inside a hook can take the whole world down.
// So there is no `unwrap()` and no `expect()`.

use crate::game::Game;
use crate::game::htp_park::commands::WAND_ITEM;
use crate::game::htp_park::host::{Host, PlayerInfo};
use crate::game::htp_park::runtime::{COMMAND_USAGES, Caller, Runtime, WandClick};
use crate::game::native_lifecycle::{ServerTickPhase, WorldPhase};
use crate::game::native_server::{NativeCommand, NativeServerPlayer};
use glam::DVec3;
use horizon_client::items::ItemStack;
use horizon_client::text::Text;
use std::path::PathBuf;
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

    fn action_bar(&mut self, player: u128, text: &str) -> bool {
        // The line over the player's hotbar (hook API 21 helper, stable). False if they are not on.
        self.0
            .native_action_bar(player, Text::literal(text.to_string()))
    }

    fn log(&mut self, text: &str) {
        // The game's log only (logs/latest.log); nothing in chat.
        log::info!("{text}");
    }

    fn world_dir(&self) -> Option<PathBuf> {
        // The open world's folder, where the rides are saved (None when no world is open).
        self.0.native_world_dir()
    }
}

/// Called from the world-lifecycle hook (see on_world.rs).
pub(super) fn world(game: &mut Game<'_>, phase: WorldPhase) {
    match phase {
        WorldPhase::WorldOpenReady => {
            // In single-player nobody is "placed" in the world yet at this moment, so a chat line
            // would not arrive. We only log it, and tell park-core a new world has started.
            // The world folder is logged too: the first build checks that it is already known here (F5).
            log::info!(
                "Horizon Theme Park: world open ready, world folder = {:?}",
                game.native_world_dir()
            );
            runtime().world_opened();
        }
        WorldPhase::WorldCloseFinal => {
            // Save anything not saved yet. Rides are also saved right after every change.
            runtime().world_closing(&mut GameHost(game));
        }
        _ => {}
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
        name: player.name.clone(),
        pos: DVec3::new(player.position[0], player.position[1], player.position[2]),
    };
    if let Some(answer) = runtime().command(line, caller) {
        *ran = true;
        game.native_feedback(Text::literal(answer));
    }
}

/// Called when a player right-clicks a BLOCK with an item (see on_use_item_on.rs). Only the track
/// wand is claimed (so it places no block and opens nothing); every other item is left alone.
/// Commands are refused inside this hook, so park-core only queues the click for the next tick.
pub(super) fn use_item_on(
    game: &mut Game<'_>,
    stack: &ItemStack,
    pos: [i32; 3],
    claimed: &mut bool,
) {
    if stack.item.to_string() != WAND_ITEM {
        return;
    }
    *claimed = true;
    if let Some(player) = game.native_acting_player() {
        runtime().wand_click(player.uuid, WandClick::Block(pos));
    }
}

/// Called when a player breaks a block (see on_destroy_block.rs). With the track wand in hand the
/// break is cancelled and counts as "undo the last node" (D67); anything else is left alone.
pub(super) fn destroy_block(game: &mut Game<'_>, canceled: &mut bool) {
    let holds_wand = game
        .native_held()
        .is_some_and(|(item, _, _)| item == WAND_ITEM);
    if !holds_wand {
        return;
    }
    *canceled = true;
    if let Some(player) = game.native_acting_player() {
        runtime().wand_undo(player.uuid);
    }
}

/// Called when a player right-clicks the AIR with an item (see on_use_item.rs). Same rules as above.
pub(super) fn use_item(game: &mut Game<'_>, stack: &ItemStack, claimed: &mut bool) {
    if stack.item.to_string() != WAND_ITEM {
        return;
    }
    *claimed = true;
    if let Some(player) = game.native_acting_player() {
        runtime().wand_click(player.uuid, WandClick::Air);
    }
}
