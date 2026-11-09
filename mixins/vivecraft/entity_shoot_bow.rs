fn vivecraft_entity_shoot_bow(
    game: &mut crate::game::Game<'_>,
    player: u128,
    name: &str,
    projectile: u32,
    force: f32,
) {
    crate::game::vivecraft_mod::on_entity_shoot_bow(game, player, name, projectile, force);
}