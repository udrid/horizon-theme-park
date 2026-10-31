# Learning notes (your glossary)

Terms are added the first time they appear. Each has an everyday comparison.

## Repo (repository)
A project folder with a full history of every saved change. Like a folder of documents that remembers every earlier version.

## Commit
One saved snapshot in that history, with a short note about what changed. Like a "save point" in a video game.

## Push
Sending your commits from your computer up to GitHub. Like uploading your save file to the cloud.

## GitHub
A website that stores repos online and runs free tools for them (automatic checks, cloud computers).

## Cargo
Rust's all-in-one helper program. It builds your code, runs tests and downloads libraries. Like a project manager that does the chores.

## Crate
One package of Rust code. Our project has several crates, each with one job. Like chapters in a book.

## Workspace
A list in the top `Cargo.toml` that groups several crates so Cargo builds and tests them together. Like the table of contents of the book.

## CI (continuous integration)
A robot on GitHub that runs checks on your code every time you push. Like a spell-checker that runs on every save.

## Codespace
A computer in the cloud that GitHub lends you (free hours each month), opened in your browser or VS Code. Big downloads and builds happen there, not on your internet.

## Codespace, in plain words (added later)
A Codespace is a computer GitHub lends you over the internet, with 120 free core-hours a month (about 60 hours on our 2-core one). The meter runs while it is open, not only while you work, so you must stop it. We decided to build on your own computer instead, so it is optional.

## Antigravity CLI (`agy`)
A free assistant in the terminal from Google that reads big files for us, so Claude does not have to. It has a weekly allowance. Check it with `/usage` inside `agy`.

## rustup
The official installer and updater for Rust. It gave us `cargo` and `rustc`.

## Linker
The tool that joins compiled pieces into one program. Windows Rust needs one; we use MinGW (`gcc`) because Visual Studio is not installed.

## Hook
A spot in the game's code where Horizon lets a mod join in, like a hook on a wall where you can hang your own coat. Example: `native_server_tick` is called 20 times a second, so our mod can move the coaster each time.

## Hook API number
A version number for the list of hooks. Every time Horizon adds a hook, the number goes up by one. Our mod says "I need at least 21", like a game box saying "needs version 21 or newer".

## Console command
A command typed as the server itself (not as a player), with full permission. Like the shop owner using the master key. `native_run_command` lets our mod do this, for example `summon` or `tp`.

## Mixin
A small patch that changes Horizon's own source code before it is built. Powerful but fragile: it is tied to one exact version of Horizon, like a key cut for one lock. If the lock changes, the key stops working.

## Trait
In Rust, a job description: a list of things something must be able to do, without saying how. Like a "waiter" job description: take orders, bring plates. Different people (types) can fill the same job.

## Host (our trait)
The waiter between the brain (`park-core`) and the game. The brain says "put car 1 here"; the host turns that into something the game understands.

## Fake host
A pretend waiter used in tests. It does not touch the game; it writes every order in a notebook so a test can check "did the brain ask for the right things?".

## Bundle (glued file)
`park-core` is many small files, but a native mod can only carry plain `.rs` files, not a whole library. A small tool staples the files into one big file inside the mod, like photocopying the chapters you need into one booklet. If someone edits `park-core` and forgets to re-glue, CI stops them.

## Dependency
A library someone else wrote that our code uses, like buying flour instead of growing wheat. Each one is downloaded the first time, and the game build only allows the ones Horizon already uses.

## Vector
An arrow in 3D space, stored as three numbers (x, y, z). "Where the car is" and "which way is up" are both vectors. Like directions: "3 east, 1 up, 2 north".

## f32 and f64
Two sizes of decimal number. f32 keeps about 7 digits, f64 about 15. Like a ruler marked in centimetres versus one marked in hair-widths. We use f64 so cars far from spawn don't wobble.

## glam
A borrowed maths toolbox for vectors and rotations. Horizon uses it too, so the game build allows it.

## Module
One file of code with one job, like one drawer in a filing cabinet. `spline.rs` holds curve maths; `physics.rs` holds the speed maths.

