# Engine spike results: Part A (commands only)

**Date:** 2026-10-30. **Status:** Part A done. Part B (native mod) not started: it waits for your "yes".
**Nothing downloaded or built.** Everything ran on the game you already have installed.

## What a spike is

A spike is a short, throwaway experiment that checks the risky parts before we build on them. We list
the risks (R1–R9 in [ARCHITECTURE.md §9](ARCHITECTURE.md)), test each one cheaply, and write down what
we saw. Nothing from a spike goes into the real mod. Our test files are in `spike/` (not committed).

## How the tests were run

- **Game:** your launcher's install, "Nightly 2026-10-30 · 1da354b", started by `horizon-ctl` in a
  **scratch** game home (a temporary folder, not your saves). Creative world `spike`, test terrain,
  with a flat stone platform at y = 199.
- **Moving things every tick without a mod:** a tiny datapack in the scratch world. A *function* (a
  text file of commands) runs, then schedules itself again one tick later. That's the same thing the
  real mod will do once per tick, so the game's work is comparable.
- **Looking at smoothness:** screenshots through `horizon-ctl`. Looking straight down at a floor of
  1-block black/white stripes, a **stepped** camera or display shows the stripe edges at *exactly the
  same pixels* in every shot, while a **smooth** one shows them drifting. I proved the method works with
  a control (normal flying: the edges drifted 433 → 529 → 644 px). The small analysis scripts are in `spike/`.
- **Source reading** was done in `../horizon` (commit `073afd48`, hook API 19). It is **newer** than your
  installed game (`1da354b`, hook API 7, checked in Part B), so I only used it to explain things I had first
  seen in the running game; behaviour may differ on the newer code and should be re-checked on the commit we pin.
  Quoted lines are marked with their file and line.
- **Limits:** one computer, single-player (the game runs its own server inside). A dedicated server may
  differ. The game capped itself at about 30 FPS while its window wasn't in focus, so I report
  **milliseconds per frame** (CPU, GPU) and **server tick time**, which are not hidden by the cap.

## The table

| # | Risk | Result | Evidence (short) |
|---|---|---|---|
| R1 | Ride an `item_display` / an empty one | ❌ **doesn't work**; ⚠️ a **mob** works as a seat | `ride @s mount` said "couldn't start riding" for item, block and text displays, armor stand, interaction, marker, minecart, area_effect_cloud. It worked for a pig and for all 17 mobs I tried (chicken, bat, silverfish, endermite, slime, allay, vex, squid, parrot, rabbit, bee, frog, tadpole, armadillo, cat, villager, zombie). Source: `world_cmds.rs:1190` "Players and mobs can ride mobs", anything else gets `failure.generic`. |
| R2 | A ridden seat moved every tick is smooth | ❌ **stepped (20 steps a second)** | Seat mob moved 1 block/tick by `tp` (7 shots) and by `Motion` (4 shots): stripe edges identical in **every** shot, so the rider's camera only ever sits on whole-tick positions. Control (normal flying) drifted. `teleport_duration` exists only on displays, so a seat can't use it. Reason in source: `/tp` on a mob sets `position` and `previous` to the same spot (`commands.rs:1337–1338`), so there is nothing to blend between. I did not find the camera code that follows the mob (question for the free reader below). |
| R3 | Move (`tp`) and turn stay in step | ✅ **with `tp` yaw/pitch**; ❌ with `data merge` | `data merge entity … {transformation:{left_rotation:…}}` printed "Nothing changed" and did nothing; so did `data modify`, and merging `item` or `view_range`. But `tp @e … X Y Z <yaw> <pitch>` turns the display (screen angle 44.8° for yaw 45, 89.8° for 90, −31.3° for −30) on the **same clock** as the position: over 10 shots at `teleport_duration` 1 and 3 the gap between turning and moving was −0.6…0 ticks (my noise level). **No roll** is possible this way, only yaw and pitch. |
| R4 | 1,000 displays and a packed park | ⚠️ **works, tick time is the limit** | See the numbers below. Frame cost is small; server tick time grows about 5 µs per display in the world plus about 6 µs more per display a player is near. Tracking range is real (about 160 blocks). |
| R5 | Our own UUID; `tp`/`kill` by UUID | ⚠️ UUID **kept**, but `tp <uuid>`/`kill <uuid>` **don't find it** | `summon … {UUID:[I;1,2,3,4]}` gives exactly that UUID (`data get entity … UUID` → `[I; 1, 2, 3, 4]`). The hyphenated text `00000001-0000-0002-0000-000300000004` is parsed correctly (`selector.rs:122–144`), but `tp`, `kill`, `data get` and `execute if entity` said "No entity was found" for displays, armor stand, pig, marker and interaction. The player's own UUID works. What does work: `@e[tag=…,limit=1]` and `@e[nbt={UUID:[I;1,2,3,4]},limit=1]`, but they are **slow** (R6 note below). Source: `target_with_uuid` (`execute.rs:771`) only checks the player, then `persist.uuids` and `self.mobs`. |
| R6 | 400 commands per tick under 5 ms | ⚠️ **yes for cheap commands, no for selector commands** | Through the real mod (`native_run_command` in the tick hook): **400 cheap commands = 2.9 ms** (7 µs each, under the 5 ms target). But **400 `tp @e[tag=…,limit=1]` with ~1,000 displays in the world = 437 ms** (1.1 ms each). The mod can read only success/fail, not output. See "Part B results" below. |
| R7 | Notice a sneak-off | ✅ | Holding sneak (`key shift down`) dismounts. While riding: player y = seat y + 0.71875 (same for every mob), and `execute on vehicle run say …` runs as the seat. After sneaking off, the player is at seat height and `execute on vehicle` finds nothing, so the mod can tell by comparing positions. A quick tap of the key did **not** dismount. |
| R8 | `kill @e[tag=htp]` and unloaded chunks | ⚠️ **far ones survive** | 5 tagged displays summoned at x = 400, I flew away until the chunks unloaded, ran `kill @e[tag=htp]` ("No entity was found"), came back: all 5 were still there. Plan: kill by tag again whenever a chunk or ride reloads (D20 already removes `htp` on world open). |
| R9 | Give a rider a velocity | ❌ **no command does** | The command list is vanilla's (plus `/pregen` only). `data merge entity @s {Motion:…}` → "Unable to modify player data". `/damage … by <mob>` does push a player (about 0.12 sideways + 0.19 up per tick, so 2–4 blocks/s, fixed direction, survival only); a wind charge did nothing. And **dismounting throws the speed away**: right after sneaking off a moving seat, velocity was `[0, −0.078, 0]`. So D22's fling needs a private Horizon call (Part B). |

