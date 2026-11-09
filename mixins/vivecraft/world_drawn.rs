fn vivecraft_world_drawn(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    crate::gfx::vr_bridge::on_world_drawn(world);
}