## Runtime
The part of the code that runs the loop every tick: "move the trains, then tell the game what changed". Like a conductor keeping the beat.

## Unit
What a number is counted in: blocks, seconds, degrees. "2" means nothing until you know if it is 2 spoons or 2 cups.

## Radian
Another way to measure angles that maths prefers. A full turn is about 6.28 radians (2π), a right angle about 1.57. We save degrees in files because people read them more easily.

## G-force
How hard the ride pushes on you, counted in "g". Standing still is 1 g; the bottom of a big drop might be 3.5 g (you feel three and a half times heavier); the top of an airtime hill can be 0 g or less (you float).

## Gravity scale
A dial on each coaster. 1.0 is real gravity. 1.5 makes everything fall half as fast again. G-force readouts ignore the dial so they stay true.

## Integrator
The recipe for one small jump forward in time: "work out the new speed, then the new position". Like drawing one page of a flip-book from the page before. Ours is called semi-implicit Euler.

## Substep
A small step inside one game tick. We take 10 per tick (each 0.005 s), so the maths stays accurate even though the game only ticks 20 times a second.

## Deterministic
The same input always gives exactly the same result, like a calculator. Our coaster rides the same way every time, which makes bugs easy to repeat.

## s (arc length)
Distance along the track in blocks, like the distance markers on a running track. "The train is at s = 120" means 120 blocks from the start, however twisty the track is.

## Rigid body
One solid thing that cannot stretch or squash. Our whole train is treated as one rigid body, so all its cars move at the same speed.

## Friction (rolling)
The wheels rubbing on the rails, always slowing the train a little, like a shopping trolley that never rolls freely. Ours grows when the train presses harder on the rails.

## Air drag
The air pushing back, growing with speed times speed. Like a hand out of a car window: twice as fast, about four times the push.

## Normal force
How hard the train presses on the rails (and the seat on you). Big in the bottom of a valley, near zero on an airtime hill.

## Curvature
How tightly the track bends. A huge gentle curve has low curvature; a tight loop has high curvature. Push on the rider = speed × speed × curvature.

## Heartline
An imaginary line through the riders' chests, about 1.1 blocks above the rails. Good coasters roll around the heartline, not the rails, so a roll feels smooth instead of throwing your head sideways.

## Smoothing
Averaging a number over a short time (for us, a quarter of a second) so tiny bumps don't flash scary values. Like reading a car's speedometer instead of every vibration.

## Entity
Anything in the world that isn't a block and can be anywhere: players, mobs, minecarts, and our display entities. The game sends each one to nearby players, so too many make the game slow.

## Display entity
An invisible entity whose only job is to show a block, an item model or text, at any position, angle and size. We draw rails and cars with them.

## Tracking range
How close a player must be before the server sends them an entity. Things outside it never reach your game, like shops that only deliver within a few kilometres.

## Parking a ride (our term)
When no player is near a ride, the mod removes its displays and keeps only the maths running. When someone walks up, it redraws them over about half a second.

## JSON
A plain-text way to write down data with names and values, like a very tidy form. You can open our ride files in Notepad and read them.

## Format version
A number at the top of a saved file saying which layout it uses. When the layout changes, the mod reads the number and upgrades old files instead of misreading them.

## Tag
A label stuck on an entity, like a name sticker. Our displays carry the tag `htp` so the mod can find and remove only its own.

## Spline
A smooth curve drawn through a few chosen points, like bending a flexible ruler so it touches each pin on a board. Our track is a spline through the nodes you place.

## Node
One of those chosen points on the track: a position, a bank angle (roll) and what the track does next (lift, brake...).

## Catmull-Rom and Bézier
Two recipes for drawing a spline. Catmull-Rom passes through every node by itself; Bézier uses extra "handle" points that pull the curve, like in drawing programs. Which one we use is still open.

## Arc-length table
A list made once per track: "at 0.1 blocks along, the track is here and faces this way; at 0.2 blocks...". Looking things up in it is instant, like the distance chart in a road atlas.

