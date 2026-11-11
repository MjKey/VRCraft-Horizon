//! The Vulkan interop blit: copies a rendered frame into an OpenXR swapchain
//! image (a raw `VkImage` the runtime owns), through wgpu's hal layer — as
//! `horizon-hdr` wraps Vulkan handles, but for VR eye images.
//!
//! The render crate forbids unsafe code; this crate (the mod's own engine
//! crate) owns every unsafe block of the interop.

use ash::vk::{self, Handle as _};
use wgpu::hal::api::Vulkan;

/// Extracts the raw Vulkan handles (instance, physical device, device, queue
/// family/index) of the game's wgpu Vulkan backend — everything an OpenXR
/// graphics binding needs. `None` on any other backend.
pub fn vulkan_handles(
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
) -> Option<(u64, u64, u64, u32, u32)> {
    // SAFETY: `as_hal` borrows the device's Vulkan handles read-only for the
    // duration of the guard; the game keeps both alive.
    let hal_device = unsafe { device.as_hal::<Vulkan>() }?;
    Some((
        hal_device.shared_instance().raw_instance().handle().as_raw(),
        hal_device.raw_physical_device().as_raw(),
        hal_device.raw_device().handle().as_raw(),
        hal_device.queue_family_index(),
        hal_device.queue_index(),
    ))
}

/// Maps a Vulkan `VkFormat` (as enumerated by OpenXR's swapchain formats) to
/// the matching wgpu texture format. Only the two formats the loader prefers
/// (37 = `VK_FORMAT_B8G8R8A8_SRGB`, 50 = `VK_FORMAT_R8G8B8A8_SRGB`) map; any
/// other format means no blit (the frame is still presented to the desktop).
pub fn eye_wgpu_format(vk_format: i64) -> Option<wgpu::TextureFormat> {
    match vk_format {
        37 => Some(wgpu::TextureFormat::Bgra8UnormSrgb),
        50 => Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        _ => None,
    }
}

/// Safe wrapper for `on_world_drawn` in the render crate (which forbids
/// unsafe code): blits this pass's rendered frame into one eye's OpenXR
/// swapchain image. No-op on any other backend or a missing handle.
pub fn blit_frame_to_eye_safe(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::Texture,
    source_size: [u32; 2],
    vk_image: u64,
    vk_format: i64,
    size: [u32; 2],
) -> bool {
    let Some(format) = eye_wgpu_format(vk_format) else {
        log::warn!(target: "horizon_vivecraft", "[ViveCraft] VR blit skipped: unsupported swapchain format {vk_format}");
        return false;
    };
    // SAFETY: the `VkImage` comes from this runtime's swapchain, created for
    // the session bound to the same Vulkan instance/device wgpu runs on; the
    // image is in a copy-destination-compatible layout (the runtime's
    // acquire+wait put it there); external memory, so wgpu never frees it.
    unsafe { blit_into_vk_image(device, encoder, color, source_size, vk_image, format, size) }
}

