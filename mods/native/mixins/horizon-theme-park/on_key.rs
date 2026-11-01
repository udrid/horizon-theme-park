// The game calls this when a key or mouse button goes down or up. All it does is pass it on.
fn htp_on_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::htp_glue::key(game, key, pressed);
}
