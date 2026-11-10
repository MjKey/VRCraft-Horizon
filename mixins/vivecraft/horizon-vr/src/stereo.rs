//! Stereo view and projection matrices for OpenXR and Horizon's Reversed-Z WGPU pipeline
//! (ported from `VRRenderer.java` and `horizon-client-render/src/gfx/view.rs`).

use crate::xr::XrFovf;
use glam::{Mat4, Vec3, Vec4};

/// Vanilla / Horizon near clipping plane (`Camera.NEAR_PLANE = 0.05`).
pub const NEAR_PLANE: f32 = 0.05;

/// One eye's view parameters matching `horizon_client_render::gfx::View`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoEyeView {
    /// Camera-relative world to eye matrix (rotation + eye offset from HMD center).
    pub view: Mat4,
    /// Reversed-Z perspective projection matrix (`near -> 1.0`, `far -> 0.0`).
    pub projection: Mat4,
    /// Width over height of the eye viewport.
    pub aspect: f32,
    /// `tan(fov_y / 2)` for shaders reconstructing view rays.
    pub tan_half_fov: f32,
}

/// Complete stereo pair (`Left`, `Right`) plus a combined culling view that encompasses both eyes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoViewPair {
    pub left: StereoEyeView,
    pub right: StereoEyeView,
    pub cull: StereoEyeView,
}

/// Builds an asymmetric OpenXR perspective projection matrix using Horizon's Reversed-Z
/// right-handed `[0, 1]` clip-space convention (`near -> 1.0`, `far -> 0.0`).
///
/// When `fov` is symmetric (`-angle_left == angle_right`, `angle_up == -angle_down`), this
/// matches `glam::Mat4::perspective_rh(fov_y, aspect, far, near)`.
pub fn openxr_perspective_reversed_z_rh(fov: XrFovf, near: f32, far: f32) -> (Mat4, f32, f32) {
    let tan_left = fov.angle_left.tan();
    let tan_right = fov.angle_right.tan();
    let tan_up = fov.angle_up.tan();
    let tan_down = fov.angle_down.tan();

    let tan_width = (tan_right - tan_left).max(1e-5);
    let tan_height = (tan_up - tan_down).max(1e-5);

    let m00 = 2.0 / tan_width;
    let m11 = 2.0 / tan_height;
    let m20 = (tan_right + tan_left) / tan_width;
    let m21 = (tan_up + tan_down) / tan_height;

    // Reversed-Z right-handed [0, 1]: z_ndc = z_near * z_far / (z_far - z_near) ...
    // Standard glam::Mat4::perspective_rh(fov, aspect, z_near=far, z_far=near):
    // r = 1.0 / (far - near)
    // m22 = near * r
    // m32 = far * near * r
    let r = 1.0 / (far - near);
    let m22 = near * r;
    let m32 = far * near * r;

    let proj = Mat4::from_cols(
        Vec4::new(m00, 0.0, 0.0, 0.0),
        Vec4::new(0.0, m11, 0.0, 0.0),
        Vec4::new(m20, m21, m22, -1.0),
        Vec4::new(0.0, 0.0, m32, 0.0),
    );

    let tan_half_fov = 0.5 * tan_height;
    let aspect = tan_width / tan_height;
    (proj, aspect, tan_half_fov)
}

/// Builds a `StereoViewPair` from a base camera view matrix, base projection parameters,
/// and stereo IPD / eye offsets in meters.
pub fn build_stereo_view_pair(
    base_view: Mat4,
    base_projection: Mat4,
    base_aspect: f32,
    base_tan_half_fov: f32,
    ipd_meters: f32,
    world_scale: f32,
    far_plane: f32,
) -> StereoViewPair {
    build_stereo_view_pair_with_fov(
        base_view,
        base_projection,
        base_aspect,
        base_tan_half_fov,
        ipd_meters,
        world_scale,
        far_plane,
        None,
    )
}

