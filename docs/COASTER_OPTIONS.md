# Coaster rendering options (research, 2026-10-30)

Nothing here is decided. It's the research behind the next design choice. Numbers are measured in your scratch
game (build of Horizon `b5b451de`) unless marked otherwise. Full spike data: [SPIKE_RESULTS.md](SPIKE_RESULTS.md).

## The problem, in two parts

1. **Existing costs tick time.** Every display entity costs about 5 µs per tick just by existing, and about 6 µs
   more while a player is near it (3,000 displays = 19.9 ms of the 50 ms tick).
2. **Finding one to move is the real killer.** `tp @e[tag=…]` scans every entity in the world: about 0.6–1.1 µs per entity,
   per command. Cost = (moves per tick) × (entities in the world). 400 moves with 1,000 displays = 278 ms.

Different fixes attack each part, so they combine.

## Measured tricks (work today, no private Horizon code)

| Trick | Measured | Notes |
|---|---|---|
| **Move a seat through its rider by player name:** `execute as <PlayerName> on vehicle run tp @s X Y Z` | 400 per tick with 1,000 displays in the world: **8.3 ms** (about 7 µs each, idle was 5.5 ms) vs 271 ms for `@p` and 278 ms for tag selectors | Needs a rider on the seat (a player name is looked up directly). `@p` does *not* work: it scans. |
| **Move a car display less often, aimed ahead:** every *k* ticks send the car to where it will be *k* ticks from now, with `teleport_duration` *k* | `teleport_duration` 5 and a command every 5 ticks: the display glides continuously (gaps between shots 0.38–0.65 blocks, not jumps of 1.25) | The mod knows the future position exactly (deterministic physics), so the car arrives in step with its seat. It travels in a straight chord, so tight curves need a small *k*. Chord sag ≈ L²/(8R): 5 blocks at radius 10 sags 0.3 block. |
| Player moves by name (`tp <name> …`) | about 12 µs each | Also O(1), but teleporting a player **dismounts** them. |

## Ideas that failed

| Idea | Result |
|---|---|
| Summon a seat mob with `Passengers:[{display}]` so the car rides the seat | Not supported: the display was never created. |
| Ride a display, or make a display a passenger by `/ride` | Refused (`/ride` takes only mobs as vehicles, players or mobs as riders). |
| `@p`, `distance=` or position-based selectors | Still scan everything (254–278 ms). |
| Web research on how other coaster plugins render | Found almost nothing on internals. The pages describe TC-Coasters' track editing, not its rendering. ccRides mentions a *SmoothCoasters* client mod (view follows the ride's rotation); ZNPCsPlus says it works purely with packets. Treat both as pointers, not proof. |

## What each option costs and fixes

| Option | Fixes | Needs | Risk / cost |
|---|---|---|---|
| **A. Seats by rider name** (measured) | Seat moves become cheap and size-independent | Nothing private | Needs a rider; empty moving trains fall back to selectors |
| **B. Cars moved every 2–5 ticks, aimed ahead** (measured) | Cuts car commands 2–5 times | Nothing private | Polygonal on tight curves; use small *k* there |
| **C. Adaptive rail pieces** (already D29) | Fewer entities | Content mod | Stretched-model look unchecked |
| **D. Track as blocks (tile-based, like RollerCoaster Tycoon pieces)** | **Zero rail entities** | A big set of block models (straight, slopes, turns, banks) | Track snaps to a grid; large modelling job; not free-form splines |
| **E. Hybrid: block pieces for common shapes, displays only for free-form bits** | Most of D's saving, keeps freedom | Both | More code paths |
| **F. Fix `tp <uuid>` in Horizon** (ask, or a small PR) | Moves become a lookup for any display | Maintainers (or your PR) | Their schedule. In `target_with_uuid` (`execute.rs:771`) the lookup only checks players and mobs; a few lines more might cover displays. Unverified. |
| **G. `host_mixin`: move a display by its UUID/id directly** | Same as F, under our control | Private code, your approval (D7) | A module placed in `commands.rs` could call the private `teleport_to` for `Target::Other`. Pinned to a commit; untested. |
| **H. Packet-only "virtual" displays** (no server entity at all) | No tick cost, no scans, no tracking cost | Private code, big research | Biggest change; Horizon's guest packet sending (`hub.send_to`) exists but I have not tried it. |

## Rough per-tick cost with A + B (estimate, not measured end to end)

For one 6-car train, 2 seats per car, cars moved every 5 ticks:
- 12 seat moves × 7 µs = 0.1 ms, regardless of how many displays exist.
- 6 cars ÷ 5 = 1.2 car moves per tick × *N* µs, where *N* is the number of entities in the world.
- N = 1,000: about 1.3 ms per train. Ten trains running near players: about 13 ms. N = 3,000: about 36 ms.

So A + B alone put a comfortable ceiling at about 1,000 entities for a busy park, up from about 200. F or G removes the *N* term.

## Recommendation (for you to pick)

**Now:** A + B + C (already decided). They need no private code and I measured them.
**Next, in parallel:** F (ask Horizon, or draft a tiny PR for you to post) and have the free reader look into G.
**Later, only if we still need more rails:** E (blocks for the common pieces).
**Not now:** H, which is big and untested.
