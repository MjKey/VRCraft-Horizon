//! Khronos OpenXR 1.0 Dynamic Loader (`libloading`) and VR Provider abstraction
//! (ported from `MCVR.java`, `MCOpenVR.java`, and `NullVR.java`).
//!
//! Dynamically loads the platform's OpenXR loader (`openxr_loader.dll` on Windows,
//! `libopenxr_loader.so.1` on Linux, `libopenxr_loader.dylib` on macOS) via `libloading`
//! with zero C/C++ build-time SDK requirements, and falls back to `SimulatedXrProvider`
//! (`NullVR`) when no physical headset runtime is active.

use glam::{Mat4, Quat, Vec2, Vec3};
use libloading::Library;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;

// ============================================================================
// Khronos OpenXR 1.0 C ABI types (`openxr.h`)
// ============================================================================

pub type XrResult = i32;
pub type XrBool32 = u32;
pub type XrFlags64 = u64;
pub type XrTime = i64;
pub type XrDuration = i64;
pub type XrVersion = u64;
pub type XrInstance = u64;
pub type XrSystemId = u64;
pub type XrSession = u64;
pub type XrSpace = u64;
pub type XrActionSet = u64;
pub type XrAction = u64;
pub type XrPath = u64;
pub type XrSwapchain = u64;
pub type XrSwapchainCreateFlags = XrFlags64;
pub type XrSwapchainUsageFlags = XrFlags64;

pub const XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT: XrSwapchainUsageFlags = 0x00000001;
pub const XR_SWAPCHAIN_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT: XrSwapchainUsageFlags = 0x00000002;
pub const XR_SWAPCHAIN_USAGE_SAMPLED_BIT: XrSwapchainUsageFlags = 0x00000008;

#[repr(C)]
pub struct XrSwapchainCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub create_flags: XrSwapchainCreateFlags,
    pub usage_flags: XrSwapchainUsageFlags,
    pub format: i64,
    pub sample_count: u32,
    pub width: u32,
    pub height: u32,
    pub face_count: u32,
    pub array_size: u32,
    pub mip_count: u32,
}

#[repr(C)]
pub struct XrSwapchainImageBaseHeader {
    pub ty: XrStructureType,
    pub next: *mut c_void,
}

// SAFETY: OpenXR swapchain image headers are opaque handles managed by the
// OpenXR runtime. They are safe to send between threads as long as the
// underlying swapchain handles are not accessed from multiple threads
// simultaneously, which is guaranteed by the Mutex wrapper in VrClientRuntime.
unsafe impl Send for XrSwapchainImageBaseHeader {}
unsafe impl Sync for XrSwapchainImageBaseHeader {}

pub type PfnCreateSwapchain = unsafe extern "system" fn(
    session: XrSession,
    create_info: *const XrSwapchainCreateInfo,
    swapchain: *mut XrSwapchain,
) -> XrResult;
pub type PfnDestroySwapchain = unsafe extern "system" fn(swapchain: XrSwapchain) -> XrResult;
pub type PfnEnumerateInstanceExtensionProperties = unsafe extern "system" fn(
    layer: *const c_char,
    property_capacity_input: u32,
    property_count_output: *mut u32,
    properties: *mut XrInstanceExtensionProperties,
) -> XrResult;
pub type PfnEnumerateSwapchainImages = unsafe extern "system" fn(
    swapchain: XrSwapchain,
    image_capacity_input: u32,
    image_count_output: *mut u32,
    images: *mut XrSwapchainImageBaseHeader,
) -> XrResult;
pub type PfnAcquireSwapchainImage = unsafe extern "system" fn(
    swapchain: XrSwapchain,
    acquire_info: *const c_void,
    index: *mut u32,
) -> XrResult;
pub type PfnWaitSwapchainImage = unsafe extern "system" fn(
    swapchain: XrSwapchain,
    wait_info: *const c_void,
) -> XrResult;
pub type PfnReleaseSwapchainImage = unsafe extern "system" fn(
    swapchain: XrSwapchain,
    release_info: *const c_void,
) -> XrResult;

#[repr(C)]
pub struct XrFrameWaitInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

#[repr(C)]
pub struct XrFrameState {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub predicted_display_time: XrTime,
    pub predicted_display_period: XrDuration,
    pub should_render: XrBool32,
}

// SAFETY: OpenXR frame state is an opaque POD filled by `xrWaitFrame`; the
// `next` chain is never dereferenced by ViveCraft. Guarded by the runtime's Mutex.
unsafe impl Send for XrFrameState {}

#[repr(C)]
pub struct XrFrameBeginInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

#[repr(C)]
pub struct XrFrameEndInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub display_time: XrTime,
    pub environment_blend_mode: XrEnvironmentBlendMode,
    pub layer_count: u32,
    pub layers: *const *const c_void,
}

#[repr(C)]
pub struct XrCompositionLayerBaseHeader {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub layer_flags: XrFlags64,
    pub space: XrSpace,
}

#[repr(C)]
pub struct XrCompositionLayerProjectionView {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub pose: XrPosef,
    pub fov: XrFovf,
    pub sub_image: XrSwapchainSubImage,
}

/// `XrSwapchainSubImage` is an INLINE sub-struct of
/// `XrCompositionLayerProjectionView`, not a chained structure: it has no
/// `type`/`next` fields (the 0.1.6 layout wrongly carried them, shifting
/// every following field and corrupting the submission layer ABI).
#[repr(C)]
pub struct XrSwapchainSubImage {
    pub swapchain: XrSwapchain,
    pub image_rect: XrRect2Di,
    pub image_array_index: u32,
}

#[repr(C)]
pub struct XrRect2Di {
    pub offset: XrOffset2Di,
    pub extent: XrExtent2Di,
}

#[repr(C)]
pub struct XrOffset2Di {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct XrExtent2Di {
    pub width: i32,
    pub height: i32,
}

#[repr(C)]
pub struct XrCompositionLayerProjection {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub layer_flags: XrFlags64,
    pub space: XrSpace,
    pub view_count: u32,
    pub views: *const XrCompositionLayerProjectionView,
}

/// `XrViewConfigurationView`: the recommended eye image rect for one view
/// (`xrEnumerateViewConfigurationViews`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XrViewConfigurationView {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub recommended_image_rect_width: u32,
    pub recommended_image_rect_height: u32,
    pub max_image_rect_width: u32,
    pub max_image_rect_height: u32,
    pub recommended_swapchain_samples: u32,
    pub max_swapchain_samples: u32,
}

pub type PfnEnumerateViewConfigurationViews = unsafe extern "system" fn(
    instance: XrInstance,
    system_id: XrSystemId,
    view_configuration_type: XrViewConfigurationType,
    view_capacity_input: u32,
    view_count_output: *mut u32,
    view_configuration_views: *mut XrViewConfigurationView,
) -> XrResult;

pub type PfnEnumerateSwapchainFormats = unsafe extern "system" fn(
    session: XrSession,
    format_capacity_input: u32,
    format_count_output: *mut u32,
    formats: *mut i64,
) -> XrResult;

/// `XrSwapchainImageAcquireInfo`.
#[repr(C)]
pub struct XrSwapchainImageAcquireInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

/// `XrSwapchainImageWaitInfo`.
#[repr(C)]
pub struct XrSwapchainImageWaitInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub timeout: XrDuration,
}

/// `XrSwapchainImageReleaseInfo`.
#[repr(C)]
pub struct XrSwapchainImageReleaseInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrEnvironmentBlendMode {
    Opaque = 1,
    Additive = 2,
    AlphaBlend = 3,
}

pub type PfnWaitFrame = unsafe extern "system" fn(
    session: XrSession,
    frame_wait_info: *const XrFrameWaitInfo,
    frame_state: *mut XrFrameState,
) -> XrResult;

pub type PfnBeginFrame = unsafe extern "system" fn(
    session: XrSession,
    frame_begin_info: *const XrFrameBeginInfo,
) -> XrResult;

pub type PfnEndFrame = unsafe extern "system" fn(
    session: XrSession,
    frame_end_info: *const XrFrameEndInfo,
) -> XrResult;

pub const XR_SUCCESS: XrResult = 0;
pub const XR_NULL_HANDLE: u64 = 0;
pub const XR_NULL_PATH: XrPath = 0;
pub const XR_NULL_SYSTEM_ID: XrSystemId = 0;
pub const XR_TRUE: XrBool32 = 1;
pub const XR_FALSE: XrBool32 = 0;

pub const XR_MAX_APPLICATION_NAME_SIZE: usize = 128;
pub const XR_MAX_ENGINE_NAME_SIZE: usize = 128;
pub const XR_MAX_SYSTEM_NAME_SIZE: usize = 256;

