// The game calls this every frame to draw the HUD (not under F1). All it does is pass it on.
fn htp_on_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::htp_glue::hud(game, batch, corners);
}
