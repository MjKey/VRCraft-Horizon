// The world pass's view-projection: picks the active stereo eye's view from
// the `ViewSet` instead of always the primary (left) one, so the second
// render pass draws the world from the right eye (`VRRenderer.java`'s per-eye
// projection, as Java Vivecraft renders).

fn vivecraft_stereo_view_proj(
    views: &crate::gfx::view::ViewSet,
    pose: glam::Mat4,
) -> glam::Mat4 {
    let eye = vivecraft_vr::with_runtime(|rt| {
        if rt.settings.vr_enabled && rt.settings.stereo_rendering {
            rt.xr.stereo_eye()
        } else {
            0
        }
    });
    let list = views.views();
    if eye as usize >= list.len() {
        return views.primary().view_proj(pose);
    }
    list[eye as usize].view_proj(pose)
}