/// `XR_MAKE_VERSION(1, 0, 0)`
pub const XR_CURRENT_API_VERSION: XrVersion = (1u64 << 48) | (0u64 << 32) | 0u64;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
/// Discriminants verified against the official OpenXR 1.0 `XrStructureType`
/// table (openxr.h, KhronosGroup/OpenXR-SDK, checked 2026-11). Do NOT
/// renumber in sequence: the 0.1.6 enum had ~20 values off, which made
/// `GraphicsBindingVulkan2KHR` collide with `XR_TYPE_GRAPHICS_BINDING_METAL`
/// and SteamVR reject the session create outright.
pub enum XrStructureType {
    #[default]
    Unknown = 0,
    /// XR_TYPE_EXTENSION_PROPERTIES
    InstanceExtensionProperties = 2,
    InstanceCreateInfo = 3,
    SystemGetInfo = 4,
    SystemProperties = 5,
    ViewLocateInfo = 6,
    View = 7,
    SessionCreateInfo = 8,
    SwapchainCreateInfo = 9,
    SessionBeginInfo = 10,
    /// XR_TYPE_VIEW_STATE
    ViewState = 11,
    FrameEndInfo = 12,
    HapticVibration = 13,
    EventDataBuffer = 16,
    EventDataSessionStateChanged = 18,
    ActionSetCreateInfo = 28,
    ActionCreateInfo = 29,
    CompositionLayerProjection = 35,
    ReferenceSpaceCreateInfo = 37,
    ActionSpaceCreateInfo = 38,
    ViewConfigurationView = 41,
    SpaceLocation = 42,
    FrameState = 44,
    FrameBeginInfo = 46,
    FrameWaitInfo = 33,
    CompositionLayerProjectionView = 48,
    ActionStateBoolean = 23,
    ActionStateFloat = 24,
    ActionStateVector2f = 25,
    ActionStatePose = 27,
    ActionStateGetInfo = 58,
    HapticActionInfo = 59,
    SessionActionSetsAttachInfo = 60,
    ActionsSyncInfo = 61,
    InteractionProfileSuggestedBinding = 51,
    /// XR_TYPE_SWAPCHAIN_IMAGE_VULKAN_KHR (alias of Vulkan2)
    SwapchainImageVulkan2KHR = 1000025001,
    SwapchainImageAcquireInfo = 55,
    SwapchainImageWaitInfo = 56,
    SwapchainImageReleaseInfo = 57,
    /// XR_TYPE_GRAPHICS_BINDING_VULKAN_KHR (alias of Vulkan2): 1000029000 is
    /// XR_TYPE_GRAPHICS_BINDING_METAL_KHR — the 0.1.6 value made SteamVR see
    /// a Metal binding in the session create chain and reject it.
    GraphicsBindingVulkan2KHR = 1000025000,
    VulkanGraphicsDeviceGetInfoKHR = 1000090003,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum XrSessionState {
    #[default]
    Unknown = 0,
    Idle = 1,
    Ready = 2,
    Synchronized = 3,
    Visible = 4,
    Focused = 5,
    Stopping = 6,
    LossPending = 7,
    Exiting = 8,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrActionType {
    BooleanInput = 1,
    FloatInput = 2,
    Vector2fInput = 3,
    PoseInput = 4,
    VibrationOutput = 100,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrFormFactor {
    HeadMountedDisplay = 1,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrViewConfigurationType {
    PrimaryMono = 1,
    PrimaryStereo = 2,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrReferenceSpaceType {
    View = 1,
    Local = 2,
    Stage = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XrApplicationInfo {
    pub application_name: [c_char; XR_MAX_APPLICATION_NAME_SIZE],
    pub application_version: u32,
    pub engine_name: [c_char; XR_MAX_ENGINE_NAME_SIZE],
    pub engine_version: u32,
    pub api_version: XrVersion,
}

#[repr(C)]
pub struct XrInstanceCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub create_flags: XrFlags64,
    pub application_info: XrApplicationInfo,
    pub enabled_api_layer_count: u32,
    pub enabled_api_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub enabled_extension_names: *const *const c_char,
}

#[repr(C)]
pub struct XrSystemGetInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub form_factor: XrFormFactor,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrSystemGraphicsProperties {
    pub max_swapchain_image_height: u32,
    pub max_swapchain_image_width: u32,
    pub max_layer_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrSystemTrackingProperties {
    pub orientation_tracking: XrBool32,
    pub position_tracking: XrBool32,
}

#[repr(C)]
pub struct XrSystemProperties {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub system_id: XrSystemId,
    pub vendor_id: u32,
    pub system_name: [c_char; XR_MAX_SYSTEM_NAME_SIZE],
    pub graphics_properties: XrSystemGraphicsProperties,
    pub tracking_properties: XrSystemTrackingProperties,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct XrVector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl XrVector3f {
    pub fn to_vec3(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XrQuaternionf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for XrQuaternionf {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }
}

impl XrQuaternionf {
    pub fn to_quat(self) -> Quat {
        Quat::from_xyzw(self.x, self.y, self.z, self.w).normalize()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct XrPosef {
    pub orientation: XrQuaternionf,
    pub position: XrVector3f,
}

impl XrPosef {
    pub const IDENTITY: Self = Self {
        orientation: XrQuaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        position: XrVector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    };
}

pub const XR_SPACE_LOCATION_ORIENTATION_VALID_BIT: XrFlags64 = 0x00000001;
pub const XR_SPACE_LOCATION_POSITION_VALID_BIT: XrFlags64 = 0x00000002;
pub const XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT: XrFlags64 = 0x00000004;
pub const XR_SPACE_LOCATION_POSITION_TRACKED_BIT: XrFlags64 = 0x00000008;

// `XR_VIEW_STATE_POSITION_VALID_BIT` / `XR_VIEW_STATE_ORIENTATION_VALID_BIT`
// (openxr.h, `XrViewState` flags).
pub const XR_VIEW_STATE_ORIENTATION_VALID_BIT: XrFlags64 = 0x00000001;
pub const XR_VIEW_STATE_POSITION_VALID_BIT: XrFlags64 = 0x00000002;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XrHapticVibration {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub duration: XrDuration,
    pub frequency: f32,
    pub amplitude: f32,
}

impl Default for XrHapticVibration {
    fn default() -> Self {
        Self {
            ty: XrStructureType::HapticVibration,
            next: ptr::null(),
            duration: 0,
            frequency: 0.0,
            amplitude: 0.0,
        }
    }
}

#[repr(C)]
pub struct XrViewLocateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub view_configuration_type: XrViewConfigurationType,
    pub display_time: XrTime,
    pub space: XrSpace,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrViewState {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub view_state_flags: XrFlags64,
}

pub type PfnLocateViews = unsafe extern "system" fn(
    session: XrSession,
    view_locate_info: *const XrViewLocateInfo,
    view_state: *mut XrViewState,
    view_capacity_input: u32,
    view_count_output: *mut u32,
    views: *mut XrView,
) -> XrResult;

/// Asymmetric field-of-view angles in radians (per OpenXR specification).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XrFovf {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_up: f32,
    pub angle_down: f32,
}

impl Default for XrFovf {
    fn default() -> Self {
        // Default ~100 deg symmetric FOV
        let half = 50.0_f32.to_radians();
        Self {
            angle_left: -half,
            angle_right: half,
            angle_up: half,
            angle_down: -half,
        }
    }
}

#[repr(C)]
pub struct XrView {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub pose: XrPosef,
    pub fov: XrFovf,
}

#[repr(C)]
pub struct XrSpaceLocation {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub location_flags: XrFlags64,
    pub pose: XrPosef,
}

type PfnVoidFunction = Option<unsafe extern "system" fn()>;
type PfnGetInstanceProcAddr = unsafe extern "system" fn(
    instance: XrInstance,
    name: *const c_char,
    function: *mut PfnVoidFunction,
) -> XrResult;
type PfnCreateInstance = unsafe extern "system" fn(
    create_info: *const XrInstanceCreateInfo,
    instance: *mut XrInstance,
) -> XrResult;
type PfnDestroyInstance = unsafe extern "system" fn(instance: XrInstance) -> XrResult;
type PfnGetSystem = unsafe extern "system" fn(
    instance: XrInstance,
    get_info: *const XrSystemGetInfo,
    system_id: *mut XrSystemId,
) -> XrResult;
type PfnGetSystemProperties = unsafe extern "system" fn(
    instance: XrInstance,
    system_id: XrSystemId,
    properties: *mut XrSystemProperties,
) -> XrResult;

#[repr(C)]
pub struct XrSessionCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub create_flags: XrFlags64,
    pub system_id: XrSystemId,
}

/// `XR_KHR_vulkan_enable2` binding: `XrGraphicsBindingVulkan2KHR`.
/// The Vulkan handles the OpenXR runtime must render with — taken from the
/// wgpu (Vulkan backend) instance/device/queue the game already runs on.
#[repr(C)]
pub struct XrGraphicsBindingVulkan2KHR {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub instance: u64,
    pub physical_device: u64,
    pub device: u64,
    pub queue_family_index: u32,
    pub queue_index: u32,
}

/// `XrInstanceExtensionProperties` (for querying what the runtime supports).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XrInstanceExtensionProperties {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub extension_name: [c_char; 128],
    pub extension_version: u32,
}

#[repr(C)]
pub struct XrSessionBeginInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub primary_view_configuration_type: XrViewConfigurationType,
}

#[repr(C)]
pub struct XrReferenceSpaceCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub reference_space_type: XrReferenceSpaceType,
    pub pose_in_reference_space: XrPosef,
}

#[repr(C)]
pub struct XrActionSetCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub action_set_name: [c_char; 64],
    pub localized_action_set_name: [c_char; 128],
    pub priority: u32,
}

#[repr(C)]
pub struct XrActionCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub action_name: [c_char; 64],
    pub action_type: XrActionType,
    pub count_subaction_paths: u32,
    pub subaction_paths: *const XrPath,
    pub localized_action_name: [c_char; 128],
}

#[repr(C)]
pub struct XrActionSuggestedBinding {
    pub action: XrAction,
    pub binding: XrPath,
}

#[repr(C)]
pub struct XrInteractionProfileSuggestedBinding {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub interaction_profile: XrPath,
    pub count_suggested_bindings: u32,
    pub suggested_bindings: *const XrActionSuggestedBinding,
}

#[repr(C)]
pub struct XrSessionActionSetsAttachInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub count_action_sets: u32,
    pub action_sets: *const XrActionSet,
}

#[repr(C)]
pub struct XrActionSpaceCreateInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub action: XrAction,
    pub subaction_path: XrPath,
    pub pose_in_action_space: XrPosef,
}

#[repr(C)]
pub struct XrActiveActionSet {
    pub action_set: XrActionSet,
    pub subaction_path: XrPath,
}

#[repr(C)]
pub struct XrActionsSyncInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub count_active_action_sets: u32,
    pub active_action_sets: *const XrActiveActionSet,
}

