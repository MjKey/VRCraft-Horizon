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

/// The menu frame is drawn (`menu_frame` RETURN hook, at the end of the
/// title/loading frames' `draw_overlay`): the whole picture (panorama,
/// buttons, HUD) is complete in `target_texture`. Copies it into BOTH eyes'
/// OpenXR swapchain images — one render, blitted twice, per the spec's
/// "known simplification" (no stereo parallax in the menu) — then releases
/// the images and submits the projection layer with a single `xrEndFrame`
/// (never eye alternation).
///
/// This is also where the session is first created in a menu run: the menu
/// has no `native_world_drawn` call, so without this the XR session would
/// only come up once a world was entered. `create_session_with_vulkan` is a
/// no-op once a session exists, so menu → world → menu transitions keep one
/// living session.
pub fn on_menu_frame_drawn(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target_texture: &wgpu::Texture,
    size: [u32; 2],
) {
    // First frame on Vulkan: create the OpenXR session bound to the game's
    // own device (what SteamVR needs before swapchains exist). No-op on any
    // other backend or once a session is up.
    vivecraft_vr::with_runtime(|rt| {
        if rt.settings.vr_enabled && rt.xr.instance_connected() {
            let handles = vivecraft_vr::blit::vulkan_handles(device, queue);
            if let Some((inst, phys, dev, family, index)) = handles {
                rt.xr.create_session_with_vulkan(inst, phys, dev, family, index);
            }
        }
    });

    let pending = vivecraft_vr::with_runtime(|rt| {
        rt.settings.vr_enabled
            && rt.xr.frame_pending
            && rt.xr.eye_textures_ready()
            && !rt.xr.stereo_right_done()
    });
    if !pending {
        // A frame is open but the images are not usable (session starting up,
        // non-Vulkan backend): end it empty so the runtime keeps pacing.
        vivecraft_vr::with_runtime(|rt| {
            if rt.xr.frame_pending {
                if let Some(state) = rt.xr.take_frame_state() {
                    rt.xr.end_frame(state);
                }
                rt.xr.set_stereo_right_done(false);
            }
        });
        return;
    }

    // Acquire both eyes' images for this frame, blit the one menu picture
    // into each, then hand both to the compositor in one xrEndFrame.
    vivecraft_vr::with_runtime(|rt| rt.xr.acquire_openr_images());
    let mut copied = false;
    for eye in 0..2usize {
        let blitted = blit_menu_picture_to_eye(device, queue, target_texture, size, eye);
        copied |= blitted;
    }
    let _ = copied;
    vivecraft_vr::with_runtime(|rt| {
        rt.xr.release_openr_images();
        if let Some(state) = rt.xr.take_frame_state() {
            rt.xr.end_frame(state);
        }
        rt.xr.set_stereo_right_done(false);
    });
}

/// Copies the finished menu picture into one eye's OpenXR swapchain image
/// through wgpu's Vulkan interop (the eye's own encoder, submitted on the
/// frame's queue before the images are released). No-op on any other backend
/// or when a handle is missing.
fn blit_menu_picture_to_eye(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: &wgpu::Texture,
    source_size: [u32; 2],
    eye: usize,
) -> bool {
    let Some(size) = vivecraft_vr::with_runtime(|rt| rt.xr.swapchain_size) else {
        return false;
    };
    let Some(handle) = vivecraft_vr::with_runtime(|rt| rt.xr.vulkan_image_handle(eye)) else {
        return false;
    };
    let format = vivecraft_vr::with_runtime(|rt| rt.xr.swapchain_format);
    // The unsafe interop lives in `vivecraft_vr::blit` (this crate forbids
    // unsafe code); the encoder is submitted immediately so the images are
    // safe to release right after (the runtime's fences cover the GPU work).
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ViveCraft menu VR blit"),
    });
    let blitted = vivecraft_vr::blit::blit_frame_to_eye_safe(
        device,
        &mut encoder,
        color,
        source_size,
        handle,
        format,
        [size.0, size.1],
    );
    queue.submit([encoder.finish()]);
    blitted
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
