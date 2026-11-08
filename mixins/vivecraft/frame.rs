fn vivecraft_frame(game: &mut crate::game::Game<'_>, elapsed: std::time::Duration) {
    crate::game::vivecraft_mod::on_frame(game, elapsed);
}
