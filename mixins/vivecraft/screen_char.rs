fn vivecraft_screen_char(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    c: char,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::vivecraft_mod::on_screen_char_typed_pre(game, screen, c, modifiers, canceled);
}
