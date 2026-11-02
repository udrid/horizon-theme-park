//! park-core: the game-free brain of Horizon Theme Park.
//!
//! This crate must never depend on Horizon or Minecraft, so it can be
//! tested in seconds anywhere. Coaster maths lives in `coaster`, the "door" to the
//! game in `host`, the exact command texts in `commands`, what to draw in `render`
//! and the per-tick loop in `runtime`.

pub mod builder;
pub mod coaster;
pub mod commands;
pub mod finish;
pub mod host;
pub mod pieces;
pub mod preview;
pub mod render;
pub mod report;
pub mod runtime;
pub mod save;
pub mod welcome;
