# M3b plan: Coaster builder (on screen)

**Status:** plan by **Claude Opus 5.5**, 2026-11-01, rewritten after the owner's feedback. **Nothing is built.** The
owner answers section 6 first; then it's built before M4 (owner's C4 = A).

**What the owner asked for:** more options; builder-friendly, with **less typing and more on screen**; a **powerful,
realistic building simulator**, inspired by Planet Coaster and RollerCoaster Tycoon; and **help finishing the
coaster**, with a smooth helper, within reason. The wand (D62–D67) stays as an advanced tool.

## 1. What it feels like (recommended answers)

1. Press **B** anywhere: **build mode** opens. A panel appears at the side of the screen; you can still walk, fly and look
   around. Pick "New coaster" (or an existing one) on the panel. A new coaster starts **in front of you, facing where you
   look**, with its station.
2. The panel shows the **next piece** and its settings: type, length, turn, slope, element (lift, brake, block brake).
   **Scroll the mouse wheel** to change the highlighted setting, **R** to move to the next setting.
3. A **ghost piece** (see-through rails) shows exactly where the next piece will go, and moves as you change settings.
4. **Right-click** builds it: real rails appear at once. **Z** undoes and **Y** redoes, piece by piece.
5. A **live stats line** shows the train's speed when it reaches the end of the track, the height, and the g-forces of
   the ghost piece at that speed. Anything past the realism limits (D58) turns **red** and says why ("lateral 1.4 g:
   widen the turn"). Settings that would break a limit are greyed out, with the reason.
6. Press **H** for the **finish helper**: it draws a smooth ghost path from your track's end back into the station,
   with a brake run before it. Right-click accepts it; anything else cancels. If it can't make a smooth, realistic path,
   it says why in plain words ("too high above the station: build down about 12 blocks first").
7. Accepting the finish saves and opens the ride, with the realism report (D64) and the safety checks (D68).

## 2. Facts that shape it

Read at Horizon `b5b451de` by a Sonnet helper (file:line in its report). Inspiration from a web helper's report
(Planet Coaster, RollerCoaster Tycoon, NoLimits, TC-Coasters).

| # | Fact | How sure |
|---|---|---|
| F1 | `native_hud` draws on the screen every frame **without** opening a screen, so the world stays visible and you can move. Text, filled boxes, sprites and item icons can be drawn anywhere | read |
| F2 | `native_key` hears key presses; `native_wheel` hears the mouse wheel and can **claim** it (so the hotbar doesn't scroll) | read |
| F3 | The client's hooks and the server's hooks run in one program for the **host** (single-player, or the player who hosts a LAN world), so they can share the mod's own state. **LAN guests don't run our client code**, so they can't use the panel (they keep the wand and commands) | read (shared state is an inference; T0 tests it) |
| F4 | The client knows where you look (`native_player`: position, yaw, pitch), so a new coaster can start facing your view | read |
| F5 | No hook draws lines in the 3D world, but our ghost piece can be **display entities** with a see-through model, like the rails | read |
| F6 | Planet Coaster's auto-complete only works when the end is very close and level; players find it clumsy. A smooth helper that explains its refusals beats it | web, forum-level |

## 3. The pieces and settings (C2)

| Setting | Values | Notes |
|---|---|---|
| Type | Straight · Turn · Slope change · Turn + slope | one piece = one smooth stretch |
| Length | 1–32 blocks | |
| Turn | −180° to +180° in 15° steps | the radius follows from length and angle, shown on the panel |
| Slope | −70° to +70° in 5° steps (the slope at the piece's end) | climbs and drops ease in and out smoothly |
| Element | Track · Lift · Brake · Block brake (M4) · Station (only by "New coaster") | |
| Banking | shown, greyed: "comes with M5" | |

Under the hood each piece adds track nodes (D42), so a ride stays the same kind of file (D20) and the wand can still
edit it.

## 4. Tasks (builder steps; Opus reviews and runs the checks)

| # | Task |
|---|---|
| T0 | Check, small rebuild: the client and server share a static in single-player; `native_wheel` claim; `native_key` names (`key.keyboard.b`, `r`, `z`, `y`, `h`) |
| T1 | `pieces.rs`: a piece from a head pose (position, heading, slope) and its settings → nodes; smooth joins; tests that every piece joins with matching direction and slope |
| T2 | `preview.rs`: an open-track dry run from the station to the head, giving speed at the end; g-forces of a ghost piece at that speed; limit checks and greyed-out reasons (D58) |
| T3 | `finish.rs`: the finish helper: a smooth path (a Hermite curve, tuned) from the head into the station's approach, with a brake run; checks D58 and D68; plain-words refusals |
| T4 | Build sessions: build / undo / redo stacks; save and open through the same path as `create` |
| T5 | Ghost drawing: see-through rail displays for the next piece and the finish path, redrawn when settings change |
| T6 | Glue (client): HUD panel drawing, keys, wheel, and the shared state; glue (server): reads it on the tick |
| T7 | Content mod: the see-through ghost rail model |
| T8 | Opus review (`pieces.rs`, `finish.rs`: maths); then **you**: one rebuild, build a coaster on screen |

## 5. Later (M3c, after M4, unless you want it sooner)

Test run with a ghost train and a "where it stalled" report; selecting a piece in the middle and rebuilding from there;
templates (lift hill, drop, helix, starter layouts); ratings (RCT's excitement, intensity, nausea: M5).

## 6. Choices for you

**C1. How you control the builder.**
- **A. An on-screen panel (B to open), changed with the mouse wheel and a few keys; right-click builds; you keep moving
  and looking around.** Closest to Planet Coaster. **Recommended.**
- B. A build-table block that opens a full screen with buttons (like the dispatch panel, D72). You can't move while it's
  open, and you must stand at the table.

**C2. Piece settings.**
- **A. Free settings in steps (section 3): length, turn and slope you set yourself, previewed live.** Powerful and still
  easy, since the ghost shows the result. **Recommended.**
- B. Fixed pieces (RollerCoaster Tycoon style: small, medium, large turns; set slopes). Simpler, less freedom.

**C3. The finish helper.**
- **A. A smooth path back into the station with a brake run, shown as a ghost first; refuses with a plain reason
  when it can't stay smooth and realistic.** **Recommended.**
- B. Planet Coaster style: only joins when you're already very close and level.

**C4. LAN guests.**
- **A. The on-screen builder is for the world's host; guests use the wand and commands for now** (F3). **Recommended**
  (it's what Horizon allows today).
- B. Hold the builder until guests can use it too (would need a Horizon change).

**C5. Scope.**
- **A. M3b as in section 4 now; test runs, editing from the middle and templates in M3c (section 5).** **Recommended.**
- B. Everything at once: a much bigger step before you can try it.

### Small choices (I'll use these unless you say otherwise)

- **S1.** Keys: **B** build mode on/off, **wheel** change value, **R** next setting, **right-click** build, **Z** undo,
  **Y** redo, **H** finish helper. All can be changed in Horizon's mod key settings (`native_mod_key`).
- **S2.** The panel sits at the right edge, about a quarter of the screen wide, hidden with F1 like the HUD.
- **S3.** The ghost model is the rail model in light-blue glass; the finish path is drawn the same way.
- **S4.** The station placed by "New coaster" is 3 cars long (D55), flat, with the platform (D60).

## 7. Cost and risks

- No downloads. One small rebuild for T0, one for the end. OpenCode builds the steps; Opus writes briefs and reviews the
  maths (pieces, finish helper).
- **Risk:** the shared client/server state (F3) is an inference; T0 checks it before anything else is built. If it fails,
  C1-B (the build-table screen) is the fallback.
- **Risk:** the finish helper is the hardest part. It may refuse often at first; tests check that what it accepts stays
  within D58.
- **Risk:** ghost redraws cost commands (D21's 400 a tick); ghosts redraw at most 4 times a second.