/// Wraps a raw `VkImage` (from `XrSwapchainImageVulkan2KHR`) as a wgpu
/// texture and encodes a letterboxed `copy_texture_to_texture` of `source`
/// into it. No-op (false) when the backend is not Vulkan.
///
/// # Safety
/// - `vk_image` must be a live `VkImage` of `size` and format, created on the
///   same Vulkan instance/device as `device`, with an image layout compatible
///   with a copy destination (the OpenXR runtime waits the swapchain image
///   into `COLOR_ATTACHMENT_OPTIMAL`/`TRANSFER_DST_OPTIMAL` before this).
/// - The encoded copy must be submitted and complete before the image is
///   released back to the runtime.
pub unsafe fn blit_into_vk_image(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    source_size: [u32; 2],
    vk_image: u64,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> bool {
    // Safety: `as_hal` borrows the device's Vulkan handles read-only while
    // the guard lives; the caller keeps `device` alive across the frame.
    let Some(hal_device) = (unsafe { device.as_hal::<Vulkan>() }) else {
        log::warn!(target: "horizon_vivecraft", "[ViveCraft] VR blit skipped: not on the Vulkan backend");
        return false;
    };

    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let src_extent = wgpu::Extent3d {
        width: source_size[0],
        height: source_size[1],
        depth_or_array_layers: 1,
    };

    // Safety: the raw handle is wrapped, not owned — `TextureMemory::External`
    // keeps wgpu from freeing it, and the destroy below only drops wgpu's
    // bookkeeping, not the VkImage.
    let hal_texture = unsafe {
        hal_device.texture_from_raw(
            ash::vk::Image::from_raw(vk_image),
            &wgpu::hal::TextureDescriptor {
                label: Some("ViveCraft OpenXR eye image"),
                size: extent,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUses::COPY_DST,
                mip_level_count: 1,
                sample_count: 1,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: Vec::new(),
            },
            None,
            wgpu::hal::vulkan::TextureMemory::External,
        )
    };
    // Safety: wrapping an external texture whose lifetime the runtime owns.
    let eye_texture = unsafe {
        device.create_texture_from_hal::<Vulkan>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("ViveCraft OpenXR eye"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::TextureUses::COPY_DST,
        )
    };

    // §2.6: acquired OpenXR swapchain images arrive in UNDEFINED layout;
    // wgpu's copy_texture_to_texture assumes TRANSFER-compatible layouts and
    // records no transition for an externally-wrapped image. Transition the
    // image into TRANSFER_DST_OPTIMAL before the copy — via a raw
    // vkCmdPipelineBarrier on the same command buffer wgpu records into, so
    // ordering with the copy (and with the reverse transition below) is
    // guaranteed within the encoder's submission.
    unsafe {
        transition_openxr_image_layout(
            device,
            encoder,
            vk_image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
    }

    // Letterbox the source into the eye image when the aspect ratios differ.
    let scale = {
        let sx = extent.width as f32 / src_extent.width.max(1) as f32;
        let sy = extent.height as f32 / src_extent.height.max(1) as f32;
        sx.min(sy)
    };
    let dst_w = (src_extent.width as f32 * scale) as u32;
    let dst_h = (src_extent.height as f32 * scale) as u32;
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
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
    // Drop wgpu's wrapper only: the VkImage stays the runtime's.
    eye_texture.destroy();

    // §2.6 (cont.): transition back to UNDEFINED? No — the OpenXR
    // specification requires the app to leave a swapchain image in
    // TRANSFER_SRC_OPTIMAL / COLOR_ATTACHMENT_OPTIMAL / etc. only if it was
    // acquired in it; acquired images are in UNDEFINED and the runtime
    // treats the image contents at release as authored in whatever layout
    // the app last transitioned it to *through the normal mechanism*.
    // Practically: leave it in TRANSFER_DST_OPTIMAL (a valid release
    // layout, the same one a transfer-authored app would leave) — runtimes
    // (SteamVR/Monado/Oculus) accept any layout at release and insert their
    // own barriers. Do NOT transition back to UNDEFINED after writing.
    true
}

/// Records a `vkCmdPipelineBarrier` on wgpu's own command buffer (via
/// `CommandEncoder::as_hal_mut`) transitioning a raw OpenXR `VkImage` between
/// layouts (§2.6). The barrier is ordered with wgpu's own commands because it
/// goes into the same command buffer wgpu finishes and submits.
///
/// # Safety
/// - `vk_image` must be a live `VkImage` on the same Vulkan device.
/// - `new_layout` must be a valid destination for the image's current state.
/// - The encoder must not be inside a pass.
unsafe fn transition_openxr_image_layout(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    vk_image: u64,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) -> bool {
    let image = vk::Image::from_raw(vk_image);
    let aspect = vk::ImageAspectFlags::COLOR;
    let subresource = vk::ImageSubresourceRange {
        aspect_mask: aspect,
        base_mip_level: 0,
        level_count: vk::REMAINING_MIP_LEVELS,
        base_array_layer: 0,
        layer_count: vk::REMAINING_ARRAY_LAYERS,
    };
    // The encoder records a copy right after this, so the barrier only needs
    // to order against transfer-stage accesses (top/bottom-of-pipe as source
    // stage is valid for a layout transition into TRANSFER_DST).
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(subresource)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
    // Safety: `as_hal_mut` hands the raw Vulkan command buffer wgpu is
    // currently recording; the barrier is recorded into it in order, and the
    // wgpu encoder is untouched otherwise (we do not end the command buffer).
    // The `ash::Device` is reached through the wgpu `Device`, whose borrow
    // outlives this call: both live for the whole frame's rendering scope.
    let raw_device_ptr: Option<*const ash::Device> = {
        // SAFETY: `as_hal` borrows the device's Vulkan handles read-only; the
        // pointer stays valid as long as the wgpu Device lives — the caller
        // holds `device` across this entire function.
        match unsafe { device.as_hal::<Vulkan>() } {
            Some(hal_device) => Some(hal_device.raw_device() as *const ash::Device),
            None => {
                log::warn!(target: "horizon_vivecraft", 
                    "[ViveCraft] VR image layout transition skipped: not on the Vulkan backend"
                );
                return false;
            }
        }
    };
    unsafe {
        encoder.as_hal_mut::<Vulkan, _, _>(|hal_encoder| {
            let Some(hal_encoder) = hal_encoder else {
                log::warn!(target: "horizon_vivecraft", 
                    "[ViveCraft] VR image layout transition skipped: no hal encoder"
                );
                return;
            };
            let Some(ptr) = raw_device_ptr else { return };
            // SAFETY: pointer obtained above from the still-borrowed `device`.
            let raw_device = &*ptr;
            let cmd = hal_encoder.raw_handle();
            raw_device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_srgb_formats_map() {
        assert_eq!(
            eye_wgpu_format(37),
            Some(wgpu::TextureFormat::Bgra8UnormSrgb)
        );
        assert_eq!(
            eye_wgpu_format(50),
            Some(wgpu::TextureFormat::Rgba8UnormSrgb)
        );
    }

    #[test]
    fn unsupported_format_is_none() {
        assert_eq!(eye_wgpu_format(0), None);
        assert_eq!(eye_wgpu_format(44), None); // RGBA8 UNORM (non-sRGB)
    }
}
