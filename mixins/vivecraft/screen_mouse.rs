fn vivecraft_screen_mouse(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::vivecraft_mod::on_screen_mouse_button_pressed_pre(game, screen, button, modifiers, canceled);
}
