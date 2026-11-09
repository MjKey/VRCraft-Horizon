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
pub enum XrStructureType {
    #[default]
    Unknown = 0,
    InstanceCreateInfo = 3,
    SystemGetInfo = 4,
    SystemProperties = 5,
    ViewLocateInfo = 6,
    View = 7,
    SessionCreateInfo = 8,
    SessionBeginInfo = 9,
    SwapchainCreateInfo = 10,
    SwapchainImageBaseHeader = 11,
    FrameEndInfo = 12,
    EventDataBuffer = 16,
    EventDataSessionStateChanged = 17,
    HapticVibration = 25,
    FrameWaitInfo = 33,
    ReferenceSpaceCreateInfo = 37,
    SessionActionSetsAttachInfo = 38,
    ActionsSyncInfo = 39,
    ActionSetCreateInfo = 40,
    ActionCreateInfo = 41,
    SpaceLocation = 42,
    ActionSpaceCreateInfo = 43,
    ViewState = 44,
    FrameState = 45,
    ActionStateGetInfo = 46,
    FrameBeginInfo = 47,
    ActionStateBoolean = 48,
    ActionStateFloat = 49,
    ActionStateVector2f = 50,
    ActionStatePose = 51,
    InteractionProfileSuggestedBinding = 52,
    HapticActionInfo = 53,
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
    #[allow(dead_code)]
    pub system_id: XrSystemId,
    system_name: String,
    hardware_connected: bool,
    pub session: XrSession,
    pub session_state: XrSessionState,
    pub stage_space: XrSpace,
    pub view_space: XrSpace,
    pub actions: XrActionsState,
    pub hand_paths: [XrPath; 2],
    pub procs: Option<OpenXrProcs>,
    // Swapchain management for VR stereo rendering
    pub swapchains: [XrSwapchain; 2],
    pub swapchain_images: [Vec<XrSwapchainImageBaseHeader>; 2],
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
            Ok((lib, instance, destroy_instance, system_id, system_name, procs, session, stage_space, view_space, actions, hand_paths)) => {
                log::info!(
                    "[ViveCraft] OpenXR runtime initialized: system '{}' (id={})",
                    system_name,
                    system_id
                );
                Self {
                    _library: Some(lib),
                    instance,
                    destroy_instance: Some(destroy_instance),
                    system_id,
                    system_name,
                    hardware_connected: false,
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
                    system_id: XR_NULL_SYSTEM_ID,
                    system_name: "ViveCraft Simulated 6DoF (NullVR)".to_owned(),
                    hardware_connected: false,
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
                    osc_receiver,
                    current_frame: RawXrFrame::default(),
                    pending_haptics: Vec::new(),
                }
            }
        }
    }

    fn loader_candidates() -> &'static [&'static str] {
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

    fn write_fixed_cstr<const N: usize>(target: &mut [c_char; N], text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(N.saturating_sub(1));
        for (i, &b) in bytes[..len].iter().enumerate() {
            target[i] = b as c_char;
        }
        target[len] = 0;
    }

    fn try_init_openxr() -> Result<
        (
            Library,
            XrInstance,
            PfnDestroyInstance,
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
        for &candidate in Self::loader_candidates() {
            // SAFETY: Loading the standard Khronos OpenXR loader shared library.
            match unsafe { Library::new(candidate) } {
                Ok(lib) => {
                    loaded_lib = Some(lib);
                    break;
                }
                Err(e) => {
                    last_err = format!("{candidate}: {e}");
                }
            }
        }
        let lib = loaded_lib.ok_or(last_err)?;

        // SAFETY: `xrGetInstanceProcAddr` is the canonical entry point of OpenXR 1.0.
        let get_proc: PfnGetInstanceProcAddr = unsafe {
            *lib.get::<PfnGetInstanceProcAddr>(b"xrGetInstanceProcAddr\0")
                .map_err(|e| format!("missing xrGetInstanceProcAddr: {e}"))?
        };

        let mut create_fn_raw: PfnVoidFunction = None;
        // SAFETY: Resolving `xrCreateInstance` with `XR_NULL_HANDLE`.
        let res = unsafe {
            get_proc(
                XR_NULL_HANDLE,
                c"xrCreateInstance".as_ptr(),
                &mut create_fn_raw,
            )
        };
        if res < XR_SUCCESS || create_fn_raw.is_none() {
            return Err(format!("xrGetInstanceProcAddr(xrCreateInstance) failed: {res}"));
        }
        // SAFETY: Function signature matches `PfnCreateInstance` per OpenXR 1.0 spec.
        let create_instance: PfnCreateInstance =
            unsafe { std::mem::transmute(create_fn_raw.unwrap()) };

        let mut app_info = XrApplicationInfo {
            application_name: [0; XR_MAX_APPLICATION_NAME_SIZE],
            application_version: 1,
            engine_name: [0; XR_MAX_ENGINE_NAME_SIZE],
            engine_version: 1,
            api_version: XR_CURRENT_API_VERSION,
        };
        Self::write_fixed_cstr(&mut app_info.application_name, "ViveCraft-Horizon");
        Self::write_fixed_cstr(&mut app_info.engine_name, "Horizon-Rust");

        let create_info = XrInstanceCreateInfo {
            ty: XrStructureType::InstanceCreateInfo,
            next: ptr::null(),
            create_flags: 0,
            application_info: app_info,
            enabled_api_layer_count: 0,
            enabled_api_layer_names: ptr::null(),
            enabled_extension_count: 0,
            enabled_extension_names: ptr::null(),
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
        let get_system: PfnGetSystem = unsafe { std::mem::transmute(get_system_fn) };

        let sys_info = XrSystemGetInfo {
            ty: XrStructureType::SystemGetInfo,
            next: ptr::null(),
            form_factor: XrFormFactor::HeadMountedDisplay,
        };
        let mut system_id: XrSystemId = XR_NULL_SYSTEM_ID;
        let res = unsafe { get_system(instance, &sys_info, &mut system_id) };
        if res < XR_SUCCESS || system_id == XR_NULL_SYSTEM_ID {
            unsafe {
                let _ = destroy_instance(instance);
            }
            return Err(format!("no HMD present (xrGetSystem={res})"));
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

        unsafe fn load_pfn<T: Copy>(get_proc: PfnGetInstanceProcAddr, instance: XrInstance, name: &CStr) -> Result<T, String> {
            let mut raw: PfnVoidFunction = None;
            let res = unsafe { get_proc(instance, name.as_ptr(), &mut raw) };
            if res < XR_SUCCESS || raw.is_none() {
                return Err(format!("failed to load {}: res={res}", name.to_string_lossy()));
            }
            Ok(unsafe { std::mem::transmute_copy(&raw.unwrap()) })
        }

        unsafe fn opt_pfn<T: Copy>(get_proc: PfnGetInstanceProcAddr, instance: XrInstance, name: &CStr) -> Option<T> {
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
                create_session: load_pfn(get_proc, instance, c"xrCreateSession")?,
                destroy_session: load_pfn(get_proc, instance, c"xrDestroySession")?,
                begin_session: load_pfn(get_proc, instance, c"xrBeginSession")?,
                end_session: load_pfn(get_proc, instance, c"xrEndSession")?,
                create_reference_space: load_pfn(get_proc, instance, c"xrCreateReferenceSpace")?,
                destroy_space: load_pfn(get_proc, instance, c"xrDestroySpace")?,
                locate_space: load_pfn(get_proc, instance, c"xrLocateSpace")?,
                poll_event: load_pfn(get_proc, instance, c"xrPollEvent")?,
                string_to_path: load_pfn(get_proc, instance, c"xrStringToPath")?,
                create_action_set: load_pfn(get_proc, instance, c"xrCreateActionSet")?,
                create_action: load_pfn(get_proc, instance, c"xrCreateAction")?,
                suggest_interaction_profile_bindings: load_pfn(get_proc, instance, c"xrSuggestInteractionProfileBindings")?,
                attach_session_action_sets: load_pfn(get_proc, instance, c"xrAttachSessionActionSets")?,
                create_action_space: load_pfn(get_proc, instance, c"xrCreateActionSpace")?,
                sync_actions: load_pfn(get_proc, instance, c"xrSyncActions")?,
                get_action_state_boolean: load_pfn(get_proc, instance, c"xrGetActionStateBoolean")?,
                get_action_state_float: load_pfn(get_proc, instance, c"xrGetActionStateFloat")?,
                get_action_state_vector2f: load_pfn(get_proc, instance, c"xrGetActionStateVector2f")?,
                get_action_state_pose: load_pfn(get_proc, instance, c"xrGetActionStatePose")?,
                apply_haptic_feedback: load_pfn(get_proc, instance, c"xrApplyHapticFeedback")?,
                stop_haptic_feedback: load_pfn(get_proc, instance, c"xrStopHapticFeedback")?,
                locate_views: load_pfn(get_proc, instance, c"xrLocateViews")?,
                create_swapchain: opt_pfn(get_proc, instance, c"xrCreateSwapchain"),
                destroy_swapchain: opt_pfn(get_proc, instance, c"xrDestroySwapchain"),
                enumerate_swapchain_images: opt_pfn(get_proc, instance, c"xrEnumerateSwapchainImages"),
                acquire_swapchain_image: opt_pfn(get_proc, instance, c"xrAcquireSwapchainImage"),
                wait_swapchain_image: opt_pfn(get_proc, instance, c"xrWaitSwapchainImage"),
                release_swapchain_image: opt_pfn(get_proc, instance, c"xrReleaseSwapchainImage"),
                wait_frame: opt_pfn(get_proc, instance, c"xrWaitFrame"),
                begin_frame: opt_pfn(get_proc, instance, c"xrBeginFrame"),
                end_frame: opt_pfn(get_proc, instance, c"xrEndFrame"),
            }
        };

        let mut session = XR_NULL_HANDLE;
        let mut stage_space = XR_NULL_HANDLE;
        let mut view_space = XR_NULL_HANDLE;
        let mut actions = XrActionsState::default();
        let mut hand_paths = [XR_NULL_PATH, XR_NULL_PATH];

        let session_create_info = XrSessionCreateInfo {
            ty: XrStructureType::SessionCreateInfo,
            next: ptr::null(),
            create_flags: 0,
            system_id,
        };
        let _ = unsafe { (procs.create_session)(instance, &session_create_info, &mut session) };

        if session != XR_NULL_HANDLE {
            let stage_info = XrReferenceSpaceCreateInfo {
                ty: XrStructureType::ReferenceSpaceCreateInfo,
                next: ptr::null(),
                reference_space_type: XrReferenceSpaceType::Stage,
                pose_in_reference_space: XrPosef::IDENTITY,
            };
            let res = unsafe { (procs.create_reference_space)(session, &stage_info, &mut stage_space) };
            if res < XR_SUCCESS || stage_space == XR_NULL_HANDLE {
                let local_info = XrReferenceSpaceCreateInfo {
                    ty: XrStructureType::ReferenceSpaceCreateInfo,
                    next: ptr::null(),
                    reference_space_type: XrReferenceSpaceType::Local,
                    pose_in_reference_space: XrPosef::IDENTITY,
                };
                unsafe {
                    let _ = (procs.create_reference_space)(session, &local_info, &mut stage_space);
                }
            }

            let view_info = XrReferenceSpaceCreateInfo {
                ty: XrStructureType::ReferenceSpaceCreateInfo,
                next: ptr::null(),
                reference_space_type: XrReferenceSpaceType::View,
                pose_in_reference_space: XrPosef::IDENTITY,
            };
            unsafe {
                let _ = (procs.create_reference_space)(session, &view_info, &mut view_space);
            }

            unsafe {
                let _ = (procs.string_to_path)(instance, c"/user/hand/right".as_ptr(), &mut hand_paths[0]);
                let _ = (procs.string_to_path)(instance, c"/user/hand/left".as_ptr(), &mut hand_paths[1]);
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
            let set_res = unsafe { (procs.create_action_set)(instance, &set_info, &mut actions.action_set) };
            if set_res >= XR_SUCCESS && actions.action_set != XR_NULL_HANDLE {
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
                        let _ = (procs.create_action)(actions.action_set, &act_info, &mut act);
                    }
                    act
                };

                actions.aim_pose_action = create_act("aim_pose", "Aim Pose", XrActionType::PoseInput);
                actions.grip_pose_action = create_act("grip_pose", "Grip Pose", XrActionType::PoseInput);
                actions.trigger_action = create_act("trigger", "Trigger", XrActionType::FloatInput);
                actions.squeeze_action = create_act("squeeze", "Squeeze", XrActionType::FloatInput);
                actions.thumbstick_action = create_act("thumbstick", "Thumbstick", XrActionType::Vector2fInput);
                actions.haptic_action = create_act("haptic", "Haptic", XrActionType::VibrationOutput);

                let attach_info = XrSessionActionSetsAttachInfo {
                    ty: XrStructureType::SessionActionSetsAttachInfo,
                    next: ptr::null(),
                    count_action_sets: 1,
                    action_sets: &actions.action_set,
                };
                unsafe {
                    let _ = (procs.attach_session_action_sets)(session, &attach_info);
                }

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
                        let _ = (procs.create_action_space)(session, &sp_info, &mut sp);
                    }
                    sp
                };

                actions.right_aim_space = create_act_space(actions.aim_pose_action, hand_paths[0]);
                actions.left_aim_space = create_act_space(actions.aim_pose_action, hand_paths[1]);
                actions.right_grip_space = create_act_space(actions.grip_pose_action, hand_paths[0]);
                actions.left_grip_space = create_act_space(actions.grip_pose_action, hand_paths[1]);
            }
        }

        Ok((lib, instance, destroy_instance, system_id, system_name, procs, session, stage_space, view_space, actions, hand_paths))
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
                log::info!("[ViveCraft] OpenXR session state changed to {:?}", self.session_state);
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

        // If session is active, sync action sets and locate spaces
        if self.session != XR_NULL_HANDLE
            && (self.session_state == XrSessionState::Focused || self.session_state == XrSessionState::Synchronized)
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
                    let res = unsafe { (procs.locate_space)(space, self.stage_space, 0, &mut loc) };
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
                    let hmd_res = unsafe { (procs.locate_space)(self.view_space, self.stage_space, 0, &mut hmd_loc) };
                    if hmd_res >= XR_SUCCESS && (hmd_loc.location_flags & XR_SPACE_LOCATION_POSITION_VALID_BIT != 0) {
                        self.current_frame.hmd_position = hmd_loc.pose.position.to_vec3();
                        self.current_frame.hmd_rotation = hmd_loc.pose.orientation.to_quat();
                        self.hardware_connected = true;
                    }
                }
            }
        }
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
    pub fn create_swapchains(&mut self) {
        if self.session == XR_NULL_HANDLE || self.procs.is_none() {
            return;
        }
        let procs = self.procs.as_ref().unwrap();

        // Enumerate view configuration views to get recommended image dimensions
        let view_locate_info = XrViewLocateInfo {
            ty: XrStructureType::ViewLocateInfo,
            next: ptr::null(),
            view_configuration_type: XrViewConfigurationType::PrimaryStereo,
            display_time: 0,
            space: self.stage_space,
        };
        let mut view_state = XrViewState {
            ty: XrStructureType::ViewState,
            next: ptr::null_mut(),
            view_state_flags: 0,
        };
        let mut view_count_output: u32 = 0;
        let mut view_capacity_input = 2;
        let mut view_array: [XrView; 2] = unsafe { std::mem::zeroed() };

        if unsafe { (procs.locate_views)(self.session, &view_locate_info, &mut view_state, view_capacity_input, &mut view_count_output, &mut view_array as *mut _ as *mut XrView) } >= XR_SUCCESS {
            // Create swapchain with suitable format
            let mut create_info = XrSwapchainCreateInfo {
                ty: XrStructureType::SwapchainCreateInfo,
                next: ptr::null(),
                create_flags: 0,
                usage_flags: XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_SAMPLED_BIT,
                format: self.swapchain_format,
                sample_count: 1,
                width: view_array[0].fov.angle_right.tan().max(view_array[1].fov.angle_right.tan()) as u32 * 2,
                height: view_array[0].fov.angle_up.tan().max(view_array[1].fov.angle_up.tan()) as u32 * 2,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            };

            // Create left swapchain
            let mut left_swapchain: XrSwapchain = XR_NULL_HANDLE;
            if let Some(create_swapchain) = procs.create_swapchain {
                let res = unsafe { create_swapchain(self.session, &create_info, &mut left_swapchain) };
                if res >= XR_SUCCESS && left_swapchain != XR_NULL_HANDLE {
                    self.swapchains[0] = left_swapchain;
                    // Enumerate images
                    let mut left_image_count: u32 = 0;
                    if let Some(enumerate_images) = procs.enumerate_swapchain_images {
                        unsafe { enumerate_images(left_swapchain, 0, &mut left_image_count, ptr::null_mut()) };
                        let mut left_images: Vec<XrSwapchainImageBaseHeader> = Vec::with_capacity(left_image_count as usize);
                        unsafe { enumerate_images(left_swapchain, left_image_count, &mut left_image_count, left_images.as_mut_ptr() as *mut XrSwapchainImageBaseHeader) };
                        self.swapchain_images[0] = left_images;
                    }
                }
            }

            // Create right swapchain (same parameters, separate handle)
            let mut right_swapchain: XrSwapchain = XR_NULL_HANDLE;
            if let Some(create_swapchain) = procs.create_swapchain {
                let res = unsafe { create_swapchain(self.session, &create_info, &mut right_swapchain) };
                if res >= XR_SUCCESS && right_swapchain != XR_NULL_HANDLE {
                    self.swapchains[1] = right_swapchain;
                    // Enumerate images
                    let mut right_image_count: u32 = 0;
                    if let Some(enumerate_images) = procs.enumerate_swapchain_images {
                        unsafe { enumerate_images(right_swapchain, 0, &mut right_image_count, ptr::null_mut()) };
                        let mut right_images: Vec<XrSwapchainImageBaseHeader> = Vec::with_capacity(right_image_count as usize);
                        unsafe { enumerate_images(right_swapchain, right_image_count, &mut right_image_count, right_images.as_mut_ptr() as *mut XrSwapchainImageBaseHeader) };
                        self.swapchain_images[1] = right_images;
                    }
                }
            }
        }
    }

    /// Begin an OpenXR frame loop for swapchain submission.
    /// Returns the frame state if successful.
    pub fn begin_frame(&mut self) -> Option<XrFrameState> {
        if self.procs.is_none() || self.session == XR_NULL_HANDLE {
            return None;
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

        let wait_frame_fn = procs.wait_frame.ok_or_else(|| XR_SUCCESS - 1).unwrap();
        let res = unsafe { wait_frame_fn(self.session, &frame_wait_info, &mut frame_state) };
        if res >= XR_SUCCESS {
            // Now begin the frame
            let frame_begin_info = XrFrameBeginInfo {
                ty: XrStructureType::FrameBeginInfo,
                next: ptr::null(),
            };
            let begin_frame_fn = procs.begin_frame.ok_or_else(|| XR_SUCCESS - 1).unwrap();
            let _ = unsafe { begin_frame_fn(self.session, &frame_begin_info) };
            Some(frame_state)
        } else {
            None
        }
    }

    /// End an OpenXR frame loop and submit to the compositor.
    pub fn end_frame(&mut self, frame_state: XrFrameState) {
        if self.procs.is_none() || self.session == XR_NULL_HANDLE {
            return;
        }
        let procs = self.procs.as_ref().unwrap();

        // Create composition layer projection (simplified - in real implementation would need actual textures)
        let frame_end_info = XrFrameEndInfo {
            ty: XrStructureType::FrameEndInfo,
            next: ptr::null(),
            display_time: frame_state.predicted_display_time,
            environment_blend_mode: XrEnvironmentBlendMode::Opaque,
            layer_count: 0,
            layers: ptr::null(),
        };

        // In a full implementation, we would create XrCompositionLayerProjection
        // with swapchain images for both eyes and pass it to xrEndFrame
        let end_frame_fn = procs.end_frame.ok_or_else(|| XR_SUCCESS - 1).unwrap();
        let _ = unsafe { end_frame_fn(self.session, &frame_end_info) };
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
