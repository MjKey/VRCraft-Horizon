fn vivecraft_camera_shake(
    game: &crate::game::Game<'_>,
    partial: f32,
    shake: &mut crate::game::native_hooks::NativeShake,
) {
    crate::game::vivecraft_mod::on_camera_shake(game, partial, shake);
}
