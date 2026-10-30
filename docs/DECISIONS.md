# Decisions

Every design choice you approve is recorded here so nobody asks twice.

| # | Choice | Reason | Date |
|---|---|---|---|
| 1 | Tree felling is dropped entirely. | Focus on one project. | 2026-10-30 |
| 2 | Aim at theme parks from day one ("option C"): repo `horizon-theme-park`, room for `coasters/`, `rides/`, `park/`. Milestone order unchanged; the coaster is still the first showcase. | Cheapest change, keeps the research done, gives the project a park-sized name now. | 2026-10-30 |
| 3 | Crates are named `park-core`, `park-sim`, `park-web`. Coaster code will be a module inside `park-core`. | A park with several ride types does not fit in a crate called "coaster". | 2026-10-30 |
| 4 | Project folder is `horizon-theme-park`, next to the other Horizon folders on the Desktop. | Everything stays together and is easy to find. | 2026-10-30 |
| 5 | Tests and builds run mostly on your own computer. CI on GitHub is the free backup check. The Codespace file stays in the repo but is optional. | You have unlimited internet; the only limit is AI usage, and running commands locally lets the agent fix problems without copy-and-paste. | 2026-10-30 |
| 6 | Cargo for our own tests comes from official rustup (Rust 1.99.1, MinGW/`gnu` target, minimal profile), not the launcher's toolchain. Its folders are added to PATH per command, not permanently. | The launcher's `rust install` left no rustc or cargo. The launcher's MinGW linker works, and there is no Visual Studio. | 2026-10-30 |
