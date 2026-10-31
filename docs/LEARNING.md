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
