// The game calls this when it asks which commands mods add (once when a world opens to players).
// All it does is pass the list along, so the glue can add `/coaster demo`, `ride` and `remove`.
fn htp_on_commands(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::htp_glue::register(commands);
}
