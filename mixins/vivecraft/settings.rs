fn vivecraft_settings(
    game: &mut crate::game::Game<'_>,
    mod_id: &str,
    buttons: &mut Vec<String>,
) {
    crate::game::vivecraft_mod::settings_buttons(game, mod_id, buttons);
}
