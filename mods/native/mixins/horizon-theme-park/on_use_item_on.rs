// The game calls this when a player right-clicks a block with an item. All it does is pass it on;
// the glue claims the click only for the track wand.
fn htp_on_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    claimed: &mut bool,
) {
    crate::game::htp_glue::use_item_on(game, stack, pos, claimed);
}
