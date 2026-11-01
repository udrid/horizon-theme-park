# M3b plan: Piece builder

**Status:** plan by **Claude Opus 5.5**, 2026-11-01. **Nothing is built.** The owner answers section 5 first.
Why: the owner found the wand confusing: **placing points** (clicking blocks, scaffolding for height, not seeing
where points are) and **no live track** (only particles until `create`). The owner chose a piece builder (D74).
The wand (D62–D67) stays as an advanced, free-form tool.

## 1. Goal, in plain words

Build a coaster the way RollerCoaster Tycoon or Planet Coaster do it: pick the next piece (straight, turn left, go up,
lift, brake, station…) and the track grows from its end, **drawn for real** as you go, at any height, with no
scaffolding. When the track comes back to its start, **Finish** joins it and the ride opens, with the realism report
(D64) and the safety checks (D68).

## 2. How it works (recommended answers)

```
 /coaster new thunder      → a "track head" appears 3 blocks east of you (D52), facing east, level,
                             with a 12-block station already placed (every ride needs one)
 hotbar kit (C1):          Straight · Left · Right · Up · Down · Lift · Brake · Block brake · Undo · Finish
 right-click a piece item  → that piece is added at the head; its rails appear at once; the head moves on
 the action bar shows      → piece count, height, and where the start is ("start: 14 blocks ahead, 30° right")
 Finish                    → if the head is close to the start and pointing roughly back at it, the gap is
                             joined smoothly, the ride is checked and saved (D20, D64, D68)
```

- Under the hood, each piece adds track **nodes** (D42's Catmull-Rom passes through them), so rides stay the same
  kind of file as now (D20): the wand can still edit them.
- The head keeps a direction (heading, in 15° steps) and a slope. Pieces change them smoothly, so the track never
  kinks, and transitions are rounded (D58).

## 3. Facts and limits

- The tick can't see what a player holds (M3 F3), but **using** an item is heard (`native_use_item`, API 13), so each
  piece is an item in the hotbar, used by right-click.
- Rails are drawn with the same displays as now (D29); each piece adds a few. The budget (D27) applies.
- The game can't tell which way a player faces (D52), so a new track starts facing east; the first pieces turn it.

## 4. Tasks (builder steps; Opus reviews and runs the checks)

| # | Task |
|---|---|
| T1 | `pieces.rs`: the piece set (C2) as node generators from a head (position, heading, slope); tests that every piece joins smoothly and that a sample layout of pieces passes D58 |
| T2 | `head.rs` / builder sessions: add, undo, the head's state, "where is the start" text, Finish's join rule (C3) |
| T3 | Live drawing: rails for each new piece at once; removed again on undo |
| T4 | Content mod: the piece items (one model each, vanilla textures) and `/coaster kit` to get them |
| T5 | Glue: `native_use_item` for the kit items; commands `new`, `kit`, `finish` |
| T6 | Opus review; then **you**: one rebuild, build a coaster with pieces |

## 5. Choices for you

**C1. How you pick pieces.**
- **A. A hotbar kit: each piece is an item. Hold it and right-click to add that piece.** Feels like Minecraft, quick,
  no menus. **Recommended.**
- B. A build screen (like the dispatch panel, D72) with a button per piece. Looks like Planet Coaster, but every piece
  means opening a screen.

**C2. The piece set.**
- **A. A small, fixed set:** Straight (4 blocks); Left / Right turns of 45° in three sizes (tight, medium, wide; a
  "Size" kit item switches between them, since the mod can't tell when you sneak); Up / Down that step the
  slope through level → gentle (20°) → steep (45°) → drop (60°); Lift, Brake, Block brake (D71), and the station
  placed by `new`. **Recommended.**
- B. The same, plus a "free" piece whose length and angle you type in a command.

**C3. Closing the loop.**
- **A. You steer back to the start; Finish joins the last gap** if the head is within 16 blocks of the start and
  pointing within 45° of it. Otherwise it tells you which way to go. **Recommended.**
- B. Finish draws the whole way back by itself (much harder to make smooth and realistic).

**C4. Order.**
- **A. Build this before M4**, since building is what you do most. **Recommended.**
- B. After M4.

### Small choices (I'll use these unless you say otherwise)

- **S1.** Headings in 15° steps; piece lengths in whole blocks; one undo per piece, all the way back to the station.
- **S2.** Turn sizes: tight radius 8, medium 16, wide 24 blocks. The realism report says when a turn is too tight for
  its speed (flat turns until banking, M5).
- **S3.** The action bar shows `Piece 12 · height 18 · start: 14 ahead, 30° right · size: medium`.
- **S4.** The wand stays for moving single points (advanced); rides from either tool are the same kind of file.

## 6. Cost and risks

- No downloads. One rebuild by you. OpenCode builds the steps; Opus reviews.
- **Risk:** joining the last gap smoothly (C3) is the hardest part; tests check the join stays within D58.
- **Risk:** many pieces mean many rail displays while building; the head only draws near the builder (D30 parking).
