// VR in the main menu: `Renderer::draw_overlay` returns once the complete
// frame picture (panorama + title buttons + HUD — everything the frame
// shows) is encoded into `target_texture` and the encoder is still open
// (`draw_frame` finishes/submits it after this returns). On title/menu
// frames `native_world_drawn` never runs (`if !frame.title` in the engine),
// so this RETURN hook is the one point where a menu frame is complete with
// the render target at hand: copy it into BOTH eyes' OpenXR swapchain
// images and submit the frame (`xrEndFrame`). One desktop render, blitted
// twice — the spec's "known simplification": no stereo parallax in the
// menu, but both eyes receive the frame inside ONE `xrEndFrame` (never eye
// alternation).
//
// The blit rides in the frame's own command buffer (the encoder is a
// parameter, still open), so it is submitted with the frame; the swapchain
// images are released right after — `xrReleaseSwapchainImage` hands
// ownership back to the runtime (not a GPU fence); the runtime waits on the
// swapchain's own fences, which the frame's submit satisfies.
//
// Session creation is duplicated here from `on_world_drawn`: the menu has
// no `world_drawn` call, so without this a session would first appear only
// once a world was entered. `create_session_with_vulkan` is a no-op once a
// session exists, so both call sites coexist safely across
// menu → world → menu transitions (the session survives; the pose side
// lives in `VrClientRuntime::on_frame`, which runs in menus too).
//
// Helpers compile inside gfx.rs, whose file is `#![forbid(unsafe_code)]`:
// all the unsafe Vulkan interop stays in `vivecraft_vr::blit` (reached via
// `vr_bridge`), exactly as `world_drawn` does.

fn vivecraft_menu_frame(
    value: Option<(ScreenshotRequest, wgpu::Buffer)>,
    renderer: &mut crate::gfx::Renderer,
    frame: &crate::gfx::Frame<'_>,
    target_texture: &wgpu::Texture,
) -> Option<(ScreenshotRequest, wgpu::Buffer)> {
    // Only title/menu frames take this path; world frames go through
    // `native_world_drawn` (true stereo, both passes). Loading frames
    // (`loading_frame`) are title frames too and reach the eyes the same
    // way, so world entry/loading shows in VR as well.
    if frame.title {
        crate::gfx::vr_bridge::on_menu_frame_drawn(
            &renderer.device,
            &renderer.queue,
            target_texture,
            [renderer.config.width, renderer.config.height],
        );
    }
    value
}
