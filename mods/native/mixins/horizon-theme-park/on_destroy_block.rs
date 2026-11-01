// The game calls this when a player breaks a block. All it does is pass it on; the glue cancels the
// break only when the player holds the track wand (left-click = undo the last node).
fn htp_on_destroy_block(game: &mut crate::game::Game<'_>, canceled: &mut bool) {
    crate::game::htp_glue::destroy_block(game, canceled);
}
