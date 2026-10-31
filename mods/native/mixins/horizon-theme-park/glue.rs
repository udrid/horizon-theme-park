// htp_glue: the thin layer between Horizon and park-core.
// It is added to the game as the module `crate::game::htp_glue`.
//
// The glue only REPORTS FACTS ("a world opened", "is a player in the world?").
// Deciding when to say "Horizon Theme Park loaded" lives in park-core (welcome.rs),
// where `cargo test` checks it. (Decision D35.)
//
// RULE: nothing in here may panic (crash). A panic inside a hook can take the whole world down.
// So there is no `unwrap()` and no `expect()`.

use crate::game::Game;
use crate::game::htp_park::welcome::Welcome;
use crate::game::native_lifecycle::{ServerTickPhase, WorldPhase};
use horizon_client::text::Text;
use std::sync::{Mutex, MutexGuard};

// One `Welcome` for the whole game, behind a lock (a "Mutex"): only one piece of code
// can use it at a time. `Welcome::new()` is a `const fn`, so it can start out right here.
static WELCOME: Mutex<Welcome> = Mutex::new(Welcome::new());

/// Take the lock. If some other code panicked while holding it (a "poisoned" lock), use the
/// data anyway instead of crashing: a greeting is not worth bringing the world down.
fn welcome_state() -> MutexGuard<'static, Welcome> {
    WELCOME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Called from the world-lifecycle hook (see on_world.rs).
pub(super) fn world(phase: WorldPhase) {
    if matches!(phase, WorldPhase::WorldOpenReady) {
        // In single-player nobody is "placed" in the world yet at this moment, so a chat line
        // would not arrive. We only log it, and tell park-core a new world has started.
        log::info!("Horizon Theme Park: world open ready");
        welcome_state().world_opened();
    }
}

/// Called from the server-tick hook (see on_tick.rs).
pub(super) fn tick(game: &Game<'_>, phase: ServerTickPhase) {
    // The hook fires at the start and at the end of a tick; we only act at the end.
    if !matches!(phase, ServerTickPhase::ServerTickEnd { .. }) {
        return;
    }

    let mut welcome = welcome_state();
    // Once the greeting has been said, skip reading the player list on every tick.
    if !welcome.waiting() {
        return;
    }

    // "Placed" players have fully arrived in the world; only they receive chat.
    let someone_here = !game.native_server_players().is_empty();
    if let Some(line) = welcome.tick(someone_here) {
        // Writes to chat for every placed player, and to the log.
        game.native_broadcast(Text::literal(line.to_string()));
    }
}
