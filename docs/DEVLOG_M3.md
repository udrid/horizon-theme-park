Draft, not posted.

# M3: build your own coaster

In M2 we built one ride by typing a command. M3 lets you build your own. You place the track yourself, block by block, and the game checks that the train can really ride it.

## What M3 built

You get a track wand, an item with the name `htp:wand`. Using it works like placing blocks:

- Right-click a block. A node (a point the track passes through) appears 1 block above it.
- Right-click the air to switch mode. The modes are track, lift, brake and station.
- Left-click a block to remove the last node. `/coaster undo` does the same thing.

Each node shows a small marker, and end_rod particles (thin sparks) draw the curve for 30 seconds, so you can see the shape.

When the track is ready, `/coaster create <name>` joins the nodes into a loop and checks it. The checks are:

- At least 4 nodes.
- Exactly one station.
- Nodes at least 1 block apart.
- The station is long enough for the cars.
- Nothing is vertical (straight up and down).

The game also refuses a track where the train would enter the station too fast, or where it would stall (stop halfway). Then it prints a realism report: top speed, g-forces, drops and lift height. g-force is how heavy you feel. Anything past the limits shows in red.

Other commands:

- `edit` and `delete`. Delete asks you to type it twice. The file is kept as `.deleted`, so it can be recovered.
- `list` shows your rides.
- `set <name> cars <1-8>` changes how many cars the train has.
- `demo` builds the M2 ride.
- `remove` takes a ride away.

## Saving

Rides are saved as JSON files in the world folder. JSON is a plain text file with labels, like a neat list anyone can read. Saves are written safely: the game writes a `.tmp` file first, then swaps it in, and keeps a `.bak` backup. A file from a newer version of the mod is never overwritten. If a file gets damaged, it is restored from its backup. If there is no backup, only that one ride is lost.

## Far-away track

Track far from a player is not kept loaded. When a player walks up, it is drawn again. Some rules keep this light:

- Pieces are handled in groups of 16.
- Track more than 96 blocks away is taken down.
- Track within 80 blocks is drawn again.
- At most 50 pieces are handled per game tick (a tick is one of the 20 small steps the game takes each second).

Leftover entities (the invisible helper objects the game uses for track pieces) are swept every 5 seconds. Each one carries a session tag, so the game knows which ones are safe to remove.

## Numbers

- About 130 automatic tests run with `cargo test`. No game needed.
- 4 local commits for the build, plus review fixes.
- Not yet tried in the game. The owner still has to rebuild (about 1.5 minutes) and try it.

## What it felt like

[owner: not tried yet. Add what it felt like after the first game test.]

[video/screenshot]

## Next

M4 is next. This draft is not posted.
