// The game calls this when a player types a command a mod registered. All it does is pass it on.
fn htp_on_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::htp_glue::command(game, player, command, ran);
}
