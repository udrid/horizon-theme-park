// The game calls this at the start and end of every server tick. All it does is pass the news on.
fn htp_on_tick(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::ServerTickPhase,
) {
    crate::game::htp_glue::tick(game, phase);
}
