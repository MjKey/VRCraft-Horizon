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
