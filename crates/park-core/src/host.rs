//! The "game door" (D43): everything park-core may ask of the game, and nothing else.
//!
//! park-core never touches Minecraft. It only calls the four small methods of `Host`, and the
//! glue next to the mod passes them on to Horizon. In tests, `FakeHost` stands in for the game
//! and simply writes down everything it is asked, so a test can check the exact list.
//!
//! Later, private fallbacks (for example "launch a player", D28) are added here as extra methods
//! with a default that says "not available", so this stays the only door [D7].

use glam::DVec3;
use std::path::PathBuf;

/// What park-core knows about one player who is fully in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub uuid: u128,
    pub name: String,
    pub pos: DVec3,
    pub dimension: String,
}

/// The game, as park-core sees it.
pub trait Host {
    /// Players who have fully arrived in the world ("placed" players only).
    fn players(&self) -> Vec<PlayerInfo>;
    /// Run a console command (no leading slash). `true` means it ran without failing.
    fn run(&mut self, command: &str) -> bool;
    /// A line of chat for everyone, also written to the log.
    fn broadcast(&mut self, text: &str);
    /// A line of chat for one player.
    fn tell(&mut self, player: u128, text: &str);
    /// A line for the game's log only (nobody sees it in chat). Optional: the default drops it.
    fn log(&mut self, _text: &str) {}
    /// Show a line on one player's action bar (above the hotbar). Optional: the default says "not
    /// available" (`false`) and park-core then falls back to the `title ... actionbar` command.
    /// The glue answers this with Horizon's API 21 helper `native_action_bar` (D43's "extra method" rule).
    fn action_bar(&mut self, _player: u128, _text: &str) -> bool {
        false
    }
    /// The open world's folder (F5), where rides are saved. `None` when no world is open (or the
    /// host does not know); nothing is saved then.
    fn world_dir(&self) -> Option<PathBuf> {
        None
    }
}

/// A pretend game for tests: it records every call, in order.
#[derive(Clone, Debug, Default)]
pub struct FakeHost {
    /// The players the pretend world contains. Tests change this between ticks.
    pub players: Vec<PlayerInfo>,
    /// Every command that was run, in order.
    pub commands: Vec<String>,
    /// A command starting with any of these texts "fails" (returns `false`).
    pub failing_prefixes: Vec<String>,
    /// Every broadcast line.
    pub broadcasts: Vec<String>,
    /// Every private line, as (player, text).
    pub tells: Vec<(u128, String)>,
    /// Every log line.
    pub logs: Vec<String>,
    /// Every action-bar line, as (player, text).
    pub action_bars: Vec<(u128, String)>,
    /// The world folder the pretend game reports (`None` = no folder).
    pub world: Option<PathBuf>,
}

impl FakeHost {
    /// An empty pretend world.
    pub fn new() -> FakeHost {
        FakeHost::default()
    }

    /// Forget the recorded commands (keeps the players), so a test can look at one tick at a time.
    pub fn take_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.commands)
    }
}

impl Host for FakeHost {
    fn players(&self) -> Vec<PlayerInfo> {
        self.players.clone()
    }

    fn run(&mut self, command: &str) -> bool {
        self.commands.push(command.to_string());
        !self
            .failing_prefixes
            .iter()
            .any(|prefix| command.starts_with(prefix.as_str()))
    }

    fn broadcast(&mut self, text: &str) {
        self.broadcasts.push(text.to_string());
    }

    fn tell(&mut self, player: u128, text: &str) {
        self.tells.push((player, text.to_string()));
    }

    fn log(&mut self, text: &str) {
        self.logs.push(text.to_string());
    }

    fn action_bar(&mut self, player: u128, text: &str) -> bool {
        self.action_bars.push((player, text.to_string()));
        true
    }

    fn world_dir(&self) -> Option<PathBuf> {
        self.world.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_host_records_commands_in_order() {
        let mut host = FakeHost::new();
        assert!(host.run("say one"));
        assert!(host.run("say two"));
        assert_eq!(host.commands, vec!["say one", "say two"]);
        assert_eq!(host.take_commands().len(), 2);
        assert!(host.commands.is_empty());
    }

    #[test]
    fn a_command_can_be_made_to_fail() {
        let mut host = FakeHost::new();
        host.failing_prefixes.push("tp ".to_string());
        assert!(!host.run("tp @s 1 2 3"));
        assert!(host.run("summon pig"));
        // Failed commands are still written down.
        assert_eq!(host.commands.len(), 2);
    }

    #[test]
    fn chat_lines_are_recorded() {
        let mut host = FakeHost::new();
        host.broadcast("hello");
        host.tell(7, "psst");
        assert_eq!(host.broadcasts, vec!["hello"]);
        assert_eq!(host.tells, vec![(7, "psst".to_string())]);
    }
}
