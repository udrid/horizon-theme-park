// The command-line face of park-bundle. All the real work is in lib.rs, so tests can reach it.
//
//   cargo run -p park-bundle            writes park.rs
//   cargo run -p park-bundle -- --check says whether park.rs is up to date (exit code 1 if not)

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // This tool lives in <repo>/tools/park-bundle, so the repo's root is two folders up.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."));

    let outcome = park_bundle::run(&args, root);
    if outcome.code == 0 {
        println!("{}", outcome.message);
    } else {
        eprintln!("{}", outcome.message);
    }
    // Exit codes are tiny numbers (0, 1, 2), so this conversion never loses anything.
    ExitCode::from(u8::try_from(outcome.code).unwrap_or(2))
}
