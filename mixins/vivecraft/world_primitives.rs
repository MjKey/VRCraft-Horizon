fn vivecraft_world_primitives(
    game: &crate::game::Game<'_>,
    partial: f32,
    camera: [f64; 3],
    primitives: &mut crate::world_primitives::WorldPrimitives,
) {
    crate::game::vivecraft_mod::on_world_primitives(game, partial, camera, primitives);
}
