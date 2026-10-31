# Milestones

Each milestone ends in a video. A milestone is **done** only when `cargo test --workspace` passes,
the mod builds in the launcher (the owner runs that build), and the video is recorded. Before
building one, an Opus session writes `docs/milestones/Mx.md` (tasks, tests, open choices) and the
owner approves its design choices ([WORKING_WITH_ME.md](WORKING_WITH_ME.md)). Architecture:
[ARCHITECTURE.md](ARCHITECTURE.md). Decisions: [DECISIONS.md](DECISIONS.md).

The engine spike (prompt 03) is **done** (2026-10-30). Results are in [SPIKE_RESULTS.md](SPIKE_RESULTS.md) and
D23–D32. M1's plan builds on them.

---

## M0: Foundations

- **Scope:** repo, CI (fmt, clippy, test), `park-core` and `park-sim` skeletons *(done 2026-10-30)*;
  `tools/park-bundle` with `--check` in CI; native mod skeleton (`horizon-mixin.toml`, `glue.rs`,
  glued `park.rs`) on hook API 21 that broadcasts "Horizon Theme Park loaded" when the first player is placed in a world [D35].
- **Out of scope:** any coaster code, displays, content mod.
- **Tests:** `park-bundle --check` passes; a unit test that the glued file contains no `include!`,
  `#[path]` or leftover `crate::coaster` paths; CI green.
- **Video:** none. A short dev-log post: the repo, CI going green, the "loaded" line in chat.

## M1: First ride

- **Scope:** closed demo oval (59.7 blocks) built by `/coaster demo`; spline, arc-length table, frames; adaptive rails; one car with two minecart seats; board by right-click; the train waits at the start while empty, runs while anyone rides, and finishes its lap and stops after the last rider gets off; `/coaster ride` is only a hint; content mod `htp`.
- **Out of scope:** gravity, elements, building tools, saving, multiple cars, parking.
- **Tests:** the 12 tests of the M1 plan (`docs/milestones/M1.md`), plus a rider-height check.
- **Video, "I made a roller coaster in Rust Minecraft":**
  1. Fly-over of the oval track.
  2. Typing `/coaster demo` and right-clicking a seat.
  3. Full POV lap.
  4. Side shot of the car gliding past.
  5. Close-up of the rails curving.

## M2: Gravity

- **Scope:** physics from ARCHITECTURE §4 (gravity, friction, drag, 10 substeps); elements Station,
  Lift, Brake; one hard-coded layout with a lift hill, first drop, hills, brake run and station stop;
  speed shown on the action bar (km/h); `park-sim` prints a speed / g CSV.
- **Out of scope:** BlockBrake, Booster, multiple trains, building, saving, HUD meter, banking.
- **Tests:** all seven physics tests in ARCHITECTURE §4; lift: train never rolls back; brake: leaves
  the brake run at `target_v` ± 0.1; station: train stops within 0.2 blocks of the stop point.
- **Video:**
  1. Lift-hill climb with the click sound (if any; sounds aren't a hook yet).
  2. First-drop POV with the speed overlay.
  3. Back-row vs front-row comparison over the top.
  4. Brake run stop.
  5. A `park-sim` speed graph on screen.

## M3: Builder

- **Scope:** track wand item (`native_use_item_on` / `native_use_item`); node blocks
  (`native_place_block` / `native_destroy_block`); particle preview of the curve; `/coaster create |
  delete | list | ride | set <setting> <value>`; saving and loading per ARCHITECTURE §7; parking of far
  rides.
- **Out of scope:** multiple trains, block sections, presets, browser designer.
- **Tests:** save → load round trip gives an equal ride; a newer `format` is refused and not
  overwritten; a `.tmp` left over from a crash is ignored and `.bak` survives; parking: `FakeHost`
  sees `remove_tagged` when the last player walks past 128 blocks, and at most 50 spawns per tick on
  return; orders queued in `native_perform_command` run on the next tick.
- **Video, "build-a-coaster timelapse":**
  1. Empty field.
  2. Placing nodes with the wand.
  3. Particle preview.
  4. `/coaster create`.
  5. First ride.
  6. Quit and reload, and the coaster is still there.

## M4: Trains & safety

- **Scope:** multi-car trains (rigid, weight spread); 2 seats per car; block sections and BlockBrake;
  two trains on one ride; station dispatch panel (block entity + custom screen, API 15–19).
- **Out of scope:** inversions, presets, HUD, ratings.
- **Tests:** two trains on a closed ride never share a section over 10,000 simulated ticks; a train
  held at a block brake restarts when the section clears; dispatch from the panel is queued and runs
  on tick; the 6-car hill-crest test; a rider off mid-ride gets one `launch_player` with their
  seat's velocity, while one off in the station gets none [D22].
- **Video:**
  1. Two trains running.
  2. A near-miss, held by a block brake (split screen).
  3. Using the dispatch panel.
  4. Riding with a friend (LAN).

## M5: Elements & feel

- **Scope:** banking (roll) and heartline; presets (loop, corkscrew, camelback, helix); Booster/LSM
  launch; g-force HUD (`native_hud`, smoothed); ride stats and our own ratings (formulas approved
  first).
- **Out of scope:** browser designer, flat rides.
- **Tests:** loop preset produces a closed, flip-free frame set; heartline roll keeps the rider
  point's lateral g under 0.2 g through a corkscrew at design speed; booster adds `force · dt` per
  substep; ratings are deterministic, and their formulas are written in `docs/` with sample values.
- **Video:**
  1. LSM launch.
  2. Inversions POV.
  3. HUD g-meter close-up.
  4. Stats card after the ride.

## M6: Web designer

- **Scope:** `park-web` (park-core to WASM, three.js previewer) on GitHub Pages; export and import ride
  files between browser and game; NoLimits 2 CSV import.
- **Out of scope:** editing every setting in the browser; multiplayer.
- **Tests:** WASM build in CI; the same ride gives identical stats in `park-sim` and `park-web`; a
  sample NoLimits CSV imports to a ride within 0.1 block of its points.
- **Video, "designed in my browser, ridden in Minecraft":**
  1. Designing in the browser.
  2. Exporting the file.
  3. Dropping it into the world folder.
  4. Riding it in game.

## M7: Theme park kit

- **Scope:** the `rides/` and `park/` modules; flat rides on the same animated-rig system (carousel,
  drop tower, Ferris wheel); queue lines; decor pack (blocks); ticket booth (server menus, API 10);
  the packed-park entity budget tuned from the spike's numbers [D18, D19].
- **Out of scope:** guests, economy beyond tickets.
- **Tests:** each flat ride's motion is deterministic and its seats stay attached; with 5 coasters
  and 10 flat rides, `FakeHost` counts never exceed the budget near any one player; ticket booth trade
  goes through `native_exchange_items`.
- **Video, park tour:**
  1. Walk the main path.
  2. Each ride for a few seconds.
  3. Buying a ticket.
  4. Aerial of the whole park.

## M8: Park life (stretch)

- **Scope:** park stats; guest NPCs *or* a multiplayer opening day on a hosted world (pick one with
  the owner at M8 planning).
- **Out of scope:** anything needing Horizon dedicated servers to build native mods (not possible yet).
- **Tests:** decided at M8 planning.
- **Video:** opening day with friends.
