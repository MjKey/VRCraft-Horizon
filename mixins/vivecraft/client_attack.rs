fn vivecraft_client_attack(game: &mut crate::game::Game<'_>, target: u32, player: bool) {
    crate::game::vivecraft_mod::on_client_attack(game, target, player);
}
