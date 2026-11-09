// Bridge between `horizon-client-render` (`ViewSet`) and `vivecraft-vr`.
// Re-exports `vivecraft_vr` so `horizon-client` can access VR state without a duplicate crate dependency.

pub use vivecraft_vr::*;

use super::view::View;

/// Builds a stereo pair `(left, right, cull)` of `View`s from the base flat-screen `View`
/// when ViveCraft VR rendering is active for the current frame.
pub fn stereo_views(base: View) -> Option<(View, View, View)> {
    let pair = vivecraft_vr::current_stereo_view_pair(
        base.view,
        base.projection,
        base.aspect,
        base.tan_half_fov,
    )?;
    let left = View {
        view: pair.left.view,
        projection: pair.left.projection,
        aspect: pair.left.aspect,
        tan_half_fov: pair.left.tan_half_fov,
    };
    let right = View {
        view: pair.right.view,
        projection: pair.right.projection,
        aspect: pair.right.aspect,
        tan_half_fov: pair.right.tan_half_fov,
    };
    let cull = View {
        view: pair.cull.view,
        projection: pair.cull.projection,
        aspect: pair.cull.aspect,
        tan_half_fov: pair.cull.tan_half_fov,
    };
    Some((left, right, cull))
}

/// The renderer's `native_world_drawn` hook: once a pass's world picture is
/// complete, copy it into that eye's OpenXR swapchain image.
///
/// `stereo_second_pass` renders the RIGHT eye first (a full engine render with
/// `stereo_eye` 1 — `views.primary()` carries the right eye's view), then the
/// frame's own render draws the LEFT eye. This hook blits each pass's frame
/// into its eye's swapchain image; the LEFT pass, finding the right image
/// already copied, releases both images and submits the projection layer to
/// `xrEndFrame` — simultaneous stereo with depth, as Java Vivecraft renders.
pub fn on_world_drawn(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    // First frame on Vulkan: create the OpenXR session bound to the game's
    // own device (what SteamVR needs before swapchains exist). No-op on any
    // other backend or once a session is up.
    vivecraft_vr::with_runtime(|rt| {
        if rt.settings.vr_enabled && rt.xr.instance_connected() {
            let handles = vivecraft_vr::blit::vulkan_handles(world.device, world.queue);
            if let Some((inst, phys, dev, family, index)) = handles {
                rt.xr.create_session_with_vulkan(inst, phys, dev, family, index);
            }
        }
    });

    let (pending, eye, right_done) = vivecraft_vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled || !rt.xr.frame_pending {
            return (false, 0u8, false);
        }
        (
            true,
            rt.xr.stereo_eye(),
            rt.xr.stereo_right_done(),
        )
    });
    if !pending {
        return;
    }

    // Copy this pass's rendered frame into its eye's swapchain image.
    blit_frame_to_eye(world, eye as usize);

    if eye == 1 {
        // Right image copied; the left pass follows and will submit.
        vivecraft_vr::with_runtime(|rt| rt.xr.set_stereo_right_done(true));
        return;
    }
    if !right_done {
        // The right pass never ran (hardware lost mid-frame): release and
        // submit with whatever the images hold, keeping the runtime's pacing.
        vivecraft_vr::with_runtime(|rt| {
            rt.xr.release_openr_images();
            if let Some(state) = rt.xr.take_frame_state() {
                rt.xr.end_frame(state);
            }
        });
        return;
    }
    // Left image copied, right was too: hand both eyes to the compositor.
    vivecraft_vr::with_runtime(|rt| {
        rt.xr.release_openr_images();
        if let Some(state) = rt.xr.take_frame_state() {
            rt.xr.end_frame(state);
        }
        rt.xr.set_stereo_right_done(false);
    });
}

/// Copies the frame's rendered colour target into one eye's OpenXR swapchain
/// image through wgpu's Vulkan interop. On any other backend (DX12/OpenGL) or
/// when a handle is missing, this is a no-op (the frame is still submitted).
fn blit_frame_to_eye(world: &mut crate::gfx::native_render::WorldDrawn<'_>, eye: usize) {
    let Some(size) = vivecraft_vr::with_runtime(|rt| rt.xr.swapchain_size) else {
        return;
    };
    let Some(handle) = vivecraft_vr::with_runtime(|rt| rt.xr.vulkan_image_handle(eye)) else {
        return;
    };
    let format = vivecraft_vr::with_runtime(|rt| rt.xr.swapchain_format);
    // The unsafe interop lives in `vivecraft_vr::blit` (this crate forbids
    // unsafe code); this safe wrapper handles the format mapping.
    vivecraft_vr::blit::blit_frame_to_eye_safe(
        world.device,
        world.encoder,
        world.color,
        world.size,
        handle,
        format,
        [size.0, size.1],
    );
}
