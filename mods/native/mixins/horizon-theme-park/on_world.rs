// The game calls this at the start of every world phase change. All it does is pass the news on.
fn htp_on_world(phase: crate::game::native_lifecycle::WorldPhase) {
    crate::game::htp_glue::world(phase);
}
