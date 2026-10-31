# Question for the free reader (`agy`)

**How to use:** open a terminal in the `horizon` folder (next to `horizon-theme-park`), start `agy`, and paste the
block below. Then paste its answer back to me. It reads only. Nothing here changes any file.

The commit I need read is `b5b451de7c9d8d3dfc851c9405d4754724205e1b` (hook API 21). It's in the clone's history but
not checked out, so agy must read files with `git show <commit>:<path>`, not from the folder.

```
You are helping me read a Rust codebase (Horizon, a Minecraft rewrite). Read-only: change nothing.
Use `git show b5b451de7c9d8d3dfc851c9405d4754724205e1b:<path>` for every file, because that commit is not checked out.
Answer each question in order. For every claim, quote the code (at most 25 lines per quote) with file path and line number.
If you can't find something, say "not found" instead of guessing. Keep each answer under 200 words plus quotes.

Background: a display entity (item_display) is created by `/summon` and stored in `Game.others: Vec<OtherEntity>`
(`crates/horizon-client/src/game.rs` and `crates/horizon-client/src/game/commands.rs`). The commands
`/tp @e[tag=x,limit=1] ...` work but scan every entity. `/tp <uuid> ...` does not find displays.

Q1. What is `OtherEntity` (its fields), and which function creates it when a display is summoned? Is the UUID given in the
    summon NBT (`UUID:[I;a,b,c,d]`) stored anywhere that a lookup by UUID could use (for example `persist.uuids`
    or `persist_raw`)?
Q2. `target_with_uuid` in `crates/horizon-client/src/game/commands/execute.rs` (around line 771): why does it return
    None for a display summoned with an explicit UUID, and for a pig summoned with one? Which data would it need to
    also check, and what is the smallest change (in lines) that would make it find any entity in `self.others`,
    `self.armor_stands` and `self.mobs`?
Q3. `teleport_to_in_level` in `crates/horizon-client/src/game/commands.rs` has a `Target::Other(id)` arm that sets
    `o.position` and yaw/pitch. After that, how does the server tell each connected guest player (and the owner in a
    single-player world) that the display moved? Name the function that diffs or sends entity positions
    (look in `crates/horizon-client/src/game/lan_entities.rs`), and say whether it runs every tick.
Q4. Visibility rules: the function `teleport_to` is a private method of `Game` defined in `commands.rs`. A module
    injected by horizon-mixins with `[[module]] file = "crates/horizon-client/src/game/commands.rs"` would be a child
    module of `game::commands`. Would it be able to call `teleport_to`, `target_with_uuid` and read `self.others`?
    List any item among those that is NOT visible from such a child module and why.
Q5. Velocity of a hosted player. In an integrated (single-player) server the owner is a hosted player and
    `Game.serving` is Some. In `native_server_tick` (the tick hook) no player's state is swapped in. What is the
    smallest way, from code placed in `crates/horizon-client/src/game.rs`'s child module, to give a player (found by
    UUID or name) a velocity in blocks per tick and have their client receive it? Look at `send_guest_motion` in
    `crates/horizon-client/src/game/lan_mobs.rs` and at how `mace.rs` sets `p.velocity` and `p.health.sync_velocity`.
    Say which of the two paths applies to the owner of a single-player world, and how to get the `conn` number or
    player state from a UUID.
Q6. Is there any existing, stable function for a mod to read a hosted player's position and whether they are riding
    something, from `native_server_tick`? (`native_players`, `native_acting_player`, anything similar.)
```

## What I'll do with the answer
- Q1–Q4 tell us whether a private "move this display by its UUID" call is small, and where it would live
  (`host_mixin.rs`, D7). I'd bring it to you as a choice before anything is built.
- Q2 might also turn the Horizon issue into a precise two-line fix suggestion.
- Q5–Q6 answer the fling-off (D22/D28) for single-player, which I read only partly.