#[repr(C)]
pub struct XrActionStateGetInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub action: XrAction,
    pub subaction_path: XrPath,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrActionStateBoolean {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub current_state: XrBool32,
    pub changed_since_last_sync: XrBool32,
    pub last_change_time: XrTime,
    pub is_active: XrBool32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrActionStateFloat {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub current_state: f32,
    pub changed_since_last_sync: XrBool32,
    pub last_change_time: XrTime,
    pub is_active: XrBool32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrVector2f {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrActionStateVector2f {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub current_state: XrVector2f,
    pub changed_since_last_sync: XrBool32,
    pub last_change_time: XrTime,
    pub is_active: XrBool32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XrActionStatePose {
    pub ty: XrStructureType,
    pub next: *mut c_void,
    pub is_active: XrBool32,
}

#[repr(C)]
pub struct XrHapticActionInfo {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub action: XrAction,
    pub subaction_path: XrPath,
}

#[repr(C)]
pub struct XrHapticBaseHeader {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

#[repr(C)]
pub struct XrEventDataBaseHeader {
    pub ty: XrStructureType,
    pub next: *const c_void,
}

#[repr(C)]
pub struct XrEventDataBuffer {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub varying: [u8; 4000],
}

#[repr(C)]
pub struct XrEventDataSessionStateChanged {
    pub ty: XrStructureType,
    pub next: *const c_void,
    pub session: XrSession,
    pub state: XrSessionState,
    pub time: XrTime,
}

pub type PfnCreateSession = unsafe extern "system" fn(
    instance: XrInstance,
    create_info: *const XrSessionCreateInfo,
    session: *mut XrSession,
) -> XrResult;
pub type PfnDestroySession = unsafe extern "system" fn(session: XrSession) -> XrResult;
pub type PfnBeginSession = unsafe extern "system" fn(
    session: XrSession,
    begin_info: *const XrSessionBeginInfo,
) -> XrResult;
pub type PfnEndSession = unsafe extern "system" fn(session: XrSession) -> XrResult;
pub type PfnCreateReferenceSpace = unsafe extern "system" fn(
    session: XrSession,
    create_info: *const XrReferenceSpaceCreateInfo,
    space: *mut XrSpace,
) -> XrResult;
pub type PfnDestroySpace = unsafe extern "system" fn(space: XrSpace) -> XrResult;
pub type PfnLocateSpace = unsafe extern "system" fn(
    space: XrSpace,
    base_space: XrSpace,
    time: XrTime,
    location: *mut XrSpaceLocation,
) -> XrResult;
pub type PfnPollEvent = unsafe extern "system" fn(
    instance: XrInstance,
    event_data: *mut XrEventDataBuffer,
) -> XrResult;
pub type PfnStringToPath = unsafe extern "system" fn(
    instance: XrInstance,
    path_string: *const c_char,
    path: *mut XrPath,
) -> XrResult;
pub type PfnCreateActionSet = unsafe extern "system" fn(
    instance: XrInstance,
    create_info: *const XrActionSetCreateInfo,
    action_set: *mut XrActionSet,
) -> XrResult;
pub type PfnCreateAction = unsafe extern "system" fn(
    action_set: XrActionSet,
    create_info: *const XrActionCreateInfo,
    action: *mut XrAction,
) -> XrResult;
pub type PfnSuggestInteractionProfileBindings = unsafe extern "system" fn(
    instance: XrInstance,
    suggested_bindings: *const XrInteractionProfileSuggestedBinding,
) -> XrResult;
pub type PfnAttachSessionActionSets = unsafe extern "system" fn(
    session: XrSession,
    attach_info: *const XrSessionActionSetsAttachInfo,
) -> XrResult;
pub type PfnCreateActionSpace = unsafe extern "system" fn(
    session: XrSession,
    create_info: *const XrActionSpaceCreateInfo,
    space: *mut XrSpace,
) -> XrResult;
pub type PfnSyncActions = unsafe extern "system" fn(
    session: XrSession,
    sync_info: *const XrActionsSyncInfo,
) -> XrResult;
pub type PfnGetActionStateBoolean = unsafe extern "system" fn(
    session: XrSession,
    get_info: *const XrActionStateGetInfo,
    state: *mut XrActionStateBoolean,
) -> XrResult;
pub type PfnGetActionStateFloat = unsafe extern "system" fn(
    session: XrSession,
    get_info: *const XrActionStateGetInfo,
    state: *mut XrActionStateFloat,
) -> XrResult;
pub type PfnGetActionStateVector2f = unsafe extern "system" fn(
    session: XrSession,
    get_info: *const XrActionStateGetInfo,
    state: *mut XrActionStateVector2f,
) -> XrResult;
pub type PfnGetActionStatePose = unsafe extern "system" fn(
    session: XrSession,
    get_info: *const XrActionStateGetInfo,
    state: *mut XrActionStatePose,
) -> XrResult;
pub type PfnApplyHapticFeedback = unsafe extern "system" fn(
    session: XrSession,
    haptic_action_info: *const XrHapticActionInfo,
    haptic_feedback: *const XrHapticBaseHeader,
) -> XrResult;
pub type PfnStopHapticFeedback = unsafe extern "system" fn(
    session: XrSession,
    haptic_action_info: *const XrHapticActionInfo,
) -> XrResult;

#[derive(Clone, Copy)]
pub struct OpenXrProcs {
    pub create_session: PfnCreateSession,
    pub destroy_session: PfnDestroySession,
    pub begin_session: PfnBeginSession,
    pub end_session: PfnEndSession,
    pub create_reference_space: PfnCreateReferenceSpace,
    pub destroy_space: PfnDestroySpace,
    pub locate_space: PfnLocateSpace,
    pub poll_event: PfnPollEvent,
    pub string_to_path: PfnStringToPath,
    pub create_action_set: PfnCreateActionSet,
    pub create_action: PfnCreateAction,
    pub suggest_interaction_profile_bindings: PfnSuggestInteractionProfileBindings,
    pub attach_session_action_sets: PfnAttachSessionActionSets,
    pub create_action_space: PfnCreateActionSpace,
    pub sync_actions: PfnSyncActions,
    pub get_action_state_boolean: PfnGetActionStateBoolean,
    pub get_action_state_float: PfnGetActionStateFloat,
    pub get_action_state_vector2f: PfnGetActionStateVector2f,
    pub get_action_state_pose: PfnGetActionStatePose,
    pub apply_haptic_feedback: PfnApplyHapticFeedback,
    pub stop_haptic_feedback: PfnStopHapticFeedback,
    pub locate_views: PfnLocateViews,
    pub create_swapchain: Option<PfnCreateSwapchain>,
    pub destroy_swapchain: Option<PfnDestroySwapchain>,
    pub enumerate_swapchain_images: Option<PfnEnumerateSwapchainImages>,
    pub acquire_swapchain_image: Option<PfnAcquireSwapchainImage>,
    pub wait_swapchain_image: Option<PfnWaitSwapchainImage>,
    pub release_swapchain_image: Option<PfnReleaseSwapchainImage>,
    pub wait_frame: Option<PfnWaitFrame>,
    pub begin_frame: Option<PfnBeginFrame>,
    pub end_frame: Option<PfnEndFrame>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct XrActionsState {
    pub action_set: XrActionSet,
    pub aim_pose_action: XrAction,
    pub grip_pose_action: XrAction,
    pub trigger_action: XrAction,
    pub squeeze_action: XrAction,
    pub thumbstick_action: XrAction,
    /// Thumbstick click (`BooleanInput`).
    pub thumbstick_click_action: XrAction,
    /// Primary face button A/X (`BooleanInput`).
    pub button_primary_action: XrAction,
    /// Secondary face button B/Y (`BooleanInput`).
    pub button_secondary_action: XrAction,
    /// Menu / system button (`BooleanInput`).
    pub button_menu_action: XrAction,
    pub haptic_action: XrAction,
    pub left_aim_space: XrSpace,
    pub right_aim_space: XrSpace,
    pub left_grip_space: XrSpace,
    pub right_grip_space: XrSpace,
}

// ============================================================================
// High-Level VR Tracking & Input Types
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderEye {
    Left = 0,
    Right = 1,
    Center = 2,
}

/// 6DoF state of a single VR motion controller (Main Hand = 0, Offhand = 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerState {
    pub tracking: bool,
    /// Aim position in room space (meters).
    pub aim_position: Vec3,
    /// Aim orientation in room space.
    pub aim_rotation: Quat,
    /// Grip / hand position in room space (meters).
    pub grip_position: Vec3,
    /// Grip / hand orientation in room space.
    pub grip_rotation: Quat,
    /// Analog thumbstick / trackpad `(x, y)` in `-1.0..=1.0`.
    pub thumbstick: Vec2,
    /// Trigger analog value `0.0..=1.0`.
    pub trigger: f32,
    /// Squeeze / grip analog value `0.0..=1.0`.
    pub squeeze: f32,
    /// Primary face button (A / X).
    pub button_primary: bool,
    /// Secondary face button (B / Y).
    pub button_secondary: bool,
    /// Thumbstick click.
    pub thumbstick_click: bool,
    /// Menu button.
    pub button_menu: bool,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            tracking: true,
            aim_position: Vec3::new(0.2, 1.15, -0.35),
            aim_rotation: Quat::IDENTITY,
            grip_position: Vec3::new(0.2, 1.12, -0.30),
            grip_rotation: Quat::IDENTITY,
            thumbstick: Vec2::ZERO,
            trigger: 0.0,
            squeeze: 0.0,
            button_primary: false,
            button_secondary: false,
            thumbstick_click: false,
            button_menu: false,
        }
    }
}

impl ControllerState {
    /// Forward aim vector in room space (OpenXR `-Z` is forward).
    pub fn aim_direction(&self) -> Vec3 {
        (self.aim_rotation * Vec3::NEG_Z).normalize_or_zero()
    }

    /// Hand forward direction in room space.
    pub fn hand_direction(&self) -> Vec3 {
        (self.grip_rotation * Vec3::NEG_Z).normalize_or_zero()
    }

    /// 4x4 rotation matrix for aim pose.
    pub fn aim_matrix(&self) -> Mat4 {
        Mat4::from_quat(self.aim_rotation)
    }

    /// 4x4 rotation matrix for grip/hand pose.
    pub fn hand_matrix(&self) -> Mat4 {
        Mat4::from_quat(self.grip_rotation)
    }
}

/// Complete snapshot of raw OpenXR / VR hardware poses in room space for one frame.
#[derive(Clone, Debug)]
pub struct RawXrFrame {
    /// HMD center position in room space (meters).
    pub hmd_position: Vec3,
    /// HMD orientation in room space.
    pub hmd_rotation: Quat,
    /// Left and right eye positions in room space.
    pub eye_positions: [Vec3; 2],
    /// Left and right eye orientations in room space.
    pub eye_rotations: [Quat; 2],
    /// Left and right eye FOV tangents (`XrFovf`).
    pub eye_fovs: [XrFovf; 2],
    /// Main controller (`0`) and Offhand controller (`1`).
    pub controllers: [ControllerState; 2],
    /// Optional Full-Body Tracking (FBT) tracker poses in room space:
    /// `[waist, left_foot, right_foot, left_knee, right_knee, left_elbow, right_elbow]`.
    pub fbt_trackers: Option<[(Vec3, Quat); 7]>,
}

impl Default for RawXrFrame {
    fn default() -> Self {
        let hmd_pos = Vec3::new(0.0, 1.62, 0.0);
        let half_ipd = 0.032;
        Self {
            hmd_position: hmd_pos,
            hmd_rotation: Quat::IDENTITY,
            eye_positions: [
                hmd_pos + Vec3::new(-half_ipd, 0.0, 0.0),
                hmd_pos + Vec3::new(half_ipd, 0.0, 0.0),
            ],
            eye_rotations: [Quat::IDENTITY, Quat::IDENTITY],
            eye_fovs: [XrFovf::default(), XrFovf::default()],
            controllers: [
                ControllerState {
                    tracking: true,
                    aim_position: Vec3::new(0.22, 1.18, -0.38),
                    aim_rotation: Quat::IDENTITY,
                    grip_position: Vec3::new(0.22, 1.15, -0.33),
                    grip_rotation: Quat::IDENTITY,
                    ..ControllerState::default()
                },
                ControllerState {
                    tracking: true,
                    aim_position: Vec3::new(-0.22, 1.18, -0.38),
                    aim_rotation: Quat::IDENTITY,
                    grip_position: Vec3::new(-0.22, 1.15, -0.33),
                    grip_rotation: Quat::IDENTITY,
                    ..ControllerState::default()
                },
            ],
            fbt_trackers: None,
        }
    }
}

impl RawXrFrame {
    pub fn hmd_forward(&self) -> Vec3 {
        (self.hmd_rotation * Vec3::NEG_Z).normalize_or_zero()
    }

    pub fn hmd_matrix(&self) -> Mat4 {
        Mat4::from_quat(self.hmd_rotation)
    }

    pub fn eye_position(&self, eye: RenderEye) -> Vec3 {
        match eye {
            RenderEye::Left => self.eye_positions[0],
            RenderEye::Right => self.eye_positions[1],
            RenderEye::Center => self.hmd_position,
        }
    }

    pub fn eye_rotation(&self, eye: RenderEye) -> Quat {
        match eye {
            RenderEye::Left => self.eye_rotations[0],
            RenderEye::Right => self.eye_rotations[1],
            RenderEye::Center => self.hmd_rotation,
        }
    }
}

/// Scheduled haptic pulse on a controller (`HapticScheduler.java`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HapticPulse {
    pub controller: usize,
    pub duration_micros: u32,
    pub frequency_hz: f32,
    pub amplitude: f32,
}

// ============================================================================
// Dynamic OpenXR Runtime Handle + Fallback Simulated Provider
// ============================================================================

pub struct XrContext {
    _library: Option<Library>,
    pub instance: XrInstance,
    destroy_instance: Option<PfnDestroyInstance>,
    /// The loader's `xrGetInstanceProcAddr`, kept for optional runtime functions.
    get_proc_addr: Option<PfnGetInstanceProcAddr>,
    /// Yaw (radians) subtracted from HMD orientation after `recenter`.
    recenter_yaw_offset: f32,
    /// Whether `xrBeginFrame` ran this game frame and `xrEndFrame` is owed
    /// from the render path (`submit_openxr_frame`).
    pub frame_pending: bool,
    /// The last `xrWaitFrame` state (predicted display time for submission).
    pub frame_state: Option<XrFrameState>,
    /// Each eye swapchain's image rect size (width, height).
    pub swapchain_size: Option<(u32, u32)>,
    /// The swapchain image index each eye acquired this frame (None: not acquired).
    pub acquired_image_index: [Option<u32>; 2],
    /// Which eye this game frame renders (0 = left, 1 = right). The render
    /// hook's second pass switches it so the whole engine renders the right
    /// eye's view before `world_drawn` copies it into the right eye texture.
    pub stereo_eye_select: u8,
    /// Whether the right eye image was copied this frame (submission gate).
    pub stereo_right_done: bool,
    #[allow(dead_code)]
    pub system_id: XrSystemId,
    system_name: String,
    hardware_connected: bool,
    /// Whether `xrLocateViews` produced valid per-eye FOVs this frame
    /// (gates `hardware_eye_fov`).
    hardware_eye_fovs_valid: bool,
    pub session: XrSession,
    pub session_state: XrSessionState,
    pub stage_space: XrSpace,
    pub view_space: XrSpace,
    pub actions: XrActionsState,
    pub hand_paths: [XrPath; 2],
    pub procs: Option<OpenXrProcs>,
    // Swapchain management for VR stereo rendering
    pub swapchains: [XrSwapchain; 2],
    /// Raw `XrSwapchainImageVulkan2KHR` records per eye (count-capacity stride,
    /// 64 bytes each): the base header plus the `VkImage` handle at offset 16.
    pub swapchain_images: [Vec<u8>; 2],
    pub swapchain_format: i64,
    pub osc_receiver: crate::osc::OscTrackerReceiver,
    pub current_frame: RawXrFrame,
    pub pending_haptics: Vec<HapticPulse>,
}

impl Drop for XrContext {
    fn drop(&mut self) {
        if let Some(procs) = self.procs {
            if self.actions.left_aim_space != XR_NULL_HANDLE {
                unsafe { let _ = (procs.destroy_space)(self.actions.left_aim_space); }
                self.actions.left_aim_space = XR_NULL_HANDLE;
            }
            if self.actions.right_aim_space != XR_NULL_HANDLE {
                unsafe { let _ = (procs.destroy_space)(self.actions.right_aim_space); }
                self.actions.right_aim_space = XR_NULL_HANDLE;
            }
            if self.actions.left_grip_space != XR_NULL_HANDLE {
                unsafe { let _ = (procs.destroy_space)(self.actions.left_grip_space); }
                self.actions.left_grip_space = XR_NULL_HANDLE;
            }
            if self.actions.right_grip_space != XR_NULL_HANDLE {
                unsafe { let _ = (procs.destroy_space)(self.actions.right_grip_space); }
                self.actions.right_grip_space = XR_NULL_HANDLE;
            }
            if self.view_space != XR_NULL_HANDLE {
                unsafe { let _ = (procs.destroy_space)(self.view_space); }
                self.view_space = XR_NULL_HANDLE;
            }
            if self.stage_space != XR_NULL_HANDLE {
                // SAFETY: Destroying space with valid pointer
                unsafe { let _ = (procs.destroy_space)(self.stage_space); }
                self.stage_space = XR_NULL_HANDLE;
            }
            if self.session != XR_NULL_HANDLE {
                // SAFETY: Destroying session with valid pointer
                unsafe { let _ = (procs.destroy_session)(self.session); }
                self.session = XR_NULL_HANDLE;
            }
        }
        if self.instance != XR_NULL_HANDLE {
            if let Some(destroy) = self.destroy_instance {
                // SAFETY: `self.instance` is a valid OpenXR instance created by `xrCreateInstance`.
                unsafe {
                    let _ = destroy(self.instance);
                }
            }
            self.instance = XR_NULL_HANDLE;
        }
    }
}

/// Resolves a NUL-terminated path string against a live instance through
/// `xrStringToPath` (None on failure or an unknown path).
fn resolve_path(procs: &OpenXrProcs, instance: XrInstance, full: &str) -> Option<XrPath> {
    let mut out: XrPath = XR_NULL_PATH;
    // SAFETY: `full` ends in NUL (built by the callers) and `instance` is live.
    let res = unsafe { (procs.string_to_path)(instance, full.as_ptr() as *const c_char, &mut out) };
    (res >= XR_SUCCESS && out != XR_NULL_PATH).then_some(out)
}

impl Default for XrContext {
    fn default() -> Self {
        Self::new()
    }
}

impl XrContext {
    /// Initializes OpenXR dynamically if a loader and HMD system exist, or falls back to
    /// `SimulatedXrProvider` (`NullVR`).
    pub fn new() -> Self {
        let osc_receiver = crate::osc::OscTrackerReceiver::default();
        match Self::try_init_openxr() {
            Ok((lib, instance, destroy_instance, get_proc_addr, system_id, system_name, procs, session, stage_space, view_space, actions, hand_paths)) => {
                log::info!(
                    "[ViveCraft] OpenXR runtime initialized: system '{}' (id={})",
                    system_name,
                    system_id
                );
                Self {
                    _library: Some(lib),
                    instance,
                    destroy_instance: Some(destroy_instance),
                    get_proc_addr,
                    frame_pending: false,
                    frame_state: None,
                    swapchain_size: None,
                    acquired_image_index: [None, None],
                    stereo_eye_select: 0,
                    stereo_right_done: false,
                    system_id,
                    system_name,
                    hardware_connected: false,
                    hardware_eye_fovs_valid: false,
                    session,
                    session_state: XrSessionState::Idle,
                    stage_space,
                    view_space,
                    actions,
                    hand_paths,
                    procs: Some(procs),
                    swapchains: [XR_NULL_HANDLE, XR_NULL_HANDLE],
                    swapchain_images: [Vec::new(), Vec::new()],
                    swapchain_format: 0,
                    recenter_yaw_offset: 0.0,
                    osc_receiver,
                    current_frame: RawXrFrame::default(),
                    pending_haptics: Vec::new(),
                }
            }
            Err(reason) => {
                log::info!(
                    "[ViveCraft] OpenXR hardware not active ({}); using NullVR / Simulated 6DoF Stereo Provider",
                    reason
                );
                Self {
                    _library: None,
                    instance: XR_NULL_HANDLE,
                    destroy_instance: None,
                    get_proc_addr: None,
                    frame_pending: false,
                    frame_state: None,
                    swapchain_size: None,
                    acquired_image_index: [None, None],
                    stereo_eye_select: 0,
                    stereo_right_done: false,
                    system_id: XR_NULL_SYSTEM_ID,
                    system_name: "ViveCraft Simulated 6DoF (NullVR)".to_owned(),
                    hardware_connected: false,
                    hardware_eye_fovs_valid: false,
                    session: XR_NULL_HANDLE,
                    session_state: XrSessionState::Idle,
                    stage_space: XR_NULL_HANDLE,
                    view_space: XR_NULL_HANDLE,
                    actions: XrActionsState::default(),
                    hand_paths: [XR_NULL_PATH, XR_NULL_PATH],
                    procs: None,
                    swapchains: [XR_NULL_HANDLE, XR_NULL_HANDLE],
                    swapchain_images: [Vec::new(), Vec::new()],
                    swapchain_format: 0,
                    recenter_yaw_offset: 0.0,
                    osc_receiver,
                    current_frame: RawXrFrame::default(),
                    pending_haptics: Vec::new(),
                }
            }
        }
    }

    /// The loader's candidate paths, known SteamVR installs first: Steam's own
    /// folder carries the OpenXR loader a SteamVR install needs, and nothing
    /// else puts one on a clean machine's PATH — this is what saves a vanilla
    /// Windows + SteamVR setup from the quiet Simulated fallback. Then the
    /// bundle's `native-data/vivecraft/` beside the game's executable (the
    /// post-build step's output, `horizon_client::native_data::built`), and
    /// last the plain PATH names, for a loader installed system-wide.
    fn loader_candidates() -> Vec<std::path::PathBuf> {
        let names: &[&'static str] = Self::loader_names();
        let mut candidates = Vec::new();
        candidates.extend(Self::steamvr_loader_candidates(names));
        // `native-data/<mod id>/` beside the game's executable, absolute: the
        // post-build step's output. `None` only when the game's own path is
        // unknown, in which case the PATH names still apply.
        if let Some(dir) = Self::native_built_data_dir() {
            for name in names {
                candidates.push(dir.join(name));
            }
        }
        candidates.extend(names.iter().map(|n| std::path::PathBuf::from(*n)));
        candidates
    }

    /// Steam's install folders from the registry, then the loader every
    /// SteamVR install carries under `<steam>\steamapps\common\SteamVR\bin\x64\`.
    /// Windows-only: empty everywhere else.
    fn steamvr_loader_candidates(names: &[&'static str]) -> Vec<std::path::PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let mut candidates = Vec::new();
            let mut roots: Vec<std::path::PathBuf> = Vec::new();
            // Per-user Steam (what Steam itself installs with) and the 32-bit
            // machine-wide key under WOW64. Either or both may be absent.
            for (root, key, value) in [
                (windows_registry::CURRENT_USER, "Software\\Valve\\Steam", "SteamPath"),
                (
                    windows_registry::LOCAL_MACHINE,
                    "SOFTWARE\\WOW6432Node\\Valve\\Steam",
                    "InstallPath",
                ),
            ] {
                if let Ok(steam) = (|| {
                    root.open(key)
                        .and_then(|k| k.get_string(value))
                        .map(std::path::PathBuf::from)
                })() {
                    if !roots.contains(&steam) {
                        roots.push(steam);
                    }
                }
            }
            // The common fallback install location, for a Steam the registry
            // doesn't name (portable, or a stale key).
            roots.push(std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam",
            ));
            for root in roots {
                // SteamVR's real layout: `bin\win64\` (checked on live
                // installs; `bin\x64` does not exist there). Keep `x64` as a
                // fallback for older or custom layouts.
                for sub in ["win64", "x64"] {
                    for name in names {
                        candidates.push(
                            root.join("steamapps")
                                .join("common")
                                .join("SteamVR")
                                .join("bin")
                                .join(sub)
                                .join(name),
                        );
                    }
                }
            }
            candidates
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = names;
            Vec::new()
        }
    }

    fn loader_names() -> &'static [&'static str] {
        #[cfg(target_os = "windows")]
        {
            &["openxr_loader.dll"]
        }
        #[cfg(target_os = "macos")]
        {
            &["libopenxr_loader.dylib"]
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            &["libopenxr_loader.so.1", "libopenxr_loader.so"]
        }
    }

    /// The folder the bundle's post-build step filled: `native-data/vivecraft/`
    /// beside the game's executable, absolute. Mirrors
    /// `horizon_client::native_data::built` (the engine crate can't be reached
    /// from here): `None` only when the game's own path is unknown.
    fn native_built_data_dir() -> Option<std::path::PathBuf> {
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.join("native-data").join("vivecraft"))
    }

    fn write_fixed_cstr<const N: usize>(target: &mut [c_char; N], text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(N.saturating_sub(1));
        for (i, &b) in bytes[..len].iter().enumerate() {
            target[i] = b as c_char;
        }
        target[len] = 0;
    }

    /// Reads a fixed C string field (NUL-terminated) as a `&str` lossily.
    fn fixed_cstr_to_str<const N: usize>(source: &[c_char; N]) -> &str {
        let len = source.iter().position(|&c| c == 0).unwrap_or(N);
        std::str::from_utf8(
            // SAFETY: `c_char` is a one-byte integer type on every supported
            // platform, and the first NUL terminates the string.
            unsafe { std::slice::from_raw_parts(source.as_ptr() as *const u8, len) },
        )
        .unwrap_or("")
    }

    fn try_init_openxr() -> Result<
        (
            Library,
            XrInstance,
            PfnDestroyInstance,
            Option<PfnGetInstanceProcAddr>,
            XrSystemId,
            String,
            OpenXrProcs,
            XrSession,
            XrSpace,
            XrSpace,
            XrActionsState,
            [XrPath; 2],
        ),
        String,
    > {
        let mut last_err = String::from("no OpenXR loader library found");
        let mut loaded_lib = None;
        let mut loader_path = String::new();
        for candidate in Self::loader_candidates() {
            // SAFETY: Loading the standard Khronos OpenXR loader shared library.
            match unsafe { Library::new(&candidate) } {
                Ok(lib) => {
                    loader_path = candidate.display().to_string();
                    loaded_lib = Some(lib);
                    break;
                }
                Err(e) => {
                    last_err = format!("{}: {e}", candidate.display());
                }
            }
        }
        // The path matters: a quiet PATH fallback (no bundled loader) is the usual
        // reason a clean Windows machine lands in Simulated mode. On failure the
        // last candidate error names what was tried, instead of an empty string
        // that reads in the log like a loader with no path.
        if loaded_lib.is_none() {
            log::warn!("[ViveCraft] OpenXR loader not found; tried: {}", last_err);
        } else {
            log::warn!("[ViveCraft] loader path: {loader_path}");
        }
        let lib = loaded_lib.ok_or(last_err)?;

        // SAFETY: `xrGetInstanceProcAddr` is the canonical entry point of OpenXR 1.0.
        let get_proc: PfnGetInstanceProcAddr = unsafe {
            *lib.get::<PfnGetInstanceProcAddr>(b"xrGetInstanceProcAddr\0")
                .map_err(|e| format!("missing xrGetInstanceProcAddr: {e}"))?
        };
        // The loader-level entry points are taken as DIRECT DLL exports, not
        // through `xrGetInstanceProcAddr` with a null instance: SteamVR's
        // loader 1.1.62 hands out function pointers that hard-crash the
        // process on call (verified by an isolated reproduction — the same
        // calls through direct exports succeed). The instance-level ones
        // below are fine via `get_proc` with a real instance.
        let direct = |name: &[u8]| -> Option<PfnVoidFunction> {
            unsafe { lib.get::<PfnVoidFunction>(name).ok().map(|sym| *sym) }
        };
        let create_instance: PfnCreateInstance = match direct(b"xrCreateInstance\0") {
            Some(f) => unsafe { std::mem::transmute(f) },
            None => {
                // Fall back to the spec path for loaders that do not export
                // it directly (the crash pattern was SteamVR 1.1.62 only).
                let mut raw: PfnVoidFunction = None;
                let res = unsafe {
                    get_proc(XR_NULL_HANDLE, c"xrCreateInstance".as_ptr(), &mut raw)
                };
                if res < XR_SUCCESS || raw.is_none() {
                    return Err(format!(
                        "xrCreateInstance not resolvable (direct export missing, GetProcAddr={res})"
                    ));
                }
                unsafe { std::mem::transmute(raw.unwrap()) }
            }
        };

        let mut app_info = XrApplicationInfo {
            application_name: [0; XR_MAX_APPLICATION_NAME_SIZE],
            application_version: 1,
            engine_name: [0; XR_MAX_ENGINE_NAME_SIZE],
            engine_version: 1,
            api_version: XR_CURRENT_API_VERSION,
        };
        Self::write_fixed_cstr(&mut app_info.application_name, "ViveCraft-Horizon");
        Self::write_fixed_cstr(&mut app_info.engine_name, "Horizon-Rust");

        // Query the runtime's instance extensions and enable Vulkan device
        // creation (`XR_KHR_vulkan_enable2`): SteamVR, Oculus/Meta, Monado and
        // all conformant desktop runtimes support it; without it no graphics
        // binding is possible and `xrCreateSwapchain` fails.
        // Loader-level function: direct export first (same SteamVR 1.1.62
        // GetProcAddr crash pattern), spec path as fallback.
        let enumerate_ext_raw: Option<PfnVoidFunction> =
            direct(b"xrEnumerateInstanceExtensionProperties\0").or_else(|| {
                let mut raw: PfnVoidFunction = None;
                let _ = unsafe {
                    get_proc(
                        XR_NULL_HANDLE,
                        c"xrEnumerateInstanceExtensionProperties".as_ptr(),
                        &mut raw,
                    )
                };
                Some(raw)
            });
        let mut ext_names: Vec<&'static [u8]> = Vec::new();
        if let Some(enumerate_ext) = enumerate_ext_raw {
            let enumerate_ext: PfnEnumerateInstanceExtensionProperties =
                unsafe { std::mem::transmute(enumerate_ext) };
            let mut count: u32 = 0;
            // SAFETY: capacity query with null layer and null properties.
            let res = unsafe { enumerate_ext(ptr::null(), 0, &mut count, ptr::null_mut()) };
            if res >= XR_SUCCESS && count > 0 {
                let mut ext_props: Vec<XrInstanceExtensionProperties> = (0..count)
                    .map(|_| XrInstanceExtensionProperties {
                        ty: XrStructureType::InstanceExtensionProperties,
                        next: ptr::null(),
                        extension_name: [0 as c_char; 128],
                        extension_version: 0,
                    })
                    .collect();
                // SAFETY: `ext_props` holds `count` valid output structs.
                let res = unsafe {
                    enumerate_ext(
                        ptr::null(),
                        count,
                        &mut count,
                        ext_props.as_mut_ptr(),
                    )
                };
                if res >= XR_SUCCESS {
                    let wants_vulkan2 = ext_props.iter().any(|p| {
                        let name = Self::fixed_cstr_to_str(&p.extension_name);
                        name == "XR_KHR_vulkan_enable2"
                    });
                    if wants_vulkan2 {
                        ext_names.push(b"XR_KHR_vulkan_enable2\0");
                    }
                    for p in &ext_props {
                        let name = Self::fixed_cstr_to_str(&p.extension_name);
                        if name == "XR_KHR_vulkan_enable" {
                            // Fallback when the runtime lacks enable2: still
                            // enables Vulkan device creation (non-2 binding).
                            ext_names.push(b"XR_KHR_vulkan_enable\0");
                        }
                    }
                }
            }
        }
        // `&'static [u8]` literals above end in NUL; take their `as_ptr()` as
        // the C strings the create-info wants.
        let ext_name_ptrs: Vec<*const c_char> =
            ext_names.iter().map(|n| n.as_ptr() as *const c_char).collect();

        let create_info = XrInstanceCreateInfo {
            ty: XrStructureType::InstanceCreateInfo,
            next: ptr::null(),
            create_flags: 0,
            application_info: app_info,
            enabled_api_layer_count: 0,
            enabled_api_layer_names: ptr::null(),
            enabled_extension_count: ext_name_ptrs.len() as u32,
            enabled_extension_names: if ext_name_ptrs.is_empty() {
                ptr::null()
            } else {
                ext_name_ptrs.as_ptr()
            },
        };

        let mut instance: XrInstance = XR_NULL_HANDLE;
        // SAFETY: Passing valid `XrInstanceCreateInfo`.
        let res = unsafe { create_instance(&create_info, &mut instance) };
        if res < XR_SUCCESS || instance == XR_NULL_HANDLE {
            return Err(format!("xrCreateInstance returned {res}"));
        }

        let mut destroy_raw: PfnVoidFunction = None;
        let _ = unsafe {
            get_proc(
                instance,
                c"xrDestroyInstance".as_ptr(),
                &mut destroy_raw,
            )
        };
        let destroy_instance: PfnDestroyInstance = match destroy_raw {
            Some(f) => unsafe { std::mem::transmute(f) },
            None => return Err("missing xrDestroyInstance".into()),
        };

        let mut get_system_raw: PfnVoidFunction = None;
        let _ = unsafe { get_proc(instance, c"xrGetSystem".as_ptr(), &mut get_system_raw) };
        let Some(get_system_fn) = get_system_raw else {
            unsafe {
                let _ = destroy_instance(instance);
            }
            return Err("missing xrGetSystem".into());
        };
        let get_system: PfnGetSystem = match unsafe {
            direct_export::<PfnGetSystem>(&lib, c"xrGetSystem")
        } {
            Some(f) => f,
            None => unsafe { std::mem::transmute(get_system_fn) },
        };

        let sys_info = XrSystemGetInfo {
            ty: XrStructureType::SystemGetInfo,
            next: ptr::null(),
            form_factor: XrFormFactor::HeadMountedDisplay,
        };
        let mut system_id: XrSystemId = XR_NULL_SYSTEM_ID;
        // The SteamVR runtime auto-starts vrserver when the instance is
        // created; until it is up, xrGetSystem returns
        // XR_ERROR_FORM_FACTOR_UNAVAILABLE, which the spec allows to succeed
        // later. Poll for up to 5 s (SteamVR typically takes 2-3 s) instead
        // of dropping to Simulated forever — the instance is already alive,
        // so this is a cheap blocking wait done once at startup. A machine
        // with no HMD at all fails on the first call and falls through
        // immediately.
        let mut res = i32::MIN;
        for attempt in 0..50 {
            res = unsafe { get_system(instance, &sys_info, &mut system_id) };
            if res >= XR_SUCCESS && system_id != XR_NULL_SYSTEM_ID {
                break;
            }
            if attempt == 0 {
                log::warn!(
                    "[ViveCraft] xrGetSystem={res}: HMD not ready yet (SteamVR starting?); polling up to 5 s"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if res < XR_SUCCESS || system_id == XR_NULL_SYSTEM_ID {
            unsafe {
                let _ = destroy_instance(instance);
            }
            return Err(format!("no HMD present (xrGetSystem={res} after retries)"));
        }

        let mut system_name = String::from("OpenXR HMD");
        let mut get_props_raw: PfnVoidFunction = None;
        let _ = unsafe {
            get_proc(
                instance,
                c"xrGetSystemProperties".as_ptr(),
                &mut get_props_raw,
            )
        };
        if let Some(get_props_fn) = get_props_raw {
            let get_props: PfnGetSystemProperties = unsafe { std::mem::transmute(get_props_fn) };
            let mut props = XrSystemProperties {
                ty: XrStructureType::SystemProperties,
                next: ptr::null_mut(),
                system_id: 0,
                vendor_id: 0,
                system_name: [0; XR_MAX_SYSTEM_NAME_SIZE],
                graphics_properties: XrSystemGraphicsProperties::default(),
                tracking_properties: XrSystemTrackingProperties::default(),
            };
            if unsafe { get_props(instance, system_id, &mut props) } >= XR_SUCCESS {
                let cstr = unsafe { CStr::from_ptr(props.system_name.as_ptr()) };
                if let Ok(s) = cstr.to_str() {
                    if !s.is_empty() {
                        system_name = s.to_owned();
                    }
                }
            }
        }

        // SteamVR's loader 1.1.62 hands out call-crashing pointers through
        // `xrGetInstanceProcAddr` (verified by isolated reproduction), so
        // every function is taken as a DIRECT DLL export first — the Khronos
        // loader exports all core 1.0 functions — and only falls back to the
        // spec path (valid instance) for loaders without direct exports.
        unsafe fn direct_export<T: Copy>(lib: &Library, name: &CStr) -> Option<T> {
            let mut buf = name.to_bytes().to_vec();
            buf.push(0);
            unsafe { lib.get::<T>(buf.as_slice()).ok().map(|sym| *sym) }
        }

        unsafe fn load_pfn<T: Copy>(lib: &Library, get_proc: PfnGetInstanceProcAddr, instance: XrInstance, name: &CStr) -> Result<T, String> {
            if let Some(f) = unsafe { direct_export::<T>(lib, name) } {
                return Ok(f);
            }
            let mut raw: PfnVoidFunction = None;
            let res = unsafe { get_proc(instance, name.as_ptr(), &mut raw) };
            if res < XR_SUCCESS || raw.is_none() {
                return Err(format!("failed to load {}: res={res}", name.to_string_lossy()));
            }
            Ok(unsafe { std::mem::transmute_copy(&raw.unwrap()) })
        }

        unsafe fn opt_pfn<T: Copy>(lib: &Library, get_proc: PfnGetInstanceProcAddr, instance: XrInstance, name: &CStr) -> Option<T> {
            if let Some(f) = unsafe { direct_export::<T>(lib, name) } {
                return Some(f);
            }
            let mut raw: PfnVoidFunction = None;
            let res = unsafe { get_proc(instance, name.as_ptr(), &mut raw) };
            if res >= XR_SUCCESS && raw.is_some() {
                Some(unsafe { std::mem::transmute_copy(&raw.unwrap()) })
            } else {
                None
            }
        }

        let procs = unsafe {
            OpenXrProcs {
                create_session: load_pfn(&lib, get_proc, instance, c"xrCreateSession")?,
                destroy_session: load_pfn(&lib, get_proc, instance, c"xrDestroySession")?,
                begin_session: load_pfn(&lib, get_proc, instance, c"xrBeginSession")?,
                end_session: load_pfn(&lib, get_proc, instance, c"xrEndSession")?,
                create_reference_space: load_pfn(&lib, get_proc, instance, c"xrCreateReferenceSpace")?,
                destroy_space: load_pfn(&lib, get_proc, instance, c"xrDestroySpace")?,
                locate_space: load_pfn(&lib, get_proc, instance, c"xrLocateSpace")?,
                poll_event: load_pfn(&lib, get_proc, instance, c"xrPollEvent")?,
                string_to_path: load_pfn(&lib, get_proc, instance, c"xrStringToPath")?,
                create_action_set: load_pfn(&lib, get_proc, instance, c"xrCreateActionSet")?,
                create_action: load_pfn(&lib, get_proc, instance, c"xrCreateAction")?,
                suggest_interaction_profile_bindings: load_pfn(&lib, get_proc, instance, c"xrSuggestInteractionProfileBindings")?,
                attach_session_action_sets: load_pfn(&lib, get_proc, instance, c"xrAttachSessionActionSets")?,
                create_action_space: load_pfn(&lib, get_proc, instance, c"xrCreateActionSpace")?,
                sync_actions: load_pfn(&lib, get_proc, instance, c"xrSyncActions")?,
                get_action_state_boolean: load_pfn(&lib, get_proc, instance, c"xrGetActionStateBoolean")?,
                get_action_state_float: load_pfn(&lib, get_proc, instance, c"xrGetActionStateFloat")?,
                get_action_state_vector2f: load_pfn(&lib, get_proc, instance, c"xrGetActionStateVector2f")?,
                get_action_state_pose: load_pfn(&lib, get_proc, instance, c"xrGetActionStatePose")?,
                apply_haptic_feedback: load_pfn(&lib, get_proc, instance, c"xrApplyHapticFeedback")?,
                stop_haptic_feedback: load_pfn(&lib, get_proc, instance, c"xrStopHapticFeedback")?,
                locate_views: load_pfn(&lib, get_proc, instance, c"xrLocateViews")?,
                create_swapchain: opt_pfn(&lib, get_proc, instance, c"xrCreateSwapchain"),
                destroy_swapchain: opt_pfn(&lib, get_proc, instance, c"xrDestroySwapchain"),
                enumerate_swapchain_images: opt_pfn(&lib, get_proc, instance, c"xrEnumerateSwapchainImages"),
                acquire_swapchain_image: opt_pfn(&lib, get_proc, instance, c"xrAcquireSwapchainImage"),
                wait_swapchain_image: opt_pfn(&lib, get_proc, instance, c"xrWaitSwapchainImage"),
                release_swapchain_image: opt_pfn(&lib, get_proc, instance, c"xrReleaseSwapchainImage"),
                wait_frame: opt_pfn(&lib, get_proc, instance, c"xrWaitFrame"),
                begin_frame: opt_pfn(&lib, get_proc, instance, c"xrBeginFrame"),
                end_frame: opt_pfn(&lib, get_proc, instance, c"xrEndFrame"),
            }
        };

        // The session itself is NOT created here: a graphics binding with the
        // game's real Vulkan instance/device/queue (wgpu) must be passed in
        // `XrSessionCreateInfo::next`, and those only exist once the renderer
        // is up. `create_session_with_vulkan` (called from the render hook)
        // creates it — until then the runtime stays in Simulated mode.
        let session = XR_NULL_HANDLE;
        let stage_space = XR_NULL_HANDLE;
        let view_space = XR_NULL_HANDLE;
        let actions = XrActionsState::default();
        let hand_paths = [XR_NULL_PATH, XR_NULL_PATH];
        Ok((lib, instance, destroy_instance, Some(get_proc), system_id, system_name, procs, session, stage_space, view_space, actions, hand_paths))
    }

    /// Creates the OpenXR session with a Vulkan graphics binding built from
    /// the game's own wgpu Vulkan instance/device/queue — what SteamVR and
    /// every conformant desktop runtime require before `xrCreateSwapchain`
    /// can hand out real images. Idempotent: a no-op once a session exists.
    ///
    /// `vulkan2` selects the `XrGraphicsBindingVulkan2KHR` form (the instance
    /// was created with `XR_KHR_vulkan_enable2`); when false the runtime only
    /// got `XR_KHR_vulkan_enable` and the legacy `XrGraphicsBindingVulkanKHR`
    /// would be needed — this loader reports and refuses that case rather
    /// than render wrong, since every current desktop runtime has enable2.
    pub fn create_session_with_vulkan(&mut self, vk_instance: u64, vk_physical_device: u64, vk_device: u64, queue_family_index: u32, queue_index: u32) -> bool {
        if self.session != XR_NULL_HANDLE || self.instance == XR_NULL_HANDLE {
            return self.session != XR_NULL_HANDLE;
        }
        let Some(procs) = self.procs else { return false; };
        if self.system_id == XR_NULL_SYSTEM_ID {
            return false;
        }

        let binding = XrGraphicsBindingVulkan2KHR {
            ty: XrStructureType::GraphicsBindingVulkan2KHR,
            next: ptr::null(),
            instance: vk_instance,
            physical_device: vk_physical_device,
            device: vk_device,
            queue_family_index,
            queue_index,
        };
        let session_create_info = XrSessionCreateInfo {
            ty: XrStructureType::SessionCreateInfo,
            next: &binding as *const XrGraphicsBindingVulkan2KHR as *const c_void,
            create_flags: 0,
            system_id: self.system_id,
        };
        let mut session = XR_NULL_HANDLE;
        // SAFETY: `binding` holds the live Vulkan handles wgpu created and
        // keeps alive for the game's whole run; it outlives this call.
        let res = unsafe {
            (procs.create_session)(self.instance, &session_create_info, &mut session)
        };
        if res < XR_SUCCESS || session == XR_NULL_HANDLE {
            log::warn!("[ViveCraft] xrCreateSession with Vulkan binding failed ({res}); staying in Simulated mode");
            return false;
        }
        log::warn!("[ViveCraft] OpenXR session created with Vulkan graphics binding");
        self.session = session;

        // Reference spaces, hand paths and the gameplay action set.
        let stage_info = XrReferenceSpaceCreateInfo {
            ty: XrStructureType::ReferenceSpaceCreateInfo,
            next: ptr::null(),
            reference_space_type: XrReferenceSpaceType::Stage,
            pose_in_reference_space: XrPosef::IDENTITY,
        };
        // SAFETY: valid create-info against the live session.
        let res = unsafe { (procs.create_reference_space)(session, &stage_info, &mut self.stage_space) };
        if res < XR_SUCCESS || self.stage_space == XR_NULL_HANDLE {
            let local_info = XrReferenceSpaceCreateInfo {
                ty: XrStructureType::ReferenceSpaceCreateInfo,
                next: ptr::null(),
                reference_space_type: XrReferenceSpaceType::Local,
                pose_in_reference_space: XrPosef::IDENTITY,
            };
            unsafe {
                let _ = (procs.create_reference_space)(session, &local_info, &mut self.stage_space);
            }
        }

        let view_info = XrReferenceSpaceCreateInfo {
            ty: XrStructureType::ReferenceSpaceCreateInfo,
            next: ptr::null(),
            reference_space_type: XrReferenceSpaceType::View,
            pose_in_reference_space: XrPosef::IDENTITY,
        };
        unsafe {
            let _ = (procs.create_reference_space)(session, &view_info, &mut self.view_space);
        }

        unsafe {
            let _ = (procs.string_to_path)(self.instance, c"/user/hand/right".as_ptr(), &mut self.hand_paths[0]);
            let _ = (procs.string_to_path)(self.instance, c"/user/hand/left".as_ptr(), &mut self.hand_paths[1]);
        }

        let mut set_info = XrActionSetCreateInfo {
            ty: XrStructureType::ActionSetCreateInfo,
            next: ptr::null(),
            action_set_name: [0; 64],
            localized_action_set_name: [0; 128],
            priority: 0,
        };
        Self::write_fixed_cstr(&mut set_info.action_set_name, "gameplay");
        Self::write_fixed_cstr(&mut set_info.localized_action_set_name, "Gameplay Actions");
        // SAFETY: valid create-info on the live instance.
        let set_res = unsafe { (procs.create_action_set)(self.instance, &set_info, &mut self.actions.action_set) };
        if set_res >= XR_SUCCESS && self.actions.action_set != XR_NULL_HANDLE {
            let hand_paths = self.hand_paths;
            let create_act = |name: &str, loc: &str, ty: XrActionType| -> XrAction {
                let mut act_info = XrActionCreateInfo {
                    ty: XrStructureType::ActionCreateInfo,
                    next: ptr::null(),
                    action_name: [0; 64],
                    action_type: ty,
                    count_subaction_paths: 2,
                    subaction_paths: hand_paths.as_ptr(),
                    localized_action_name: [0; 128],
                };
                Self::write_fixed_cstr(&mut act_info.action_name, name);
                Self::write_fixed_cstr(&mut act_info.localized_action_name, loc);
                let mut act = XR_NULL_HANDLE;
                unsafe {
                    let _ = (procs.create_action)(self.actions.action_set, &act_info, &mut act);
                }
                act
            };

            self.actions.aim_pose_action = create_act("aim_pose", "Aim Pose", XrActionType::PoseInput);
            self.actions.grip_pose_action = create_act("grip_pose", "Grip Pose", XrActionType::PoseInput);
            self.actions.trigger_action = create_act("trigger", "Trigger", XrActionType::FloatInput);
            self.actions.squeeze_action = create_act("squeeze", "Squeeze", XrActionType::FloatInput);
            self.actions.thumbstick_action = create_act("thumbstick", "Thumbstick", XrActionType::Vector2fInput);
            self.actions.thumbstick_click_action = create_act("thumbstick_click", "Thumbstick Click", XrActionType::BooleanInput);
            self.actions.button_primary_action = create_act("button_primary", "Button Primary", XrActionType::BooleanInput);
            self.actions.button_secondary_action = create_act("button_secondary", "Button Secondary", XrActionType::BooleanInput);
            self.actions.button_menu_action = create_act("button_menu", "Button Menu", XrActionType::BooleanInput);
            self.actions.haptic_action = create_act("haptic", "Haptic", XrActionType::VibrationOutput);

            // §2.2: suggest bindings for the interaction profiles BEFORE
            // `xrAttachSessionActionSets` — runtimes ignore bindings suggested
            // after attach. Per-profile the same action may bind different
            // user paths. Failures are logged, not fatal: a runtime without a
            // given controller just never fires those bindings.
            self.suggest_interaction_bindings(&procs);

            let attach_info = XrSessionActionSetsAttachInfo {
                ty: XrStructureType::SessionActionSetsAttachInfo,
                next: ptr::null(),
                count_action_sets: 1,
                action_sets: &self.actions.action_set,
            };
            unsafe {
                let _ = (procs.attach_session_action_sets)(session, &attach_info);
            }

            let session_for_spaces = self.session;
            let create_act_space = |act: XrAction, sub: XrPath| -> XrSpace {
                if act == XR_NULL_HANDLE {
                    return XR_NULL_HANDLE;
                }
                let sp_info = XrActionSpaceCreateInfo {
                    ty: XrStructureType::ActionSpaceCreateInfo,
                    next: ptr::null(),
                    action: act,
                    subaction_path: sub,
                    pose_in_action_space: XrPosef::IDENTITY,
                };
                let mut sp = XR_NULL_HANDLE;
                unsafe {
                    let _ = (procs.create_action_space)(session_for_spaces, &sp_info, &mut sp);
                }
                sp
            };

            let hand_paths = self.hand_paths;
            self.actions.right_aim_space = create_act_space(self.actions.aim_pose_action, hand_paths[0]);
            self.actions.left_aim_space = create_act_space(self.actions.aim_pose_action, hand_paths[1]);
            self.actions.right_grip_space = create_act_space(self.actions.grip_pose_action, hand_paths[0]);
            self.actions.left_grip_space = create_act_space(self.actions.grip_pose_action, hand_paths[1]);
        }
        true
    }

    /// Resolves a runtime path string to `XrPath` (None on failure).
    fn path(&self, procs: &OpenXrProcs, path: &CStr) -> Option<XrPath> {
        let mut out: XrPath = XR_NULL_PATH;
        // SAFETY: valid instance + NUL-terminated path string per spec.
        let res = unsafe { (procs.string_to_path)(self.instance, path.as_ptr(), &mut out) };
        (res >= XR_SUCCESS && out != XR_NULL_PATH).then_some(out)
    }

    /// Suggests the gameplay action bindings for the three controller
    /// interaction profiles (§2.2): Khronos Simple, Valve Index, Oculus Touch.
    /// Must run before `xrAttachSessionActionSets`; per-call failures are
    /// logged — a runtime that lacks a profile rejects only that profile.
    fn suggest_interaction_bindings(&mut self, procs: &OpenXrProcs) {
        const PROFILES: [&CStr; 3] = [
            c"/interaction_profiles/khr/simple_controller",
            c"/interaction_profiles/valve/index_controller",
            c"/interaction_profiles/oculus/touch_controller",
        ];
        // (right hand, left hand)
        const HANDS: [(&CStr, &CStr); 1] = [
            (c"/user/hand/right", c"/user/hand/left"),
        ];
        // Per-profile component suffixes: (action, right-hand suffix,
        // left-hand suffix). The three profiles define these input paths
        // (Simple: poses/trigger/menu; Index & Touch also squeeze/thumbstick/
        // face buttons; face buttons are A/X and B/Y on Touch).
        for profile in PROFILES {
            let Some(profile_path) = self.path(procs, profile) else {
                continue;
            };
            let a = &self.actions;
            let suggestions: [(XrAction, &CStr, &CStr); 10] = [
                (a.aim_pose_action, c"/pose/aim", c"/pose/aim"),
                (a.grip_pose_action, c"/pose/grip", c"/pose/grip"),
                (a.trigger_action, c"/trigger/value", c"/trigger/value"),
                (a.squeeze_action, c"/squeeze/value", c"/squeeze/value"),
                (a.thumbstick_action, c"/thumbstick", c"/thumbstick"),
                (a.thumbstick_click_action, c"/thumbstick/click", c"/thumbstick/click"),
                (a.button_primary_action, c"/a/click", c"/x/click"),
                (a.button_secondary_action, c"/b/click", c"/y/click"),
                (a.button_menu_action, c"/menu/click", c"/menu/click"),
                (a.haptic_action, c"/output/haptic", c"/output/haptic"),
            ];
            let mut bindings: Vec<XrActionSuggestedBinding> = Vec::with_capacity(20);
            for (right_hand, left_hand) in HANDS {
                for &(action, right_suffix, left_suffix) in &suggestions {
                    if action == XR_NULL_HANDLE {
                        continue;
                    }
                    // One binding per hand (the actions carry both hand
                    // subaction paths): right hand gets the right suffix,
                    // left hand the left suffix.
                    let right_full =
                        format!("{}{}{}\0", right_hand.to_string_lossy(), "/input", right_suffix.to_string_lossy());
                    let left_full =
                        format!("{}{}{}\0", left_hand.to_string_lossy(), "/input", left_suffix.to_string_lossy());
                    if let Some(binding) = resolve_path(procs, self.instance, &right_full) {
                        bindings.push(XrActionSuggestedBinding { action, binding });
                    }
                    if let Some(binding) = resolve_path(procs, self.instance, &left_full) {
                        bindings.push(XrActionSuggestedBinding { action, binding });
                    }
                }
            }
            if bindings.is_empty() {
                continue;
            }
            let suggested = XrInteractionProfileSuggestedBinding {
                ty: XrStructureType::InteractionProfileSuggestedBinding,
                next: ptr::null(),
                interaction_profile: profile_path,
                count_suggested_bindings: bindings.len() as u32,
                suggested_bindings: bindings.as_ptr(),
            };
            // SAFETY: valid suggested-binding struct on the live instance,
            // called before `xrAttachSessionActionSets` as the spec requires.
            let res = unsafe { (procs.suggest_interaction_profile_bindings)(self.instance, &suggested) };
            if res < XR_SUCCESS {
                log::warn!(
                    "[ViveCraft] xrSuggestInteractionProfileBindings({}) failed: {res}",
                    profile.to_string_lossy()
                );
            } else {
                log::info!(
                    "[ViveCraft] Suggested {} bindings for {}",
                    bindings.len(),
                    profile.to_string_lossy()
                );
            }
        }
    }

    /// Resolves an optional OpenXR function by name through
    /// `xrGetInstanceProcAddr` (None when the runtime lacks it).
    fn opt_proc<T: Copy>(&self, name: &str) -> Option<T> {
        if self.instance == XR_NULL_HANDLE {
            return None;
        }
        // The loader's `xrGetInstanceProcAddr` is reachable through any loaded
        // proc's transitive context; store it at init instead (see `get_proc_addr`).
        let get = self.get_proc_addr?;
        let mut raw: PfnVoidFunction = None;
        let cname = format!("{name}\0");
        let res = unsafe { get(self.instance, cname.as_ptr() as *const c_char, &mut raw) };
        if res >= XR_SUCCESS && raw.is_some() {
            Some(unsafe { std::mem::transmute_copy(&raw.unwrap()) })
        } else {
            None
        }
    }

    /// True when an OpenXR instance (runtime) is loaded — the session may
    /// still need creating (`create_session_with_vulkan`).
    pub fn instance_connected(&self) -> bool {
        self.instance != XR_NULL_HANDLE && self.procs.is_some()
    }

    pub fn is_hardware_connected(&self) -> bool {
        self.hardware_connected
    }

    pub fn system_name(&self) -> &str {
        &self.system_name
    }

    /// Polls OpenXR runtime events each frame, manages session state transitions,
    /// syncs actions and locates physical controllers / HMD space.
    pub fn poll_events(&mut self) {
        let Some(procs) = self.procs else {
            return;
        };
        let mut event = XrEventDataBuffer {
            ty: XrStructureType::EventDataBuffer,
            next: ptr::null(),
            varying: [0u8; 4000],
        };

        loop {
            // SAFETY: Calling `xrPollEvent` with a valid buffer.
            let res = unsafe { (procs.poll_event)(self.instance, &mut event) };
            if res != XR_SUCCESS {
                break;
            }
            if event.ty == XrStructureType::EventDataSessionStateChanged {
                let state_event: &XrEventDataSessionStateChanged =
                    unsafe { &*(&event as *const _ as *const XrEventDataSessionStateChanged) };
                self.session_state = state_event.state;
                log::warn!("[ViveCraft] OpenXR session state changed to {:?}", self.session_state);
                match self.session_state {
                    XrSessionState::Ready => {
                        let begin_info = XrSessionBeginInfo {
                            ty: XrStructureType::SessionBeginInfo,
                            next: ptr::null(),
                            primary_view_configuration_type: XrViewConfigurationType::PrimaryStereo,
                        };
                        unsafe {
                            let _ = (procs.begin_session)(self.session, &begin_info);
                        }
                        // Swapchains are created once the session runs.
                        self.create_swapchains();
                    }
                    XrSessionState::Stopping => {
                        unsafe {
                            let _ = (procs.end_session)(self.session);
                        }
                    }
                    _ => {}
                }
            }
        }

        // If session is active, sync action sets and locate spaces.
        // §2.7: `Visible` also counts as running — poses must keep updating
        // (frozen) instead of falling back to the simulated provider whenever
        // the game window briefly loses focus while the HMD stays visible.
        if self.session != XR_NULL_HANDLE
            && (self.session_state == XrSessionState::Focused
                || self.session_state == XrSessionState::Synchronized
                || self.session_state == XrSessionState::Visible)
            && self.actions.action_set != XR_NULL_HANDLE
        {
            let active_set = XrActiveActionSet {
                action_set: self.actions.action_set,
                subaction_path: XR_NULL_PATH,
            };
            let sync_info = XrActionsSyncInfo {
                ty: XrStructureType::ActionsSyncInfo,
                next: ptr::null(),
                count_active_action_sets: 1,
                active_action_sets: &active_set,
            };
            let sync_res = unsafe { (procs.sync_actions)(self.session, &sync_info) };
            if sync_res >= XR_SUCCESS {
                // §2.4: locate with the frame's predicted display time (0
                // invalidates tracking on most runtimes, freezing poses).
                let display_time = self
                    .frame_state
                    .as_ref()
                    .map(|f| f.predicted_display_time)
                    .unwrap_or(0);
                let locate_action = |space: XrSpace| -> Option<(Vec3, Quat)> {
                    if space == XR_NULL_HANDLE || self.stage_space == XR_NULL_HANDLE {
                        return None;
                    }
                    let mut loc = XrSpaceLocation {
                        ty: XrStructureType::SpaceLocation,
                        next: ptr::null_mut(),
                        location_flags: 0,
                        pose: XrPosef::IDENTITY,
                    };
                    let res = unsafe { (procs.locate_space)(space, self.stage_space, display_time, &mut loc) };
                    if res >= XR_SUCCESS && (loc.location_flags & XR_SPACE_LOCATION_POSITION_VALID_BIT != 0) {
                        Some((loc.pose.position.to_vec3(), loc.pose.orientation.to_quat()))
                    } else {
                        None
                    }
                };

                // Right controller (0)
                if let Some((pos, rot)) = locate_action(self.actions.right_aim_space) {
                    self.current_frame.controllers[0].tracking = true;
                    self.current_frame.controllers[0].aim_position = pos;
                    self.current_frame.controllers[0].aim_rotation = rot;
                }
                if let Some((pos, rot)) = locate_action(self.actions.right_grip_space) {
                    self.current_frame.controllers[0].grip_position = pos;
                    self.current_frame.controllers[0].grip_rotation = rot;
                }

                // Left controller (1)
                if let Some((pos, rot)) = locate_action(self.actions.left_aim_space) {
                    self.current_frame.controllers[1].tracking = true;
                    self.current_frame.controllers[1].aim_position = pos;
                    self.current_frame.controllers[1].aim_rotation = rot;
                }
                if let Some((pos, rot)) = locate_action(self.actions.left_grip_space) {
                    self.current_frame.controllers[1].grip_position = pos;
                    self.current_frame.controllers[1].grip_rotation = rot;
                }

                // Locate HMD / VIEW space
                if self.view_space != XR_NULL_HANDLE && self.stage_space != XR_NULL_HANDLE {
                    let mut hmd_loc = XrSpaceLocation {
                        ty: XrStructureType::SpaceLocation,
                        next: ptr::null_mut(),
                        location_flags: 0,
                        pose: XrPosef::IDENTITY,
                    };
                    let hmd_res = unsafe { (procs.locate_space)(self.view_space, self.stage_space, display_time, &mut hmd_loc) };
                    if hmd_res >= XR_SUCCESS && (hmd_loc.location_flags & XR_SPACE_LOCATION_POSITION_VALID_BIT != 0) {
                        self.current_frame.hmd_position = hmd_loc.pose.position.to_vec3();
                        self.current_frame.hmd_rotation = hmd_loc.pose.orientation.to_quat();
                        self.hardware_connected = true;
                    }
                }

                // §2.1: read the per-hand input action states into the
                // controller fields the rest of the mod (gui / locomotion /
                // interaction) already consumes.
                self.read_action_states(&procs);

                // §2.3: locate the two eye views (per-eye pose + FOV) at the
                // predicted display time so the projection is built from the
                // runtime's FOV, not the desktop screen's.
                self.locate_views(&procs, display_time);
            }
        }
    }

    /// Reads trigger / squeeze / thumbstick / button action states for both
    /// hands into `current_frame.controllers` (§2.1). Only `is_active` results
    /// are written, so an unbound hand keeps its previous values.
    fn read_action_states(&mut self, procs: &OpenXrProcs) {
        let a = &self.actions;
        let get_info = |action: XrAction, subaction: XrPath| XrActionStateGetInfo {
            ty: XrStructureType::ActionStateGetInfo,
            next: ptr::null(),
            action,
            subaction_path: subaction,
        };
        for (hand, &subaction) in self.hand_paths.iter().enumerate() {
            // `hand_paths[0]` = right = controller 0, `[1]` = left = 1.
            let ctrl = &mut self.current_frame.controllers[hand];

            if a.trigger_action != XR_NULL_HANDLE {
                let mut state = XrActionStateFloat {
                    ty: XrStructureType::ActionStateFloat,
                    next: ptr::null_mut(),
                    current_state: 0.0,
                    changed_since_last_sync: 0,
                    last_change_time: 0,
                    is_active: 0,
                };
                let info = get_info(a.trigger_action, subaction);
                // SAFETY: valid action state structs against the live session.
                if unsafe { (procs.get_action_state_float)(self.session, &info, &mut state) }
                    >= XR_SUCCESS
                    && state.is_active != 0
                {
                    ctrl.trigger = state.current_state.clamp(0.0, 1.0);
                }
            }
            if a.squeeze_action != XR_NULL_HANDLE {
                let mut state = XrActionStateFloat {
                    ty: XrStructureType::ActionStateFloat,
                    next: ptr::null_mut(),
                    current_state: 0.0,
                    changed_since_last_sync: 0,
                    last_change_time: 0,
                    is_active: 0,
                };
                let info = get_info(a.squeeze_action, subaction);
                if unsafe { (procs.get_action_state_float)(self.session, &info, &mut state) }
                    >= XR_SUCCESS
                    && state.is_active != 0
                {
                    ctrl.squeeze = state.current_state.clamp(0.0, 1.0);
                }
            }
            if a.thumbstick_action != XR_NULL_HANDLE {
                let mut state = XrActionStateVector2f {
                    ty: XrStructureType::ActionStateVector2f,
                    next: ptr::null_mut(),
                    current_state: XrVector2f::default(),
                    changed_since_last_sync: 0,
                    last_change_time: 0,
                    is_active: 0,
                };
                let info = get_info(a.thumbstick_action, subaction);
                if unsafe { (procs.get_action_state_vector2f)(self.session, &info, &mut state) }
                    >= XR_SUCCESS
                    && state.is_active != 0
                {
                    let x = state.current_state.x.clamp(-1.0, 1.0);
                    let y = state.current_state.y.clamp(-1.0, 1.0);
                    ctrl.thumbstick = Vec2::new(x, y);
                }
            }
            for (action, target) in [
                (a.thumbstick_click_action, 0usize),
                (a.button_primary_action, 1),
                (a.button_secondary_action, 2),
                (a.button_menu_action, 3),
            ] {
                if action == XR_NULL_HANDLE {
                    continue;
                }
                let mut state = XrActionStateBoolean {
                    ty: XrStructureType::ActionStateBoolean,
                    next: ptr::null_mut(),
                    current_state: 0,
                    changed_since_last_sync: 0,
                    last_change_time: 0,
                    is_active: 0,
                };
                let info = get_info(action, subaction);
                if unsafe { (procs.get_action_state_boolean)(self.session, &info, &mut state) }
                    >= XR_SUCCESS
                    && state.is_active != 0
                {
                    let pressed = state.current_state != 0;
                    match target {
                        0 => ctrl.thumbstick_click = pressed,
                        1 => ctrl.button_primary = pressed,
                        2 => ctrl.button_secondary = pressed,
                        _ => ctrl.button_menu = pressed,
                    }
                }
            }
        }
    }

    /// `xrLocateViews` with `PRIMARY_STEREO` (§2.3): fills the per-eye poses
    /// and FOVs in `current_frame` from the runtime. On failure (or without a
    /// valid view state) the existing HMD-derived eye data is kept.
    fn locate_views(&mut self, procs: &OpenXrProcs, display_time: XrTime) {
        if self.session == XR_NULL_HANDLE || self.stage_space == XR_NULL_HANDLE {
            return;
        }
        let locate_info = XrViewLocateInfo {
            ty: XrStructureType::ViewLocateInfo,
            next: ptr::null(),
            view_configuration_type: XrViewConfigurationType::PrimaryStereo,
            display_time,
            space: self.stage_space,
        };
        let mut view_state = XrViewState {
            ty: XrStructureType::ViewState,
            next: ptr::null_mut(),
            view_state_flags: 0,
        };
        let mut views: [XrView; 2] = [
            XrView {
                ty: XrStructureType::View,
                next: ptr::null_mut(),
                pose: XrPosef::IDENTITY,
                fov: XrFovf::default(),
            },
            XrView {
                ty: XrStructureType::View,
                next: ptr::null_mut(),
                pose: XrPosef::IDENTITY,
                fov: XrFovf::default(),
            },
        ];
        let mut view_count: u32 = 0;
        // SAFETY: valid view-locate structs and a two-view capacity, the
        // capacity of PRIMARY_STEREO.
        let res = unsafe {
            (procs.locate_views)(
                self.session,
                &locate_info,
                &mut view_state,
                2,
                &mut view_count,
                views.as_mut_ptr(),
            )
        };
        if res < XR_SUCCESS || view_count < 2 {
            return;
        }
        // Only trust views whose pose the runtime marked valid; otherwise keep
        // the previous HMD-derived eye data.
        let orientation_valid =
            view_state.view_state_flags & XR_VIEW_STATE_ORIENTATION_VALID_BIT != 0;
        let position_valid = view_state.view_state_flags & XR_VIEW_STATE_POSITION_VALID_BIT != 0;
        for (i, view) in views.iter().take(2).enumerate() {
            if orientation_valid && position_valid {
                self.current_frame.eye_positions[i] = view.pose.position.to_vec3();
                self.current_frame.eye_rotations[i] = view.pose.orientation.to_quat();
            }
            // FOV is independent of pose validity flags.
            self.current_frame.eye_fovs[i] = view.fov;
        }
        self.hardware_eye_fovs_valid = true;
    }

    /// The runtime's per-eye FOVs from this frame's `xrLocateViews` (left,
    /// right) — `None` until the first successful locate, so the stereo
    /// projection falls back to the base (screen) projection (§2.3).
    pub fn hardware_eye_fov(&self) -> Option<[XrFovf; 2]> {
        (self.hardware_eye_fovs_valid && self.hardware_connected)
            .then_some(self.current_frame.eye_fovs)
    }

    /// Recenters tracking: the current HMD yaw becomes the new forward
    /// (standing calibration of the seated/stage reference), as ViveCraft's
    /// recenter key. Only affects the simulated pose baseline; a hardware
    /// HMD's reference space is owned by the runtime, so this records the
    /// current yaw as the player's chosen forward for the world-post offset.
    pub fn recenter(&mut self) {
        let yaw = self.current_frame.hmd_rotation.to_axis_angle().1;
        // Rotate the stored recenter offset so the current yaw reads as 0.
        self.recenter_yaw_offset = yaw;
        log::warn!("[ViveCraft] Tracking recentered");
    }

    /// Yaw to subtract from HMD orientation for the world-post rotation
    /// (radians), set by `recenter`.
    pub fn recenter_yaw_offset(&self) -> f32 {
        self.recenter_yaw_offset
    }

    /// Updates eye poses from the current HMD pose and IPD (in meters), and polls OSC trackers.
    pub fn update_eyes_from_hmd(&mut self, ipd: f32) {
        let half_ipd = ipd * 0.5;
        let left_offset = self.current_frame.hmd_rotation * Vec3::new(-half_ipd, 0.0, 0.0);
        let right_offset = self.current_frame.hmd_rotation * Vec3::new(half_ipd, 0.0, 0.0);
        self.current_frame.eye_positions[0] = self.current_frame.hmd_position + left_offset;
        self.current_frame.eye_positions[1] = self.current_frame.hmd_position + right_offset;
        self.current_frame.eye_rotations[0] = self.current_frame.hmd_rotation;
        self.current_frame.eye_rotations[1] = self.current_frame.hmd_rotation;

        // Poll FBT trackers from OSC receiver
        if let Some(fbt) = self.osc_receiver.poll_trackers(self.current_frame.hmd_forward()) {
            self.current_frame.fbt_trackers = Some(fbt);
        }

        self.drain_pending_haptics();
    }

    /// Syncs simulated/seated HMD orientation from player look angles when no physical HMD overrides it.
    pub fn sync_simulated_pose(&mut self, yaw_deg: f32, pitch_deg: f32, standing_height: f32, ipd: f32) {
        if self.hardware_connected {
            self.update_eyes_from_hmd(ipd);
            return;
        }
        let yaw_rad = -yaw_deg.to_radians();
        let pitch_rad = -pitch_deg.to_radians();
        let rot = Quat::from_rotation_y(yaw_rad) * Quat::from_rotation_x(pitch_rad);
        self.current_frame.hmd_rotation = rot;
        self.current_frame.hmd_position.y = standing_height;
        self.update_eyes_from_hmd(ipd);

        // Keep controllers naturally positioned in front of the player in simulated mode
        let body_rot = Quat::from_rotation_y(yaw_rad);
        self.current_frame.controllers[0].aim_position =
            self.current_frame.hmd_position + body_rot * Vec3::new(0.24, -0.38, -0.42);
        self.current_frame.controllers[0].grip_position =
            self.current_frame.hmd_position + body_rot * Vec3::new(0.24, -0.42, -0.36);
        self.current_frame.controllers[0].aim_rotation = rot;
        self.current_frame.controllers[0].grip_rotation = rot;

        self.current_frame.controllers[1].aim_position =
            self.current_frame.hmd_position + body_rot * Vec3::new(-0.24, -0.38, -0.42);
        self.current_frame.controllers[1].grip_position =
            self.current_frame.hmd_position + body_rot * Vec3::new(-0.24, -0.42, -0.36);
        self.current_frame.controllers[1].aim_rotation = rot;
        self.current_frame.controllers[1].grip_rotation = rot;

        if let Some(fbt) = self.osc_receiver.poll_trackers(self.current_frame.hmd_forward()) {
            self.current_frame.fbt_trackers = Some(fbt);
        }

        self.drain_pending_haptics();
    }

    /// Drains queued haptic pulses and dispatches them to OpenXR haptics if connected.
    pub fn drain_pending_haptics(&mut self) {
        if self.pending_haptics.is_empty() {
            return;
        }
        let Some(procs) = self.procs else {
            self.pending_haptics.clear();
            return;
        };
        if self.session == XR_NULL_HANDLE || self.actions.haptic_action == XR_NULL_HANDLE {
            self.pending_haptics.clear();
            return;
        }
        for pulse in self.pending_haptics.drain(..) {
            let subaction = if pulse.controller == 0 {
                self.hand_paths[0]
            } else {
                self.hand_paths[1]
            };
            let haptic_action_info = XrHapticActionInfo {
                ty: XrStructureType::HapticActionInfo,
                next: ptr::null(),
                action: self.actions.haptic_action,
                subaction_path: subaction,
            };
            let vibration = XrHapticVibration {
                ty: XrStructureType::HapticVibration,
                next: ptr::null(),
                duration: (pulse.duration_micros as i64) * 1000,
                frequency: pulse.frequency_hz,
                amplitude: pulse.amplitude.clamp(0.0, 1.0),
            };
            unsafe {
                let _ = (procs.apply_haptic_feedback)(
                    self.session,
                    &haptic_action_info,
                    &vibration as *const _ as *const XrHapticBaseHeader,
                );
            }
        }
    }

    /// Schedules a haptic pulse on controller `0` (Main) or `1` (Offhand).
    pub fn trigger_haptic_pulse(&mut self, controller: usize, duration_micros: u32, frequency_hz: f32, amplitude: f32) {
        if controller < 2 {
            self.pending_haptics.push(HapticPulse {
                controller,
                duration_micros,
                frequency_hz,
                amplitude: amplitude.clamp(0.0, 1.0),
            });
        }
    }

    /// Reference Full-Body Tracking (FBT) positions in room space (`MCVR.FBT_REFERENCE_POSITIONS`):
    /// `[waist, left_foot, right_foot, left_knee, right_knee, left_elbow, right_elbow]`.
    pub const FBT_REFERENCE_POSITIONS: [Vec3; 7] = [
        Vec3::new(0.0, 0.85, 0.0),    // Waist
        Vec3::new(-0.1, 0.05, -0.05), // Left Foot
        Vec3::new(0.1, 0.05, -0.05),  // Right Foot
        Vec3::new(-0.1, 0.45, -0.05), // Left Knee
        Vec3::new(0.1, 0.45, -0.05),  // Right Knee
        Vec3::new(-0.4, 1.4, 0.0),    // Left Elbow
        Vec3::new(0.4, 1.4, 0.0),     // Right Elbow
    ];

    /// Updates a Full-Body Tracking (FBT) / OSC tracker pose (`OSCTracker.java`) at `index` (`0..7`).
    pub fn set_osc_tracker_pose(&mut self, index: usize, position: Vec3, rotation: Quat) {
        if index >= 7 {
            return;
        }
        let trackers = self.current_frame.fbt_trackers.get_or_insert_with(|| {
            Self::FBT_REFERENCE_POSITIONS.map(|pos| (pos, Quat::IDENTITY))
        });
        trackers[index] = (position, rotation.normalize());
    }

    /// Create and manage OpenXR swapchains for VR stereo rendering.
    /// This should be called after xrBeginSession when the session is ready.
    ///
    /// Sizes come from `xrEnumerateViewConfigurationViews` (the runtime's
    /// recommended image rect) and the format from `xrEnumerateSwapchainFormats`,
    /// as the OpenXR render-loop spec says — never derived from FOV tangents.
    pub fn create_swapchains(&mut self) {
        if self.session == XR_NULL_HANDLE || self.procs.is_none() {
            return;
        }
        // Already created for this session.
        if self.swapchains[0] != XR_NULL_HANDLE && self.swapchains[1] != XR_NULL_HANDLE {
            return;
        }
        let procs = self.procs.as_ref().unwrap();

        // 1. The runtime's recommended eye image size for PrimaryStereo.
        let mut view_count: u32 = 0;
        let mut views_cfg: [XrViewConfigurationView; 2] = unsafe { std::mem::zeroed() };
        for v in views_cfg.iter_mut() {
            v.ty = XrStructureType::ViewConfigurationView;
        }
        let enumerate_cfg_views: Option<PfnEnumerateViewConfigurationViews> =
            self.opt_proc("xrEnumerateViewConfigurationViews");
        let mut width = 0u32;
        let mut height = 0u32;
        if let Some(enumerate) = enumerate_cfg_views {
            let res = unsafe {
                enumerate(
                    self.instance,
                    self.system_id,
                    XrViewConfigurationType::PrimaryStereo,
                    2,
                    &mut view_count,
                    views_cfg.as_mut_ptr(),
                )
            };
            if res >= XR_SUCCESS && view_count >= 2 {
                width = views_cfg[0]
                    .recommended_image_rect_width
                    .max(views_cfg[1].recommended_image_rect_width);
                height = views_cfg[0]
                    .recommended_image_rect_height
                    .max(views_cfg[1].recommended_image_rect_height);
            }
        }
        // Fallback: a sane Quest-class default (each eye 1024x1024).
        let width = if width == 0 { 1024 } else { width };
        let height = if height == 0 { 1024 } else { height };

        // 2. A supported swapchain format: prefer sRGB RGBA8 (Vulkan 37/50).
        let enumerate_formats: Option<PfnEnumerateSwapchainFormats> =
            self.opt_proc("xrEnumerateSwapchainFormats");
        let mut format = 0i64;
        if let Some(enumerate) = enumerate_formats {
            let mut count: u32 = 0;
            let res = unsafe { enumerate(self.session, 0, &mut count, ptr::null_mut()) };
            if res >= XR_SUCCESS && count > 0 {
                let mut formats: Vec<i64> = vec![0; count as usize];
                let res = unsafe { enumerate(self.session, count, &mut count, formats.as_mut_ptr()) };
                if res >= XR_SUCCESS {
                    format = *formats
                        .iter()
                        .find(|f| **f == 37 || **f == 50)
                        .unwrap_or(&formats[0]);
                }
            }
        }
        self.swapchain_format = format;
        self.swapchain_size = Some((width, height));

        let create_info = XrSwapchainCreateInfo {
            ty: XrStructureType::SwapchainCreateInfo,
            next: ptr::null(),
            create_flags: 0,
            usage_flags: XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_SAMPLED_BIT,
            format,
            sample_count: 1,
            width,
            height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        };

        // 3. One swapchain per eye.
        for eye in 0..2 {
            let mut swapchain: XrSwapchain = XR_NULL_HANDLE;
            let Some(create_swapchain) = procs.create_swapchain else {
                return;
            };
            let res = unsafe { create_swapchain(self.session, &create_info, &mut swapchain) };
            if res < XR_SUCCESS || swapchain == XR_NULL_HANDLE {
                log::warn!("[ViveCraft] xrCreateSwapchain for eye {eye} failed: {res}");
                continue;
            }
            self.swapchains[eye] = swapchain;
            // Enumerate the swapchain's images: count first, then the array.
            // XrSwapchainImageVulkan2KHR is larger than the base header, so each
            // slot gets capacity for the largest known layout.
            if let Some(enumerate_images) = procs.enumerate_swapchain_images {
                const IMAGE_STRUCT_CAPACITY: usize = 64;
                let mut image_count: u32 = 0;
                unsafe { enumerate_images(swapchain, 0, &mut image_count, ptr::null_mut()) };
                let mut raw: Vec<u8> = vec![0u8; image_count as usize * IMAGE_STRUCT_CAPACITY];
                // Write each image's type first, as the runtime requires.
                for i in 0..image_count as usize {
                    unsafe {
                        let header = raw
                            .as_mut_ptr()
                            .add(i * IMAGE_STRUCT_CAPACITY)
                            as *mut XrSwapchainImageBaseHeader;
                        (*header).ty = XrStructureType::SwapchainImageVulkan2KHR;
                    }
                }
                let mut written = image_count;
                let res = unsafe {
                    enumerate_images(
                        swapchain,
                        image_count,
                        &mut written,
                        raw.as_mut_ptr() as *mut XrSwapchainImageBaseHeader,
                    )
                };
                if res >= XR_SUCCESS {
                    raw.truncate(written as usize * IMAGE_STRUCT_CAPACITY);
                    self.swapchain_images[eye] = raw;
                }
            }
        }
        log::info!(
            "[ViveCraft] OpenXR swapchains: {width}x{height} fmt {format} (L:{}, R:{})",
            self.swapchains[0] != XR_NULL_HANDLE,
            self.swapchains[1] != XR_NULL_HANDLE
        );
    }

    /// Begin an OpenXR frame loop for swapchain submission.
    /// Returns the frame state if successful and marks the frame as pending
    /// (`end_frame` must run before the next `begin_frame`).
    pub fn begin_frame(&mut self) -> Option<XrFrameState> {
        if self.procs.is_none() || self.session == XR_NULL_HANDLE {
            return None;
        }
        // A pending frame means the render path never submitted: end it empty
        // (no layers) so the runtime keeps pacing, then start the next.
        if self.frame_pending {
            self.end_frame_pending();
        }
        let procs = self.procs.as_ref().unwrap();

        let mut frame_state = XrFrameState {
            ty: XrStructureType::FrameState,
            next: ptr::null_mut(),
            predicted_display_time: 0,
            predicted_display_period: 0,
            should_render: XR_TRUE,
        };

        let frame_wait_info = XrFrameWaitInfo {
            ty: XrStructureType::FrameWaitInfo,
            next: ptr::null(),
        };

        let Some(wait_frame_fn) = procs.wait_frame else {
            return None;
        };
        let res = unsafe { wait_frame_fn(self.session, &frame_wait_info, &mut frame_state) };
        if res >= XR_SUCCESS {
            let frame_begin_info = XrFrameBeginInfo {
                ty: XrStructureType::FrameBeginInfo,
                next: ptr::null(),
            };
            if let Some(begin_frame_fn) = procs.begin_frame {
                unsafe { let _ = begin_frame_fn(self.session, &frame_begin_info); }
            }
            self.frame_pending = true;
            let copy = XrFrameState {
                ty: frame_state.ty,
                next: ptr::null_mut(),
                predicted_display_time: frame_state.predicted_display_time,
                predicted_display_period: frame_state.predicted_display_period,
                should_render: frame_state.should_render,
            };
            self.frame_state = Some(frame_state);
            Some(copy)
        } else {
            None
        }
    }

    /// Takes the pending `XrFrameState` for submission (None when no frame began).
    pub fn take_frame_state(&mut self) -> Option<XrFrameState> {
        self.frame_state.take()
    }

    /// Submits the pending frame with no layers (the runtime keeps its pacing
    /// when the renderer could not produce eye images this frame).
    fn end_frame_pending(&mut self) {
        let Some(state) = self.frame_state.take() else {
            return;
        };
        self.frame_pending = false;
        let Some(procs) = self.procs else {
            return;
        };
        if self.session == XR_NULL_HANDLE {
            return;
        }
        let frame_end_info = XrFrameEndInfo {
            ty: XrStructureType::FrameEndInfo,
            next: ptr::null(),
            display_time: state.predicted_display_time,
            environment_blend_mode: XrEnvironmentBlendMode::Opaque,
            layer_count: 0,
            layers: ptr::null(),
        };
        if let Some(end_frame_fn) = procs.end_frame {
            unsafe { let _ = end_frame_fn(self.session, &frame_end_info); }
        }
    }

    /// Acquire and wait both eyes' swapchain images for rendering
    /// (the render side of the frame loop; release with `release_openr_images`
    /// once the GPU work is encoded).
    pub fn acquire_openr_images(&mut self) {
        let Some(procs) = self.procs else {
            return;
        };
        if self.session == XR_NULL_HANDLE {
            return;
        }
        for eye in 0..2 {
            if self.swapchains[eye] == XR_NULL_HANDLE {
                continue;
            }
            if let Some(acquire) = procs.acquire_swapchain_image {
                let mut index: u32 = 0;
                let acquire_info = XrSwapchainImageAcquireInfo {
                    ty: XrStructureType::SwapchainImageAcquireInfo,
                    next: ptr::null(),
                };
                let res = unsafe {
                    acquire(
                        self.swapchains[eye],
                        &acquire_info as *const _ as *const c_void,
                        &mut index,
                    )
                };
                if res < XR_SUCCESS {
                    log::warn!("[ViveCraft] xrAcquireSwapchainImage eye {eye}: {res}");
                    continue;
                }
                self.acquired_image_index[eye] = Some(index);
                if let Some(wait) = procs.wait_swapchain_image {
                    let wait_info = XrSwapchainImageWaitInfo {
                        ty: XrStructureType::SwapchainImageWaitInfo,
                        next: ptr::null(),
                        timeout: 1_000_000_000, // 1 s
                    };
                    let _ = unsafe {
                        wait(self.swapchains[eye], &wait_info as *const _ as *const c_void)
                    };
                }
            }
        }
    }

    /// Release both eyes' swapchain images after the frame's GPU work is
    /// encoded (the compositor can then composite them).
    pub fn release_openr_images(&mut self) {
        let Some(procs) = self.procs else {
            return;
        };
        if self.session == XR_NULL_HANDLE {
            return;
        }
        for eye in 0..2 {
            if self.acquired_image_index[eye].take().is_none() {
                continue;
            }
            if let Some(release) = procs.release_swapchain_image {
                let release_info = XrSwapchainImageReleaseInfo {
                    ty: XrStructureType::SwapchainImageReleaseInfo,
                    next: ptr::null(),
                };
                let _ = unsafe {
                    release(self.swapchains[eye], &release_info as *const _ as *const c_void)
                };
            }
        }
    }

    /// The acquired `VkImage` handle (non-dispatchable, a `u64` on 64-bit) for
    /// `eye`, from the stored `XrSwapchainImageVulkan2KHR` records: the image
    /// handle sits at offset 16 after the base header.
    pub fn vulkan_image_handle(&self, eye: usize) -> Option<u64> {
        let index = self.acquired_image_index.get(eye).copied().flatten()? as usize;
        const IMAGE_STRUCT_CAPACITY: usize = 64;
        let raw = self.swapchain_images.get(eye)?;
        let offset = index.checked_mul(IMAGE_STRUCT_CAPACITY)?.checked_add(16)?;
        let bytes: [u8; 8] = raw.get(offset..offset + 8)?.try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    }

    /// Which eye the engine renders this pass (0 = left, 1 = right).
    pub fn stereo_eye(&self) -> u8 {
        self.stereo_eye_select
    }

    /// Selects which eye the engine's world pass renders (the view-projection
    /// modifier reads this to pick `ViewSet`'s second view).
    pub fn set_stereo_eye(&mut self, eye: u8) {
        self.stereo_eye_select = eye.min(1);
    }

    /// Whether the right eye's image was already copied this frame (set by the
    /// right-eye pass's `world_drawn`; the left pass submits when it is true).
    pub fn stereo_right_done(&self) -> bool {
        self.stereo_right_done
    }

    /// Marks the right eye's image copied (the left pass resets it after
    /// submitting the frame).
    pub fn set_stereo_right_done(&mut self, done: bool) {
        self.stereo_right_done = done;
    }

    /// Whether the OpenXR session and both eye swapchains exist, so the
    /// second stereo pass is worth rendering.
    pub fn eye_textures_ready(&self) -> bool {
        self.session != XR_NULL_HANDLE
            && self.frame_pending
            && self.swapchains[0] != XR_NULL_HANDLE
            && self.swapchains[1] != XR_NULL_HANDLE
            && self.is_hardware_connected()
    }

    /// End an OpenXR frame loop and submit to the compositor with stereo layers.
    ///
    /// The eye poses and FOVs come from the frame's sampled views
    /// (`current_frame`), and the image rects from the swapchain sizes.
    /// Images are acquired/waited/released around the caller's rendering:
    /// `acquire_openxr_image` / `release_openxr_image` bracket the render
    /// encoding; this call only submits the layers.
    pub fn end_frame(&mut self, frame_state: XrFrameState) {
        if self.procs.is_none() || self.session == XR_NULL_HANDLE {
            return;
        }
        let procs = self.procs.as_ref().unwrap();
        self.frame_pending = false;
        self.frame_state = None;

        let eye_view = |eye: usize, swapchain: XrSwapchain| XrCompositionLayerProjectionView {
            ty: XrStructureType::CompositionLayerProjectionView,
            next: ptr::null(),
            pose: XrPosef {
                orientation: crate::xr::XrQuaternionf {
                    x: self.current_frame.eye_rotations[eye].x,
                    y: self.current_frame.eye_rotations[eye].y,
                    z: self.current_frame.eye_rotations[eye].z,
                    w: self.current_frame.eye_rotations[eye].w,
                },
                position: crate::xr::XrVector3f {
                    x: self.current_frame.eye_positions[eye].x,
                    y: self.current_frame.eye_positions[eye].y,
                    z: self.current_frame.eye_positions[eye].z,
                },
            },
            fov: self.current_frame.eye_fovs[eye],
            sub_image: XrSwapchainSubImage {
                swapchain,
                image_rect: XrRect2Di {
                    offset: XrOffset2Di { x: 0, y: 0 },
                    extent: XrExtent2Di {
                        width: self.swapchain_size.map(|s| s.0 as i32).unwrap_or(0),
                        height: self.swapchain_size.map(|s| s.1 as i32).unwrap_or(0),
                    },
                },
                image_array_index: 0,
            },
        };

        // No swapchains (yet): submit with no layers so the runtime keeps pacing.
        if self.swapchains[0] == XR_NULL_HANDLE || self.swapchains[1] == XR_NULL_HANDLE {
            let frame_end_info = XrFrameEndInfo {
                ty: XrStructureType::FrameEndInfo,
                next: ptr::null(),
                display_time: frame_state.predicted_display_time,
                environment_blend_mode: XrEnvironmentBlendMode::Opaque,
                layer_count: 0,
                layers: ptr::null(),
            };
            if let Some(end_frame_fn) = procs.end_frame {
                unsafe { let _ = end_frame_fn(self.session, &frame_end_info); }
            }
            return;
        }

        let views = [eye_view(0, self.swapchains[0]), eye_view(1, self.swapchains[1])];
        let projection_layer = XrCompositionLayerProjection {
            ty: XrStructureType::CompositionLayerProjection,
            next: ptr::null(),
            layer_flags: 0,
            space: self.stage_space,
            view_count: 2,
            views: views.as_ptr(),
        };

        let frame_end_info = XrFrameEndInfo {
            ty: XrStructureType::FrameEndInfo,
            next: ptr::null(),
            display_time: frame_state.predicted_display_time,
            environment_blend_mode: XrEnvironmentBlendMode::Opaque,
            layer_count: 1,
            layers: &projection_layer as *const _ as *const *const c_void,
        };

        if let Some(end_frame_fn) = procs.end_frame {
            unsafe { let _ = end_frame_fn(self.session, &frame_end_info); }
        }
    }

    /// Clean up swapchain resources
    pub fn destroy_swapchains(&mut self) {
        if self.procs.is_none() {
            return;
        }
        let procs = self.procs.as_ref().unwrap();

        if let Some(destroy_swapchain) = procs.destroy_swapchain {
            for i in 0..2 {
                if self.swapchains[i] != XR_NULL_HANDLE {
                    unsafe { let _ = destroy_swapchain(self.swapchains[i]); }
                    self.swapchains[i] = XR_NULL_HANDLE;
                    self.swapchain_images[i].clear();
                }
            }
        }
    }
}
