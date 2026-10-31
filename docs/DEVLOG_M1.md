Draft, not posted.

# M1: our first ride

Horizon Theme Park is a roller coaster mod for Horizon, a rewrite of Minecraft 26.3 in Rust. M1 is our first ride. A player can now sit in a car and go around a track. We are not building gravity yet, so the train only follows a fixed path.

## What M1 built

Typing `/coaster demo` builds an oval of rails about 59.7 blocks long, three blocks east of the player, with one red car on it. Right-click a seat to board. The train waits at the start while it is empty, runs while someone rides, and after the last rider gets off it finishes the lap and stops at the start. `/coaster remove` takes it away.

The track is a centripetal Catmull-Rom spline, which is a smooth curve that passes through the points we give it, like a flexible ruler bent through pins. The oval's measured length is within 0.1% of the exact 59.70.

## What testing taught us

We tested the risky parts before building. The server console cannot seat a player, because Horizon treats it as a guest. A player's own right-click works. Saddled pigs were a workaround, so we used real minecarts as seats instead.

A rider in a minecart hangs 1.2125 blocks below the seat. The owner measured this with a `/data get` command. Once the code allowed for that gap, the train started as soon as someone sat down.

## Numbers

- 51 automated tests. They run with `cargo test` in about a second, with no game needed.
- First game build: 7 min 50 s. Rebuilds take about 1.5 min.
- Speed: we tried 20 blocks/s (72 km/h). The rider's camera updates 20 times a second, and the ride still felt smooth.

## Not done yet

Gravity, building tools, saving and custom car models come later.

**Next:** M2, gravity, a lift hill and a first drop.
