// The compile test (D37).
//
// The game will paste `park.rs` inside `crate::game::htp_park`. Here we pretend to be the game:
// we build the same module path in this test, paste the generated file into it, and let the
// compiler check it. Because `park.rs` still contains park-core's test blocks, running this file
// also runs park-core's tests a second time, through the glued copy. If gluing broke something
// (a bad path, a missed `crate::`), this test fails in seconds instead of a 6-minute game build.
//
// `park.rs` itself is made by `cargo run -p park-bundle`. If it is stale, this test checks the
// OLD copy, which is why `cargo run -p park-bundle -- --check` exists too.

// The game never uses most of our code yet, so silence "never used" warnings for the pretend game.
#[allow(dead_code)]
mod game {
    pub mod htp_park {
        include!("../../../mods/native/mixins/horizon-theme-park/park.rs");
    }
}

#[test]
fn the_glued_copy_has_the_exact_message() {
    assert_eq!(
        game::htp_park::welcome::LOADED_MESSAGE,
        "Horizon Theme Park loaded"
    );
}

#[test]
fn the_glued_copy_behaves_like_the_original() {
    let mut welcome = game::htp_park::welcome::Welcome::new();
    assert_eq!(welcome.tick(false), None);
    assert_eq!(
        welcome.tick(true),
        Some(game::htp_park::welcome::LOADED_MESSAGE)
    );
    assert_eq!(welcome.tick(true), None);
}
