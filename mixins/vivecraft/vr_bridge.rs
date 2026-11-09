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

/// The renderer's `native_world_drawn` hook: once the frame's world picture is
/// complete, copy it into both OpenXR swapchain images and submit the frame to
/// the VR compositor (the render half of the frame loop `native_frame`'s
/// `xrWaitFrame`/`xrBeginFrame` started).
///
/// Each eye's `XrSwapchainImageVulkan2KHR` holds a native `VkImage`; on the
/// Vulkan backend it is wrapped as a `wgpu` texture (`create_texture_from_hal`)
/// and the frame's colour target is copied into it. The copy letterboxes when
/// the eye image and window aspect ratios differ, as a mirror view does.
pub fn on_world_drawn(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    // 1. Acquire this frame's swapchain images (and their VkImage handles).
    let pending = vivecraft_vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled || !rt.xr.frame_pending {
            return false;
        }
        rt.xr.acquire_openr_images();
        true
    });
    if !pending {
        return;
    }

    // 2. Copy the frame's colour target into both eyes' swapchain images.
    blit_frame_to_eyes(world);

    // 3. Release the images and submit the projection layer to xrEndFrame.
    vivecraft_vr::with_runtime(|rt| {
        rt.xr.release_openr_images();
        if let Some(state) = rt.xr.take_frame_state() {
            rt.xr.end_frame(state);
        }
    });
}

/// Copies the frame's rendered colour target into both eyes' OpenXR swapchain
/// images through wgpu's Vulkan interop. On any other backend (DX12/OpenGL) or
/// when a handle is missing, this is a no-op and `xrEndFrame` still submits
/// with the runtime's pacing.
fn blit_frame_to_eyes(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    use wgpu::hal::api::Vulkan;

    let Some(size) = vivecraft_vr::with_runtime(|rt| rt.xr.swapchain_size) else {
        return;
    };
    let handles: [Option<u64>; 2] = vivecraft_vr::with_runtime(|rt| {
        [rt.xr.vulkan_image_handle(0), rt.xr.vulkan_image_handle(1)]
    });
    if handles.iter().all(Option::is_none) {
        return;
    }
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
    for handle in handles.into_iter().flatten() {
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
}
