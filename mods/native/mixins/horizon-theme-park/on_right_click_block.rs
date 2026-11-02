// The game calls this when a player right-clicks a block, before the block's own use. All it does
// is pass it on; the glue cancels the click while the on-screen builder is open.
fn htp_on_right_click_block(canceled: &mut bool) {
    crate::game::htp_glue::right_click_block(canceled);
}
