# Horizon Theme Park

A theme park kit for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 rewritten in Rust). It starts with realistic roller coasters and grows into flat rides, queues, decor and a full park.

Status: just started (setup stage). Nothing to play yet.

- `crates/park-core`: pure Rust logic, no Minecraft code, tested anywhere
- `crates/park-sim`: command-line test tool
- `crates/park-web`: browser previewer (later)
- `mods/content`: blocks and models (`.hmod`)
- `mods/native`: the thin Rust mod that connects the game to `park-core`
- `docs/`: rules, decisions, glossary and milestone plans

Licence: MIT.
