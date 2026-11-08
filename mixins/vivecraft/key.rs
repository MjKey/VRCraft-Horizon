fn vivecraft_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::vivecraft_mod::on_key(game, key, pressed);
}