Also checked:

| Item | Result |
|---|---|
| D22 free view | ✅ With a rider on a mob seat, `tp` with yaw, `rotate`, `data merge … Rotation` and moving the seat never changed the rider's yaw 30 / pitch 10. |
| Displays render with rotation + scale | ✅ item_display and block_display with `left_rotation` and `scale` drew correctly (screenshot). |
| Smooth display movement (no rider) | ✅ at `teleport_duration` 1, 2 or 3; ❌ at 0. Display moved 0.25 block/tick: at 0 the gaps between shots were all about 0.50 or 0.75 (whole ticks); at 1 and 3 they varied freely. Lag behind the true position was about N−1 ticks: td 3 sat about 1.6 ticks behind td 1 (theory: 2). |

## Measured numbers

Standing still at the middle of the park, render distance 17, scratch world. "Tick" is the server's
smoothed tick time (budget 50 ms). "Tracked" is how many displays the game currently holds near the player.

| Displays in the world | Tracked near player | Tick ms | Frame CPU ms | Frame GPU ms |
|---:|---:|---:|---:|---:|
| 0 | 0 | 1.0 | 1.07 | 1.72 |
| 1,000 (packed park, random) | 525 | 7.4 | 2.06 | 1.91 |
| 3,000 | 1,655 | 19.9 | 3.84 | 2.91 |
| 6,000 (400 × 400 area, y 205–230) | 3,360 | 38.6 | 6.46 | 2.73 |
| 2,000 **outside** tracking range (185–255 blocks away) | 0 | 10.8 | 1.14 | 3.45 |

