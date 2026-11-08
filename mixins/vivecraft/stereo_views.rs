// Expands `ViewSet::single` into a two-eye stereo `ViewSet` when ViveCraft VR is active.
fn vivecraft_stereo_view_set(view: crate::gfx::view::View) -> crate::gfx::view::ViewSet {
    if let Some((left, right, cull)) = crate::gfx::vr_bridge::stereo_views(view) {
        crate::gfx::view::ViewSet {
            views: [left, right],
            len: 2,
            cull,
        }
    } else {
        crate::gfx::view::ViewSet {
            views: [view, view],
            len: 1,
            cull: view,
        }
    }
}