## Frame
Three arrows that say how the track is turned at one spot: forward (along the track), up (out of the rails) and right. The car is drawn using them.

## Rotation-minimising frame
A way to carry the "up" arrow along the track with the least twisting, like sliding a ring along a bent wire without turning it. It never flips over the way some other methods do.

## Quaternion
Four numbers that store a 3D rotation without the "stuck" problems of plain angles. Display entities take their rotation as a quaternion.

## Element
What a piece of track does: normal track, station, lift hill, brake, block brake or booster.

## Block section
A stretch of track only one train may be in at a time, ended by a block brake. Like a single-track railway where a signal stops the next train until the stretch is clear.

## UUID
A very long ID number (128 bits) that names one entity. We pick each display's UUID ourselves, so we can always find it again.

## Engine spike
A short, throwaway experiment that proves the risky parts work before we build on them. Like testing one tile before tiling the whole bathroom.

## Summon
The command that creates an entity, like ordering a new item from a shop. `summon minecraft:item_display 10 70 10 {…}` makes a display at that spot; the `{…}` part is a list of settings (its tags, its item, its size, even its UUID).

## Teleport duration (`teleport_duration`)
A setting on a display: how many ticks it takes to glide to a new spot after a `tp`. 0 means it jumps; 1 means it glides over one tick, so on screen it moves smoothly. Like a slide on a train: the longer the slide, the smoother but the more it trails behind.

## Interpolation
The game drawing the in-between positions of something that only changes 20 times a second, so you see 100+ smooth pictures a second instead of 20 jumps. Like the frames drawn between two key poses in a cartoon.

## Tick (and tick time)
One step of the game's clock; there are 20 a second, so each has 50 milliseconds to finish everything. "Tick time" is how long it really took. If it goes over 50 ms the whole game slows down.

## Selector
The `@e[tag=x]` part of a command: "every entity that matches this". The game checks every entity to answer, so the more entities exist, the slower each selector gets. Like finding a person by calling out their hair colour in a crowd, instead of by their ID card.

## Datapack function
A text file of commands the game can run with one `function` command, and which can schedule itself again a tick later. We used one in the spike to move things every tick without writing a mod.

## Seat (mob seat)
The thing a rider sits on. In Horizon today only a mob (a living creature, like a chicken) can carry a rider, so our "invisible seat" would be an invisible mob.

## Velocity
Speed together with direction, how fast and which way something is moving right now. A rider thrown off a coaster keeps the coaster's velocity. Horizon has no command to give a player one.

## Stepped vs smooth
Stepped motion shows only the 20 positions per second the game calculates, so it looks like a slideshow at high speed. Smooth motion draws the positions in between. We tested the difference by looking at a striped floor: stepped shows the stripes at exactly the same screen spots every time.

## Lookup vs scan
A lookup goes straight to one thing by its name or ID, like finding a book by its shelf number. A scan checks everything until it finds a match, like reading every spine in the library. A command that finds a player by name is a lookup; `@e[tag=x]` is a scan, so it gets slower the more entities exist.

## Aimed ahead (our term)
Instead of moving a car every tick, the mod sends it to where it will be a few ticks from now and tells the game to glide there over those ticks. Like throwing a ball to where your friend will be, not to where they are.

## Chord
The straight line between two points on a curve. A car gliding between two updates travels along the chord, so on a tight curve the updates must come more often or the car cuts the corner.

## Compile (compiler)
Turning the Rust text we write into a program the computer can run. The compiler is strict, like a teacher marking every spelling mistake before you may hand in the essay. "It compiles" means it passed that check.

## Panic
A Rust program stopping on purpose because something "can't happen" did happen, like a machine's emergency stop button. Inside the game a panic can take the whole world down, so our glue code must never panic.

## Edition (Rust edition)
A version of Rust's grammar rules. Every few years Rust adds new rules; each project says which set it follows, like a board game played with the 2021 or the 2024 rulebook. Horizon plays with 2024. Our code gets glued into Horizon, so it is judged by the 2024 rulebook there.

