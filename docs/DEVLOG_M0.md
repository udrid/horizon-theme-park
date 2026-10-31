# M0: our code says hello inside Minecraft

*Draft, not posted. Fill in the CI line before posting.*

Horizon Theme Park is a roller coaster and theme park mod for Horizon. A mod is an add-on that changes a game. Horizon is a rewrite of Minecraft 26.3 in Rust, the programming language we use. We are not building coasters yet. M0 is the first milestone (one planned chunk of work), and its job is to prove that our code can reach the game.

## What M0 built

- **park-core:** the mod's "brain", code that runs without the game. Its first file decides when to say the greeting. Its tests run in a blink with `cargo test`, Rust's test command.
- **park-bundle:** a small helper that glues park-core into one file the game can read. It also checks that the file is up to date, so mistakes show up in seconds.
- **The native mod:** a thin layer that plugs into Horizon when a world opens, and on each game tick (one short step of game time).

## The greeting in game

On 31 October 2026, the owner built the mod with Horizon's launcher, which took 520 seconds. They opened a new creative world and saw "Horizon Theme Park loaded" appear in chat, once.

*Suggested screenshot: the chat line.*

The greeting waits until a player is in the world. In single-player, nobody is there when the world first opens, so an early message would probably not reach chat. The code waits for the first tick with a player, then says the line once per world.

## Where CI stands

CI (continuous integration) is a robot that re-runs our checks on GitHub, the website that stores our code, each time we push. Pushing sends our saved code to GitHub. Nothing has been pushed yet, so the robot has not run.

**[CI result: fill in after the owner pushes]**

*Suggested screenshot: the green run on GitHub's Actions tab, once it exists.*

**Next:** push the code and watch the checks run.

---

**Notes for you (not part of the post)**

- Nothing is posted. Post it only where and when you choose.
- After you push, replace the bold CI line with the real result, and delete the draft line at the top.
- Add the two screenshots yourself: the first from your chat, the second from GitHub once the first run exists.
- Left out on purpose, because they are not done yet: the check of the game's log file, and the test in a scratch world.
