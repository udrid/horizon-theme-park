// The game calls this on mouse-wheel steps while playing. The glue may "claim" them.
fn htp_on_wheel(steps: f32, claimed: &mut bool) {
    crate::game::htp_glue::wheel(steps, claimed);
}