## Attribute
A sticky note on a piece of code for the compiler, written `#[…]`. `#[test]` says "this is a test"; `#[cfg(test)]` says "only include this when testing". A note with `!`, `#![…]`, sticks to the whole file instead of one item. Horizon's mixin tool refuses those whole-file notes.

## Doc comment
A comment meant for documentation. `///` describes the thing below it; `//!` describes the whole file. To Rust, `//!` is a whole-file attribute, so our glue tool turns it into a plain `//` comment.

## Macro
A shortcut that writes code for you, recognised by its `!`, like `println!` or `env!`. `env!` reads a setting from the project being built. Inside the game that would be Horizon's project, not ours, so `park-core` doesn't use it.

## Module path (`crate::`)
The address of a piece of code. `crate::welcome::Welcome` means "in this project, the drawer `welcome`, the item `Welcome`". When the glue tool moves `park-core` into the game, the address changes to `crate::game::htp_park::welcome::Welcome`, like updating every address book after moving house.

## Generated file (and "stale")
A file a tool writes, never a person. `park.rs` is generated by `park-bundle`. It is "stale" when the source changed but nobody re-ran the tool, like a printed timetable that is out of date. The `--check` step catches that.

## Test-only code
Code marked `#[cfg(test)]`. It is included only when running tests and left out of the real build, like scaffolding that is never part of the finished house. That's why our glue tool can leave it in the game file: the game never builds it.

## Integration test (and sample tree)
A test that uses code from the outside, the way someone else would. Ours will paste the glued file into a pretend "game" folder and check it works there. A sample tree (also called a fixture) is a small set of made-up files the tests run on, like practice dough before baking the real cake.

## Library feature
An optional extra in a library that a project can switch on, like the options list when you buy a car. Horizon's `glam` has no extras switched on, so our code must not rely on any (for example the one that lets `serde` save a `DVec3`).

## serde
A library that turns our data into text (like JSON) and back. Like a translator between "what the program holds in memory" and "what goes in a file". We'll use it to save rides at M3.

## Parser (and syn)
A parser reads text and works out its structure, like a reader spotting sentences and paragraphs. `syn` is a library that reads Rust exactly like the compiler. Our glue tool could use it, or simply read the text line by line.

## Line endings
The invisible mark at the end of each line of text. Windows uses two characters (written CRLF), Linux and Mac use one (LF), like two countries writing dates differently. Git on your PC converts between them, so our tools must treat both as the same. A `.gitattributes` file tells Git which style to keep for a file.

## Exit code
A number a program gives back when it finishes: 0 means "all good", anything else means "something failed". CI reads it, like a traffic light: `park-bundle --check` returns 1 (red) when `park.rs` is stale.

## Pin (pinning a commit)
Fixing exactly which version of Horizon our mod is built against, by its commit ID (`b5b451de…`). Like writing "use the 2026 edition of this textbook": later changes to Horizon can't break us until we choose to move.

## Injection (HEAD) and helper
An injection tells the mixin tool where to add our code inside Horizon. A `HEAD` injection adds it at the very start of a Horizon function, like a note clipped to the front of a letter. The helper is the one small function that gets pasted in there; ours just calls into `htp_glue`.

## Placed player (Horizon's word)
A player who has fully arrived in the world, not still connecting or loading. Horizon only sends chat to placed players, and its player list only shows them. That's why the "loaded" line waits until someone is placed.

## World open ready
The moment Horizon says "the world has finished opening". In single-player this comes before your own connection is set up, so you aren't in the world yet when it fires.

## Log file (`latest.log`)
A diary the game writes while it runs: every message, warning and error, with the time. It lives in the game's `logs` folder. When something is invisible on screen, the log often still shows what happened.

## Static (and Mutex)
A `static` is one value that lives for the whole time the game runs, like a whiteboard on the wall that everybody can read. A `Mutex` is a lock on it, like a talking stick: only the one holding it may write. Our glue keeps one `Welcome` this way.
