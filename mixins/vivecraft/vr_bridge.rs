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
    use wgpu::hal::api::Vulkan;

    let (Some(size), Some(handle)) = vivecraft_vr::with_runtime(|rt| {
        (rt.xr.swapchain_size, rt.xr.vulkan_image_handle(eye))
    }) else {
        return;
    };
    // Safety: `as_hal` borrows the device's Vulkan handles read-only for the
    // duration of this guard; the game keeps the device alive across frames.
    let Some(hal_device) = (unsafe { world.device.as_hal::<Vulkan>() }) else {
        log::warn!("[ViveCraft] VR blit skipped: not on the Vulkan backend");
        return;
    };

    let extent = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let src_extent = wgpu::Extent3d {
        width: world.size[0],
        height: world.size[1],
        depth_or_array_layers: 1,
    };
    // Safety: the VkImage comes from this runtime's swapchain, created for
    // the session bound to the same Vulkan instance/device wgpu runs on.
    // External memory: wgpu does not own or free it.
    let hal_texture = unsafe {
        use wgpu::hal::Device as _;
        hal_device.texture_from_raw(
            wgpu::hal::vulkan::vk::Image::from_raw(handle as usize),
            &wgpu::hal::TextureDescriptor {
                label: Some("ViveCraft OpenXR eye image"),
                size: extent,
                dimension: wgpu::hal::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::hal::TextureUses::COPY_DST,
                mip_level_count: 1,
                sample_count: 1,
                view_formats: &[],
            },
            None,
            wgpu::hal::vulkan::TextureMemory::External,
        )
    };
    // Safety: wrapping an external texture whose lifetime the runtime owns.
    let eye_texture = unsafe {
        world.device.create_texture_from_hal::<Vulkan>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("ViveCraft OpenXR eye"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::hal::TextureUses::COPY_DST,
        )
    };
    // Copy the frame's colour target into this eye, letterboxed.
    let scale = {
        let sx = extent.width as f32 / src_extent.width.max(1) as f32;
        let sy = extent.height as f32 / src_extent.height.max(1) as f32;
        sx.min(sy)
    };
    let dst_w = (src_extent.width as f32 * scale) as u32;
    let dst_h = (src_extent.height as f32 * scale) as u32;
    world.encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: world.color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: &eye_texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: (extent.width - dst_w) / 2,
                y: (extent.height - dst_h) / 2,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: dst_w,
            height: dst_h,
            depth_or_array_layers: 1,
        },
    );
    eye_texture.destroy();
}
