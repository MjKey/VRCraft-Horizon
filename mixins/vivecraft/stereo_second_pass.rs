// Stereo render, both eyes in one game frame (AFTER-the-first-call pattern):
// this HEAD hook runs before `render_frame`'s body. It renders the RIGHT eye
// first (with `stereo_eye` 1, so `views.primary()` carries the right eye's
// view and the whole engine draws the world from it), then restores
// `stereo_eye` 0 and lets the original body render the LEFT eye. Each pass's
// `native_world_drawn` copies its frame into that eye's OpenXR swapchain
// image; the left pass, seeing the right image already copied, releases both
// images and submits the projection layer to `xrEndFrame` — simultaneous
// stereo with depth, as Java Vivecraft renders.

fn vivecraft_stereo_second_pass(game: &mut crate::game::Game<'_>) {
    if !game.native_on(crate::game::vivecraft_mod::ID) {
        return;
    }
    let active = crate::gfx::vr_bridge::with_runtime(|rt| {
        rt.settings.vr_enabled
            && rt.settings.stereo_rendering
            && rt.xr.eye_textures_ready()
            && !rt.xr.stereo_right_done()
    });
    if !active {
        return;
    }
    // Acquire both eyes' swapchain images for this frame, then render the
    // right eye's pass. `native_world_drawn` copies it into the right image.
    crate::gfx::vr_bridge::with_runtime(|rt| {
        rt.xr.acquire_openr_images();
        rt.xr.set_stereo_eye(1);
        rt.xr.set_stereo_right_done(false);
    });
    let _ = game.render();
    crate::gfx::vr_bridge::with_runtime(|rt| {
        rt.xr.set_stereo_eye(0);
    });
    // The original body now renders the left eye; its `native_world_drawn`
    // blits the left image and — seeing the right done — releases and
    // submits the frame.
}