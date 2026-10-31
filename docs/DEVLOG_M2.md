Draft, not posted.

# M2: gravity

In M1 our train went round at a constant speed on a fixed path. M2 is where real physics takes over. Now the train climbs, falls and slows down the way a real coaster does.

## What M2 built

Typing `/coaster demo` builds a raised ride with a 3-car train, which has 6 seats. The train is pulled up a 20.9-block chain lift at 18 km/h. It drops at 51 degrees, rides two airtime hills and a second drop, takes a wide turn, brakes, and stops in a station. A stone platform is built beside the station. Riders see their speed in km/h on the action bar, the line of text above the hotbar, and a 5-second countdown waits in the station.

## Shaped on paper first

Before we built anything in the game, we designed the ride with park-sim, a command-line tool. It writes speed and g-forces to a spreadsheet file. A g-force is how heavy you feel: 1 g is normal, and 0 g feels like floating.

The first results: top speed is about 70 km/h. Valleys push riders at most 3.05 g. Airtime hills lift riders about -0.2 to -0.4 g out of their seats. Sideways push is at most 0.6 g, and there are no sudden jolts.

We followed rules to keep it realistic. Drops are 50 to 65 degrees. Valleys stay at 4 g or less. The g-force never changes faster than 3 g per second. Lifts are 20 to 30 blocks tall, and every change of slope is rounded.

## Numbers

- The lap is about 327 blocks and takes about 44 seconds, plus the 5-second wait.
- About 85 automatic tests run in seconds with `cargo test`. No game needed.
- The game only runs a rebuild at the end. The physics engine has no Minecraft code in it.
- Back-row riders go faster over the lift crest than front-row riders. We will test this in the game.

## What it felt like

[owner: add what the ride felt like]

[video/screenshot]

## Not done yet

Building tools, saving and custom car models come later.