/// Builds a `StereoViewPair`, overriding the screen-derived projection with the
/// runtime's per-eye OpenXR FOVs (§2.3) when provided: `eye_fovs[0]` = left,
/// `eye_fovs[1]` = right. Eye views still get the IPD translation (from the
/// HMD-relative eye poses the base view encodes), so a hardware FOV changes
/// only the projection, not the view.
pub fn build_stereo_view_pair_with_fov(
    base_view: Mat4,
    base_projection: Mat4,
    base_aspect: f32,
    base_tan_half_fov: f32,
    ipd_meters: f32,
    world_scale: f32,
    far_plane: f32,
    eye_fovs: Option<[XrFovf; 2]>,
) -> StereoViewPair {
    let half_ipd = (ipd_meters * 0.5 * world_scale).clamp(0.0, 0.25);

    // Eye translation in eye space: Left eye shifts world by +half_ipd in X, Right eye by -half_ipd in X.
    let left_view = Mat4::from_translation(Vec3::new(half_ipd, 0.0, 0.0)) * base_view;
    let right_view = Mat4::from_translation(Vec3::new(-half_ipd, 0.0, 0.0)) * base_view;

    // Per-eye projection from the runtime's FOV when available (§2.3).
    let vr_eye = |fov: XrFovf| -> (Mat4, f32, f32) {
        openxr_perspective_reversed_z_rh(fov, NEAR_PLANE, far_plane)
    };
    let (left_proj, left_aspect, left_tan) = match eye_fovs {
        Some(fovs) => vr_eye(fovs[0]),
        None => (base_projection, base_aspect, base_tan_half_fov),
    };
    let (right_proj, right_aspect, right_tan) = match eye_fovs {
        Some(fovs) => vr_eye(fovs[1]),
        None => (base_projection, base_aspect, base_tan_half_fov),
    };

    let left = StereoEyeView {
        view: left_view,
        projection: left_proj,
        aspect: left_aspect,
        tan_half_fov: left_tan,
    };

    let right = StereoEyeView {
        view: right_view,
        projection: right_proj,
        aspect: right_aspect,
        tan_half_fov: right_tan,
    };

    // Expand the culling frustum slightly horizontally and back off slightly so geometry visible
    // in either the left or right eye is never prematurely culled. With a runtime FOV, the
    // combined frustum is the union of both eye FOVs expanded by the same margin.
    let (cull_proj, cull_aspect, cull_tan_half_fov) = match eye_fovs {
        Some(fovs) => {
            // Union FOV: widest angle on each side across both eyes.
            let union = XrFovf {
                angle_left: f32::min(fovs[0].angle_left, fovs[1].angle_left),
                angle_right: f32::max(fovs[0].angle_right, fovs[1].angle_right),
                angle_up: f32::max(fovs[0].angle_up, fovs[1].angle_up),
                angle_down: f32::min(fovs[0].angle_down, fovs[1].angle_down),
            };
            // 8% margin on each tangent, as the base-FOV path applies.
            let expand = |a: f32, _widen: bool| -> f32 {
                (a.tan() * 1.08).atan()
            };
            let widen = true;
            let widened = XrFovf {
                angle_left: expand(union.angle_left, widen),
                angle_right: expand(union.angle_right, widen),
                angle_up: expand(union.angle_up, widen),
                angle_down: expand(union.angle_down, widen),
            };
            let (proj, aspect, tan) =
                openxr_perspective_reversed_z_rh(widened, NEAR_PLANE, far_plane);
            (proj, aspect, tan)
        }
        None => {
            let cull_tan_half_fov = base_tan_half_fov * 1.08;
            let cull_fov_rad = 2.0 * cull_tan_half_fov.atan();
            let cull_proj =
                Mat4::perspective_rh(cull_fov_rad, base_aspect, far_plane.max(16.0), NEAR_PLANE);
            (cull_proj, base_aspect, cull_tan_half_fov)
        }
    };
    let cull_view = Mat4::from_translation(Vec3::new(0.0, 0.0, -half_ipd)) * base_view;

    let cull = StereoEyeView {
        view: cull_view,
        projection: cull_proj,
        aspect: cull_aspect,
        tan_half_fov: cull_tan_half_fov,
    };

    StereoViewPair { left, right, cull }
}

/// Computes the comfort vignette factor (`0.0..=max_vignette`) based on player linear velocity
/// and angular rotational speed to reduce VR motion sickness during movement.
pub fn calculate_comfort_vignette(
    linear_speed_m_per_s: f32,
    angular_speed_deg_per_s: f32,
    max_vignette: f32,
) -> f32 {
    let linear_factor = (linear_speed_m_per_s / 5.0).clamp(0.0, 1.0);
    let angular_factor = (angular_speed_deg_per_s / 120.0).clamp(0.0, 1.0);
    let combined = linear_factor * 0.4 + angular_factor * 0.6;
    (combined * max_vignette).clamp(0.0, max_vignette.clamp(0.0, 1.0))
}

/// Generates normalized 2D stencil boundary coordinates for a circular / elliptical optic mask
/// (hidden area mask to avoid rendering pixels clipped by VR headset lenses).
pub fn generate_lens_stencil_mask(segments: usize, scale_x: f32, scale_y: f32) -> Vec<[f32; 2]> {
    let count = segments.clamp(16, 128);
    let mut vertices = Vec::with_capacity(count);
    for i in 0..count {
        let theta = (i as f32 / count as f32) * std::f32::consts::TAU;
        let x = theta.cos() * scale_x;
        let y = theta.sin() * scale_y;
        vertices.push([x, y]);
    }
    vertices
}