- **Lines at render distance 12** (frame cost only, the tick wasn't read then): none 1.14 / 3.40 ms
  (CPU/GPU), 200 displays 1.49 / 3.36, 500 → 1.68 / 3.93, 1,000 → 1.64 / 3.60.
- **FPS:** pinned at 29.7 in every run (the window cap), so it says nothing. CPU + GPU at 6,000 is
  about 9 ms per frame, so the game could do roughly 100+ FPS there.
- **Tracking range:** tracked ÷ total is always about 55%, which is what a 160-block circle covers in a
  380 × 380 square. 2,000 displays beyond that were tracked by nobody and cost no frame time.
- **But they still cost tick time:** about 5 µs each (10.8 ms for 2,000). That supports D19 (remove
  the displays of far rides, don't just hide them).
- **Only 3,447 of my first 6,000 existed** the first time because I summoned them from a corner where the
  rest were in unloaded chunks. The table above comes from the clean runs from the centre.
- **A flying run through the park** read 38–55 ms tick, but it mixes in chunk loading, so I trust the
  standing numbers more.
- **Selector cost:** 1,000 displays, idle 5.65 ms. Plus 400 `tp` by tag each tick: 277.5 ms. By `nbt`
  UUID: 422.8 ms (max 1,068 ms).

Not measured: the **fling distance** (R9) since no command can fling. The vanilla rule of thumb is that
a flying player loses about 9% of sideways speed per tick, which would carry a 20 blocks/s throw
about 10–11 blocks and a 40 blocks/s throw about 20–22 blocks. That is **arithmetic, not a measurement
from this game**, and the cap (about 3.9 blocks/tick) is also unverified. Part B tests it if you approve.

## Recommended values (for you to approve; nothing recorded in DECISIONS.md)

| Question | Recommendation | Why |
|---|---|---|
| `teleport_duration` for cars | **1** | Smooth at 1, no lag. A seat can't interpolate at all, so a car at 2 or 3 would lag 1–2 blocks behind its riders. |
| For seats | Not applicable | Mobs have no such setting; they step once per tick (R2). |
| Rotation | **`tp` yaw + pitch**, not `data merge` | Same clock as the position, proven in step. |
| Per-player display budget (D19) | **1,500 displays within 160 blocks** to start; never more than about 2,000 in the world at once until commands get cheaper | 3,000 displays cost 19.9 ms of the 50 ms tick before a single command runs; the command cost (R6) multiplies by the number of displays in the world. D19's 2,000–3,000 is too high on this computer. |
| 1-block rail spacing (D17) | ⚠️ **Holds**, if parking is enforced | A 300-block coaster is 300 rail displays (about 1.5 ms of tick). Rails never move, so they cost no commands, but they still count in the world total that every selector command scans. |

## Proposals for ARCHITECTURE.md and DECISIONS.md (I have not edited either)

**P1. Seats (D16 can't work as written).** Nothing but a mob can be ridden, and the rider's camera steps.
- **A. Invisible mob seats now** (peaceful-safe ones such as a chicken: hostile ones vanish in Peaceful),
  accepting a 20 Hz camera. Works today; riding feels choppy at high speed (a 20 blocks/s car moves
  1 block between camera updates).
- **B. Ask Horizon maintainers** to let `/ride` take display entities and to blend a rider's camera
  between ticks. Smooth, but we wait on them.
- **C. `host_mixin`**, private calls to place the rider smoothly. Fast to try, but needs your approval (D7),
  pinning to one commit and re-testing after each Horizon update.
- **Recommendation: A for M0–M2** so you get something to see, **B in parallel** (a draft is below),
  **C only if B stalls.**

**P2. Rotation and roll (ARCH §5 `set_rotation`).** Replace the data-merge command with `tp … yaw pitch`.
Roll (banking) can't be sent after summon. Options: (a) rails carry their roll baked in when summoned
(they never move) and cars don't bank; (b) pre-summon banked car variants and swap them, which costs
entities and commands; (c) ask Horizon to fix `data merge` on displays.
**Recommendation: (a) now, (c) in the Discord message.**

**P3. Finding a display by UUID (ARCH §5 `move_display`).** Selector commands scan everything.
- **A. Wait for Horizon** to make `tp <uuid>` find displays (the lookup in `target_by_uuid` skips them).
- **B. Cap the world** so selector cost stays tolerable (about 70 moves per tick at 1,000 displays) and park aggressively.
- **C. `host_mixin`** for per-display moves by internal id (fast, but private).
- **Recommendation: A + B.** C is the safety net if A stalls, and it's Part B's job to price it.

**P4. Fling-off (D22).** Needs C (private call), or a maintainer hook. Part B finds the smallest call.

**P5. D19 budget:** change "first guess 2,000–3,000" to the 1,500 above, once you agree.

## Questions for the free reader (`agy`), if you want to know *why* the camera steps

> In the Horizon repo (`crates/horizon-client/src/game`), when `self.riding` is `Some(mob_id)` and the
> player rides a mob: (1) which function sets the player's position and the camera position each tick
> from the mob, and does the camera interpolate between a "previous" and a "current" position using the
> partial tick? (2) The `/tp` command sets a mob's `previous` equal to its `position`
> (`commands.rs:1337`). Where does the mob's `previous` get set at the start of each mob tick, and does
> a rider use it? Please quote the code with file and line, and say whether `Motion` set by
> `data merge entity` moves a mob that has `NoAI:1b`.

## Draft message for the Horizon maintainers (Discord; **not posted**)

> Hi! I'm building a theme-park mod on native hooks (API 21) and ran an engine spike on nightly
> 2026-10-30 (1da354b). Five things would unblock smooth rides, each with a one-line repro:
> 1. `/ride <player> mount <item_display>` says "couldn't start riding" (only mobs can be vehicles,
>    `cmd_ride` in `world_cmds.rs`). Could displays (or any entity) be vehicles?
> 2. A rider's camera follows a ridden mob in whole-tick steps. A mob moved 1 block/tick by `/tp`
>    gives identical on-screen positions between ticks, so rides look like 20 FPS. Could the camera
>    blend between ticks as it does for normal movement?
> 3. `/tp <uuid>`, `/kill <uuid>` and `data get entity <uuid>` find only players: a summoned
>    `item_display` or armor stand with `UUID:[I;1,2,3,4]` is "No entity was found". `@e[nbt=…]` works but
>    scans everything (400 `tp @e[tag=x,limit=1]` per tick with 1,000 displays took the tick to 277 ms).
> 4. `data merge entity <display> {transformation:{…}}` (also `item`, `view_range`) reports "Nothing
>    changed", so a display can't be rotated after summon except by `/tp` yaw/pitch.
> 5. Is there (or could there be) a way to give a player a velocity, or a hook when a player dismounts?
>    Dismounting zeroes velocity, so a rider can't keep a coaster's speed.
> Happy to share the scripts. Thanks!

## What you learned today

- A **spike** tests the scary parts first. Today it caught three problems (nothing but a mob can be ridden, riders' cameras step, rotation can't be changed after summon) before we wrote any mod code.
- A **tick** is one step of the game's clock (20 a second). Everything we do each tick has to fit in 50 ms, and every display entity eats a little of it even when nobody sees it.
- **Selectors** like `@e[tag=x]` look at every entity. One lookup is fast; 400 of them per tick over 1,000 entities is not. Finding things by exact ID would be fast, but that part of the game doesn't work for displays yet.
- **Smoothness is measurable.** A tiled floor and screenshots told us "stepped" from "smooth" without guessing.
- When a plan meets reality, we change the plan: your decisions D16, D17, D19 and the fling in D22 now need a second look (proposals above).

**Optional exercise:** the test game is still open. Press `T`, then type
`/summon minecraft:item_display ~ ~2 ~ {item:{id:"minecraft:diamond_block",count:1},teleport_duration:3}`
and press Enter. You should see a floating diamond block 2 blocks above you. Now type
`/tp @e[type=minecraft:item_display,limit=1,sort=nearest] ~ ~5 ~` and watch it glide up over a few ticks instead of jumping. Then try
`/ride @s mount @e[type=minecraft:item_display,limit=1]` and read the red "couldn't start riding" message: that is risk R1 in your own hands.
When finished, type `/kill @e[type=minecraft:item_display]`.

## Part B gate (nothing started)

- **Your installed game:** "Nightly 2026-10-30 · 1da354b". Its hook API number **isn't shown anywhere I can read without a build**, so I don't yet know if it's 21 or more. `../horizon` is at `073afd48` (API 19, older than #4610).
- **What updating would mean:** either the launcher installs a newer nightly (a download; I haven't measured its size, so I'd check it before asking you), or the `../horizon` clone is brought up to date (a small git download, but I'll ask first).
- **What the first native build downloads:** Rust toolchain about 1.7 GB on Windows, crates about 650 MB, about 8 minutes on your computer (D5). Your launcher's `rust install` was known to be broken, and CLAUDE.md says Cargo came from rustup, so some of this may already be on your disk; I'd check before you say yes.
- **Because P1/P3 may change the plan**, Part B should wait for your choices there. I have not read or built anything for it.

## R9 follow-up: the smallest private call for the fling-off (not built, not approved)

Read at Horizon commit **`b5b451de7c9d8d3dfc851c9405d4754724205e1b`** (hook API 21, the merge of #4610). Fetched
into `../horizon` as one commit (about 2 MB); your working files were not changed.

- **The call:** inside Horizon's `Game`, set the player's velocity directly:
  `p.velocity = Vec3::new(x, y, z)` (blocks per **tick**) where `p = self.player.as_mut()?`. This is exactly what
  Horizon's own mace code does (`mace.rs:247–248`) and what `/tp` resets to zero (`commands.rs:1325`).
- **Telling a server player's client:** the same code sets `p.health.sync_velocity = true` with the comment that a
  server player's client is told through `ClientboundSetEntityMotion` (`mace.rs:251–253`). For a guest on a
  hosted world, `send_guest_motion(host, conn, Some(v))` in `lan_mobs.rs:328` sends the packet.
- **Order matters:** the dismount itself leaves velocity at about zero (R9 measurement), so the mod must unseat
  the rider first and set the velocity afterwards, in the same tick.
- **Not tested.** I have only read it. It touches private fields (`velocity`, `health.sync_velocity`), so it belongs
  in `host_mixin.rs`, pinned to one commit (D7, D28), and needs your approval before anyone builds it.
- **Conversion:** D22's seat velocity is in blocks/s; the game wants blocks/tick, so divide by 20
  (20 blocks/s = 1.0, 40 blocks/s = 2.0).
- **Pin:** `horizon_revision = "b5b451de7c9d8d3dfc851c9405d4754724205e1b"`, which is also what Part B's mod would pin.

## Part B preparation (from source at `b5b451de`; not yet run)

- **Reading a command's answer (R6):** `native_run_command` returns only a yes/no ("whether it ran without failing")
  and prints no output (`native_server.rs:329–338`). So the mod can tell success from failure, but cannot read text
  such as an entity's position. The spike mod uses that yes/no to test `tp <uuid>` against `tp @e[tag=…]`.
- **Hosted worlds:** the source suggested `/publish` might be needed. **Wrong in practice:** in single-player the game
  already runs a host (the diagnostic printed `lan present: true`), so the tick hook could run commands without `/publish`.
  The refusal I saw first was my own bad probe command (`time query daytime` isn't valid in 26.3).
- **The mod:** `spike/native-spike/` (three short files, pinned to `b5b451de`, hooks 21). The code was type-checked
  against stand-ins with the Rust you already have; the real build can still find a mismatch.
- **Launcher Rust:** `horizon-launcher rust status` says "Rust 1.99.1 doesn't start": the launcher's `rust` folder holds
  only MinGW, no `rustc`/`cargo`. `rust install` has to run first.

## Part B results (built and run; scratch world, nothing of yours touched)

**Build:** the mod compiled against the real game at `b5b451de` (first build 836 s; rebuilds 6 min 16 s each). The
launcher's Rust had to be removed and reinstalled first (its folder held MinGW but no `rustc`). Two rebuilds were
my own fault: a diagnostic I added, and a probe command that wasn't valid.

**What the mod proved (log lines from `htp_spike`, native hooks API 21):**

| Result | Value |
|---|---|
| `native_server_tick` fires, `native_broadcast("Horizon Theme Park tick")` every 100 ticks | ✅ prints in chat and the log every 5 s |
| Commands from the tick hook (`native_run_command`) | ✅ works in single-player without `/publish` |
| 400 cheap commands (`time query gametime`) in one tick | **2.89 ms** (about 7 µs each) |
| 1,000 `summon item_display` in one tick | **33.5 ms** (about 33 µs each) |
| 400 `tp @e[tag=…,limit=1]` per tick with ~1,000 displays in the world | **436.6 ms** (1.09 ms each) |
| Can the mod read a command's answer? | Only success/fail (a bool), no text |

**Two steps of my test script did not run as designed** (a counting slip: the first summon step was skipped), so the mod's
own "tp by uuid / tp by tag" lines and its "400 tp with 1 display" line tested an empty world and prove nothing. I did not
rebuild a third time for them. I re-checked the same questions by hand with `horizon-ctl` on the **same build**:

| Re-check on commit `b5b451de` | Result |
|---|---|
| `tp <uuid>` and `data get entity <uuid>` on a display we summoned with `UUID:[I;1,2,3,4]` | ❌ "No entity was found" (same as before) |
| `tp @e[tag=…,limit=1]` | ✅ works |
| `ride` onto an `item_display` | ❌ "couldn't start riding" |
| `ride` onto a chicken | ✅ |
| `data merge entity <display> {transformation:{…}}` | ❌ "Nothing changed" |
| Rider camera on a seat moved +1 block/tick | ❌ stepped: shots 1–6 pixel-identical while the seat moved 19 blocks |

**Selector cost, measured two more ways** (datapack loop, 1,000 displays, 400 `tp` per tick): by tag 278 ms; by
position (`execute positioned … tp @e[distance=..0.1]`) 254 ms. No selector avoids scanning every entity: roughly
**0.6–1.1 µs per entity in the world, per command**.

**What this means for the plan (needs your decision, nothing edited):**

- A 6-car, 2-seat train needs 18 moves per tick. With *N* displays in the world each move costs about *N* µs, so
  18 moves cost about 18 × *N* µs: at N = 200 that is 3.6 ms, at N = 1,000 about 20 ms, at N = 3,000 about 60 ms (over the whole tick).
- So D25 (cap the world until UUID lookup is fixed) would mean about **200 displays in the world** for one ride, far below
  D27's 1,500. The rails count too (they never move, but every selector scans them).
- **Options:** (1) ask Horizon to make `tp <uuid>` find displays, which turns a scan into a lookup; (2) have the free reader
  find the smallest private call that moves a display by id, for `host_mixin` (needs your approval, D7); (3) keep
  selectors and shrink the design (fewer, longer rail pieces; parking almost everything). **Recommendation: 1 and 2 together;
  3 only as the fallback.**

## Reading follow-up: answers to the `agy` questions (read by me at `b5b451de`; agy's quota ran out)

agy's weekly allowance was used up (its error: resets in about 163 hours; the 15-minute first run was cut off, then the three
smaller batches were refused). I read the few functions myself, in an exported copy of the commit. All code is **read, not run**.

| Question | Answer |
|---|---|
| Q1: what is a summoned display? | An `OtherEntity { id, kind, position, yaw, pitch }` in `Game.others` (`game/commands.rs:66`), made by `load_other` (`game/persist.rs:588`). Its full NBT stays in `persist.raw`. The summon's `UUID` is stored: `remember(id, uuid, tag)` puts it in `persist.uuids` (a `HashMap<u32,[i32;4]>`, `persist.rs:817`). |
| Q2: why `tp <uuid>` fails | `target_with_uuid` (`game/commands/execute.rs:771`) **does find the id** in `persist.uuids`, then ends with `self.mobs.iter().any(|m| m.id == id)`. A display is in `self.others`, not `self.mobs`, so it returns None. The fix is one more check for `self.others` (and the other entity lists). I could not explain why the pig failed on the older installed game. |
| Q3: how a display's move reaches players | Every `OtherEntity` is turned into an entity state with its current position, yaw and pitch each time states are built (`lan_entities.rs:312–321`); display data comes from `persist.raw` (`lan_display_data`). So changing `o.position` is enough; no extra send is needed. (Read, not run.) |
| Q4: can a module of ours call these? | `teleport_to` is a private `fn` in `commands.rs:1304`, so only code inside that file's module could call it. But we don't need it: from a module added to `game.rs`, `self.others`, `self.persist.uuids` and `OtherEntity`'s public fields are visible (children of `game` see its private fields). |
| Q5: fling-off in single-player | The owner is a hosted guest. The path Horizon uses: find the guest's `conn` (`host.hub.guests.iter().find(|g| g.uuid == uuid)`), run `as_guest(host, conn, \|g\| g.player.velocity = v)` (`lan_actions.rs:263`), then `send_guest_motion(host, conn, Some(v))` (`lan_mobs.rs:328`). All `pub(super)`, so visible from a module in `game.rs`. Not run. Whether the owner's own window applies the packet is unverified. |
| Q6: stable way to read players | `native_server_players()` (`native_server.rs:239`): uuid, name, permission, dimension, position, game mode, health. **No riding flag.** |

**What this means:** a private "move display by UUID" call is tiny: look up the id in `persist.uuids`, find it in `others`, set
`position`, `yaw`, `pitch`. About 8 lines, in a module of ours inside `game.rs` (so `host_mixin.rs`, D7), with no call into
private `teleport_to`. Its cost should be a lookup instead of a scan, but that is a prediction, not a measurement.
