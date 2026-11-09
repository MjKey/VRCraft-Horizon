use glam::{Quat, Vec3};

use crate::settings::{BowMode, VrSettings, WeaponCollision};
use crate::vr_data::VrData;
use crate::xr::{ControllerState, HapticPulse, RenderEye};

/// Physical melee weapon swing tracker ported from Vivecraft's `SwingTracker.java`.
#[derive(Debug, Clone)]
pub struct SwingTracker {
    /// Minimum controller tip velocity in m/s (`2.5` in Vivecraft) required to trigger a physical attack.
    pub speed_threshold_m_per_s: f32,
    /// Last room-space weapon tip positions for mainhand (0) and offhand (1).
    pub last_tip_pos_room: [Vec3; 2],
    /// Smoothed tip speeds in m/s for `[mainhand, offhand]`.
    pub tip_speed_m_per_s: [f32; 2],
    /// Cooldown ticks remaining before the hand can trigger another roomscale swing.
    pub cooldown_ticks: [u8; 2],
    /// Length of the held tool/weapon forward from the grip in meters (`0.35` default).
    pub weapon_length_m: f32,
}

impl Default for SwingTracker {
    fn default() -> Self {
        Self {
            speed_threshold_m_per_s: 2.5,
            last_tip_pos_room: [Vec3::ZERO; 2],
            tip_speed_m_per_s: [0.0; 2],
            cooldown_ticks: [0; 2],
            weapon_length_m: 0.35,
        }
    }
}

/// Output of a single tick of physical melee / mining swing detection.
#[derive(Debug, Clone, Copy, Default)]
pub struct SwingEvent {
    /// Whether mainhand (0) triggered a physical swing this tick.
    pub mainhand_swung: bool,
    /// Whether offhand (1) triggered a physical swing this tick.
    pub offhand_swung: bool,
    /// World-space tip position of the mainhand weapon.
    pub mainhand_tip_world: Vec3,
    /// Optional haptic pulse to send to the VR runtime on impact.
    pub haptic: Option<HapticPulse>,
}

impl SwingTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates physical swing velocities at 20 Hz (`dt = 0.05`s) and triggers roomscale melee/block hits.
    pub fn update_tick(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        dt_seconds: f32,
    ) -> SwingEvent {
        let mut event = SwingEvent::default();
        let dt = dt_seconds.max(0.005);

        let controllers_room = [&room_pre.c0, &room_pre.c1];
        let controllers_world = [&world_post.c0, &world_post.c1];

        for hand in 0..2 {
            if self.cooldown_ticks[hand] > 0 {
                self.cooldown_ticks[hand] -= 1;
            }

            let tip_room = controllers_room[hand].pos_room
                + controllers_room[hand].dir_room * self.weapon_length_m;
            let delta = tip_room - self.last_tip_pos_room[hand];
            self.last_tip_pos_room[hand] = tip_room;

            let inst_speed = delta.length() / dt;
            self.tip_speed_m_per_s[hand] =
                self.tip_speed_m_per_s[hand] * 0.4 + inst_speed * 0.6;

            let collision_enabled = match settings.weapon_collision {
                WeaponCollision::Off => false,
                WeaponCollision::Auto | WeaponCollision::On => true,
            };

            if collision_enabled
                && self.cooldown_ticks[hand] == 0
                && self.tip_speed_m_per_s[hand] >= self.speed_threshold_m_per_s
            {
                self.cooldown_ticks[hand] = 6; // 300ms swing cooldown
                if hand == 0 {
                    event.mainhand_swung = true;
                } else {
                    event.offhand_swung = true;
                }
                event.haptic = Some(HapticPulse {
                    controller: hand,
                    duration_micros: 40_000,
                    frequency_hz: 160.0,
                    amplitude: (self.tip_speed_m_per_s[hand] / 5.0).clamp(0.35, 1.0),
                });
            }
        }

        event.mainhand_tip_world = controllers_world[0].get_position_f()
            + controllers_world[0].get_direction() * (self.weapon_length_m * world_post.world_scale);
        event
    }
}

/// Two-handed VR bow archery tracker ported from Vivecraft's `BowTracker.java`.
#[derive(Debug, Clone)]
pub struct BowTracker {
    /// Whether an arrow is currently nocked on the string.
    pub is_drawing: bool,
    /// Normalized bow draw fraction (`0.0..=1.0`), synced to server via `PayloadIdentifier::Draw`.
    pub current_draw: f32,
    /// Aim unit vector in world space (`normalize(bow_hand - string_hand)`).
    pub aim_direction: Vec3,
    /// Time in milliseconds the bow has been drawn (`max_charge_time = 1100 ms` in Vivecraft).
    pub draw_time_ms: f32,
    /// Previous draw step used to emit realistic string tension haptic ticks.
    last_haptic_step: u8,
}

impl Default for BowTracker {
    fn default() -> Self {
        Self {
            is_drawing: false,
            current_draw: 0.0,
            aim_direction: Vec3::NEG_Z,
            draw_time_ms: 0.0,
            last_haptic_step: 0,
        }
    }
}

/// Result of updating `BowTracker` for a tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct BowEvent {
    /// Whether the player just released a drawn arrow this tick.
    pub released_arrow: bool,
    /// Draw power (`0.0..=1.0`) when `released_arrow` is true, or current draw otherwise.
    pub draw_power: f32,
    /// World-space aim direction from the string hand through the bow hand.
    pub aim_direction: Vec3,
    /// Optional haptic pulse for string nocking / tension / release.
    pub haptic: Option<HapticPulse>,
}

impl BowTracker {
    pub const NOTCH_DIST_THRESHOLD: f32 = 0.18;
    pub const MAX_CHARGE_TIME_MS: f32 = 1100.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// Updates two-handed bow state (`BowTracker.doProcess`).
    /// Controller 1 (offhand) holds the bow riser; Controller 0 (mainhand) pulls the string.
    pub fn update(
        &mut self,
        settings: &VrSettings,
        world_post: &VrData,
        string_ctrl: &ControllerState,
        holding_bow: bool,
        dt_seconds: f32,
    ) -> BowEvent {
        let mut event = BowEvent {
            aim_direction: world_post.c0.get_direction(),
            ..Default::default()
        };

        if !holding_bow || settings.bow_mode == BowMode::Off {
            self.is_drawing = false;
            self.current_draw = 0.0;
            self.draw_time_ms = 0.0;
            self.last_haptic_step = 0;
            return event;
        }

        let scale = world_post.world_scale.max(0.25);
        let bow_pos = world_post.c1.get_position_f();
        let arrow_pos = world_post.c0.get_position_f();
        let controllers_dist = bow_pos.distance(arrow_pos);
        let max_draw_dist = 0.68 * scale;
        let notch_dist = Self::NOTCH_DIST_THRESHOLD * scale;

        if let Some(aim) = (bow_pos - arrow_pos).try_normalize() {
            self.aim_direction = aim;
            event.aim_direction = aim;
        }

        if !self.is_drawing {
            let can_nock = controllers_dist <= notch_dist * 1.4
                || settings.bow_mode == BowMode::Vanilla;
            if can_nock && string_ctrl.trigger > 0.55 {
                self.is_drawing = true;
                self.draw_time_ms = 0.0;
                self.last_haptic_step = 0;
                event.haptic = Some(HapticPulse {
                    controller: 0,
                    duration_micros: 30_000,
                    frequency_hz: 220.0,
                    amplitude: 0.45,
                });
            }
        }

        if self.is_drawing {
            if string_ctrl.trigger > 0.35 {
                self.draw_time_ms += dt_seconds * 1000.0;
                let dist_ratio = ((controllers_dist - notch_dist * 0.5) / max_draw_dist).clamp(0.0, 1.0);
                let time_ratio = (self.draw_time_ms / Self::MAX_CHARGE_TIME_MS).clamp(0.0, 1.0);
                self.current_draw = dist_ratio.min(time_ratio.max(0.25));
                event.draw_power = self.current_draw;

                let step = (self.current_draw * 10.0) as u8;
                if step > self.last_haptic_step {
                    self.last_haptic_step = step;
                    event.haptic = Some(HapticPulse {
                        controller: 0,
                        duration_micros: 20_000,
                        frequency_hz: 120.0 + f32::from(step) * 20.0,
                        amplitude: 0.2 + 0.06 * f32::from(step),
                    });
                }
            } else {
                event.released_arrow = self.current_draw > 0.12;
                event.draw_power = self.current_draw;
                event.haptic = Some(HapticPulse {
                    controller: 1,
                    duration_micros: 50_000,
                    frequency_hz: 260.0,
                    amplitude: 0.75,
                });
                self.is_drawing = false;
                self.current_draw = 0.0;
                self.draw_time_ms = 0.0;
                self.last_haptic_step = 0;
            }
        }

        event
    }
}

/// Over-the-shoulder backpack slot & offhand switching tracker ported from `BackpackTracker.java`.
#[derive(Debug, Clone)]
pub struct BackpackTracker {
    pub was_in: [bool; 2],
    pub previous_slot: usize,
}

impl Default for BackpackTracker {
    fn default() -> Self {
        Self {
            was_in: [false; 2],
            previous_slot: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BackpackEvent {
    /// If `Some(slot)`, mainhand reached behind shoulder and toggled hotbar slot `0` / `previous_slot`.
    pub toggled_mainhand_slot: Option<usize>,
    /// True if offhand reached behind shoulder to swap offhand item (`SWAP_ITEM_WITH_OFFHAND`).
    pub swapped_offhand: bool,
    pub haptic: Option<HapticPulse>,
}

impl BackpackTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates over-the-shoulder reach (`BackpackTracker.activeProcess`).
    pub fn update(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        current_hotbar_slot: usize,
    ) -> BackpackEvent {
        let mut event = BackpackEvent::default();
        if settings.seated || !settings.backpack_switching {
            self.was_in = [false; 2];
            return event;
        }

        let rear = room_pre.get_head_rear();
        let hmd_pos = Vec3::new(rear.x as f32, rear.y as f32, rear.z as f32);
        let mut hmd_dir = room_pre.hmd.get_custom_vector(Vec3::Z);
        hmd_dir.y = 0.0;
        hmd_dir = hmd_dir.normalize_or_zero();

        for c in 0..2 {
            let controller_pos = room_pre.get_controller(c).pos_room;
            let controller_dir = room_pre.get_hand(c). get_custom_vector(Vec3::NEG_Z);

            let mut delta = hmd_pos - controller_pos;
            delta.y = 0.0;
            let delta_norm = delta.normalize_or_zero();
            let dot = delta_norm.dot(hmd_dir);

            let below = (hmd_pos.y - controller_pos.y).abs() < 0.25;
            let behind = dot > 0.0 && delta.length() > 0.05;
            let aim_down = controller_dir.dot(Vec3::NEG_Y) > 0.6;

            let infront = dot < 0.0 && delta.length() > 0.10;
            let aim_up = controller_dir.dot(Vec3::NEG_Y) < 0.0;

            let zone = below && behind && aim_down;
            if zone {
                if !self.was_in[c] {
                    if c == 0 {
                        if current_hotbar_slot != 0 {
                            self.previous_slot = current_hotbar_slot;
                            event.toggled_mainhand_slot = Some(0);
                        } else {
                            event.toggled_mainhand_slot = Some(self.previous_slot);
                        }
                    } else {
                        event.swapped_offhand = true;
                    }
                    event.haptic = Some(HapticPulse {
                        controller: c,
                        duration_micros: 1_500,
                        frequency_hz: 180.0,
                        amplitude: 0.75,
                    });
                    self.was_in[c] = true;
                }
            } else if infront || aim_up {
                self.was_in[c] = false;
            }
        }

        event
    }
}

/// Physical VR mouth eating & drinking tracker ported from `EatingTracker.java`.
#[derive(Debug, Clone)]
pub struct EatingTracker {
    pub eating: [bool; 2],
    pub eat_start_ms: u64,
    pub tick_counter: u32,
}

impl Default for EatingTracker {
    fn default() -> Self {
        Self {
            eating: [false; 2],
            eat_start_ms: 0,
            tick_counter: 0,
        }
    }
}

impl EatingTracker {
    pub const MOUTH_TO_EYE_DISTANCE: f32 = 0.12;
    pub const THRESHOLD: f32 = 0.25;
    pub const EAT_TIME_MS: u64 = 2_100;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_eating(&self) -> bool {
        self.eating[0] || self.eating[1]
    }

    /// Evaluates whether either hand is holding food/drink near the player's mouth (`EatingTracker.activeProcess`).
    pub fn update(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        hand_is_edible: [bool; 2],
        hand_is_drink: [bool; 2],
        current_time_ms: u64,
    ) -> (bool, Option<HapticPulse>) {
        if settings.seated {
            self.eating = [false; 2];
            return (false, None);
        }

        self.tick_counter = self.tick_counter.wrapping_add(1);
        let hmd_pos = room_pre.hmd.pos_room;
        let mouth_pos = hmd_pos
            + room_pre
                .hmd
                .get_custom_vector(Vec3::new(0.0, -Self::MOUTH_TO_EYE_DISTANCE, 0.05));

        let mut haptic = None;
        for c in 0..2 {
            if !hand_is_edible[c] && !hand_is_drink[c] {
                self.eating[c] = false;
                continue;
            }

            let ctrl_pos = room_pre.get_controller(c).pos_room
                + room_pre.get_controller(c).get_custom_vector(Vec3::new(0.0, 0.0, -0.1))
                + room_pre.get_controller(c).dir_room * 0.1;

            if mouth_pos.distance(ctrl_pos) < Self::THRESHOLD {
                if hand_is_drink[c] && room_pre.get_controller(c).get_custom_vector(Vec3::Y).y > 0.0 {
                    // Liquid bottle isn't tilted towards mouth
                    self.eating[c] = false;
                    continue;
                }

                if !self.eating[c] {
                    self.eating[c] = true;
                    self.eat_start_ms = current_time_ms;
                }

                if self.tick_counter % 5 == 0 {
                    haptic = Some(HapticPulse {
                        controller: c,
                        duration_micros: 700,
                        frequency_hz: 150.0,
                        amplitude: 0.55,
                    });
                }

                if current_time_ms.saturating_sub(self.eat_start_ms) > Self::EAT_TIME_MS {
                    self.eating[c] = false;
                }
            } else {
                self.eating[c] = false;
            }
        }

        (self.is_eating(), haptic)
    }
}

/// Spyglass / Telescope eye-lens alignment & zoom tracker ported from `TelescopeTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct TelescopeTracker {
    pub lens_percent: [f32; 2],
}

impl TelescopeTracker {
    pub const LENS_DIST_MAX: f32 = 0.05;
    pub const LENS_DIST_MIN: f32 = 0.185;
    pub const LENS_DOT_MAX: f32 = 0.90;
    pub const LENS_DOT_MIN: f32 = 0.75;

    pub fn new() -> Self {
        Self::default()
    }

    /// Computes spyglass lens alignment factor (`0.0..=1.0`) for controller `c` (`TelescopeTracker.viewPercent`).
    pub fn view_percent(&mut self, world_render: &VrData, controller: usize) -> f32 {
        let mut out = 0.0_f32;
        for e in 0..2 {
            let eye = if e == 0 {
                RenderEye::Left
            } else {
                RenderEye::Right
            };
            let p = Self::view_percent_eye(world_render, controller, eye);
            if p > out {
                out = p;
            }
        }
        if controller < 2 {
            self.lens_percent[controller] = out;
        }
        out
    }

    fn view_percent_eye(world_render: &VrData, controller: usize, eye: RenderEye) -> f32 {
        let eye_pose = world_render.get_eye(eye);
        let con_pose = world_render.get_controller(controller);

        let dist = (eye_pose.get_position_f() - Self::get_lens_origin(world_render, controller)).length();
        let dfact = 1.0 - (dist - Self::LENS_DIST_MAX) / (Self::LENS_DIST_MIN - Self::LENS_DIST_MAX);
        let dfact = dfact.clamp(0.0, 1.0);

        let dot = eye_pose
            .get_direction()
            .dot(Self::get_view_vector(world_render, controller));
        let dotfact = (dot - Self::LENS_DOT_MIN) / (Self::LENS_DOT_MAX - Self::LENS_DOT_MIN);
        let dotfact = dotfact.clamp(0.0, 1.0);

        let _ = con_pose;
        dfact * dotfact
    }

    pub fn get_lens_origin(world_render: &VrData, controller: usize) -> Vec3 {
        let con = world_render.get_controller(controller);
        con.get_position_f()
            + Self::get_view_vector(world_render, controller) * -0.2
            + con.get_direction() * 0.05
    }

    pub fn get_view_vector(world_render: &VrData, controller: usize) -> Vec3 {
        world_render
            .get_controller(controller)
            .get_custom_vector(Vec3::NEG_Y)
    }
}

/// Handheld 3D Screenshot Camera widget tracker ported from `CameraTracker.java`.
#[derive(Debug, Clone)]
pub struct CameraTracker {
    pub visible: bool,
    pub position: Vec3,
    pub rotation: Quat,
    pub start_controller: usize,
    pub start_controller_pose_pos: Option<Vec3>,
    pub start_controller_pose_rot: Quat,
    pub quick_mode: bool,
}

impl Default for CameraTracker {
    fn default() -> Self {
        Self {
            visible: false,
            position: Vec3::new(0.0, 65.5, -1.0),
            rotation: Quat::IDENTITY,
            start_controller: 0,
            start_controller_pose_pos: None,
            start_controller_pose_rot: Quat::IDENTITY,
            quick_mode: false,
        }
    }
}

impl CameraTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle_visible(&mut self, world_render: &VrData) {
        self.visible = !self.visible;
        if self.visible {
            let hmd = &world_render.hmd;
            self.position = hmd.get_position_f() + hmd.get_direction() * (0.65 * world_render.world_scale);
            self.rotation = hmd.get_orientation();
        } else {
            self.stop_moving();
        }
    }

    pub fn start_moving(&mut self, world_pre: &VrData, controller: usize, quick_mode: bool) {
        self.start_controller = controller.min(1);
        let pose = world_pre.get_controller(self.start_controller);
        self.start_controller_pose_pos = Some(pose.get_position_f());
        self.start_controller_pose_rot = pose.get_orientation();
        self.quick_mode = quick_mode;
    }

    pub fn is_moving(&self) -> bool {
        self.start_controller_pose_pos.is_some()
    }

    pub fn stop_moving(&mut self) {
        self.start_controller_pose_pos = None;
        self.quick_mode = false;
    }

    /// Updates camera position/orientation while held by a controller (`CameraTracker.activeProcess`).
    pub fn update_tick(&mut self, world_render: &VrData) {
        if !self.visible {
            return;
        }
        if self.start_controller_pose_pos.is_some() {
            let ctrl = world_render.get_controller(self.start_controller);
            self.position = ctrl.get_position_f()
                + ctrl.get_direction() * (0.12 * world_render.world_scale);
            self.rotation = ctrl.get_orientation();
        }
    }

    /// Returns the 4 world-space corners of the handheld 3D camera viewfinder quad.
    pub fn viewfinder_corners_world(&self, world_scale: f32) -> [Vec3; 4] {
        let scale = world_scale.max(0.25);
        let hw = 0.14 * scale;
        let hh = 0.09 * scale;
        let right = self.rotation * Vec3::X;
        let up = self.rotation * Vec3::Y;
        [
            self.position - right * hw + up * hh,
            self.position + right * hw + up * hh,
            self.position + right * hw - up * hh,
            self.position - right * hw - up * hh,
        ]
    }
}

/// Environmental & directional damage haptic tracker ported from `HapticTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct HapticTracker {
    pub tick_counter: u32,
}

impl HapticTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Generates directional haptic pulses when the player receives damage (`DamageDirectionPayloadS2C`).
    pub fn on_damage_direction(
        &self,
        world_post: &VrData,
        damage_dir: Vec3,
    ) -> [Option<HapticPulse>; 2] {
        let right_axis = world_post.hmd.get_custom_vector(Vec3::X).normalize_or_zero();
        let side = damage_dir.normalize_or_zero().dot(right_axis);
        let amp_r = (0.65 + side * 0.35).clamp(0.25, 1.0);
        let amp_l = (0.65 - side * 0.35).clamp(0.25, 1.0);
        [
            Some(HapticPulse {
                controller: 0,
                duration_micros: 35_000,
                frequency_hz: 180.0,
                amplitude: amp_r,
            }),
            Some(HapticPulse {
                controller: 1,
                duration_micros: 35_000,
                frequency_hz: 180.0,
                amplitude: amp_l,
            }),
        ]
    }
}

/// Prioritized VR interaction modules ported from `InteractTracker.java` (`org.vivecraft.client_vr.gameplay.interact_modules.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveInteractModule {
    /// Priority 0: `InteractiveHotbarModule.java`
    InteractiveHotbar(i32),
    /// Priority 500: `BowModule.java`
    RoomscaleBow,
    /// Priority 750: `ThirdPersonCameraModule.java`
    ThirdPersonCamera,
    /// Priority 750: `ScreenshotCameraModule.java`
    ScreenshotCamera,
    /// Priority 1000: `EntityInteractionModule.java`
    EntityInteract,
    /// Priority 1500: `BlockInteractionModule.java`
    BlockInteract([i32; 3]),
}

/// Prioritized interaction dispatcher ported from `InteractTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct InteractTracker {
    pub active_modules: [Option<ActiveInteractModule>; 2],
    pub pressed: [bool; 2],
}

impl InteractTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates active interaction modules for both hands in priority order (`0 -> 500 -> 750 -> 1000 -> 1500`)
    /// and emits a `250 us` haptic pulse when a module newly activates.
    pub fn update<F>(
        &mut self,
        settings: &VrSettings,
        world_post: &VrData,
        hotbar_slot: i32,
        bow_notched: bool,
        camera_tracker: &CameraTracker,
        mut is_interactable_block: F,
    ) -> Option<HapticPulse>
    where
        F: FnMut(i32, i32, i32) -> bool,
    {
        if settings.seated {
            self.active_modules = [None, None];
            return None;
        }

        let mut haptic = None;
        for c in 0..2 {
            let was_active = self.active_modules[c].is_some();
            self.active_modules[c] = None;
            let hand_pos = world_post.get_controller(c).get_position_f();

            // Priority 0: InteractiveHotbarModule (mainhand only)
            if c == 0 && hotbar_slot >= 0 {
                self.active_modules[c] = Some(ActiveInteractModule::InteractiveHotbar(hotbar_slot));
            }
            // Priority 500: BowModule
            else if c == 0 && bow_notched {
                self.active_modules[c] = Some(ActiveInteractModule::RoomscaleBow);
            }
            // Priority 750: ScreenshotCameraModule
            else if camera_tracker.visible
                && !camera_tracker.quick_mode
                && hand_pos.distance(camera_tracker.position) < 0.11 * world_post.world_scale
            {
                self.active_modules[c] = Some(ActiveInteractModule::ScreenshotCamera);
            }
            // Priority 1500: BlockInteractionModule
            else {
                let bx = hand_pos.x.floor() as i32;
                let by = hand_pos.y.floor() as i32;
                let bz = hand_pos.z.floor() as i32;
                if is_interactable_block(bx, by, bz) {
                    self.active_modules[c] = Some(ActiveInteractModule::BlockInteract([bx, by, bz]));
                }
            }

            if !was_active && self.active_modules[c].is_some() {
                haptic = Some(HapticPulse {
                    controller: c,
                    duration_micros: 250,
                    frequency_hz: 160.0,
                    amplitude: 0.4,
                });
            }
        }

        haptic
    }
}

/// Result of a 6DoF controller voxel raycast (`raycast_voxels`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelHit {
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,
    pub hit_pos: Vec3,
    pub face_normal: [i32; 3],
    pub distance: f32,
}

/// Fast Amanatides-Woo 3D DDA voxel raycaster from a 6DoF VR controller pose.
pub fn raycast_voxels<F>(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    mut is_solid: F,
) -> Option<VoxelHit>
where
    F: FnMut(i32, i32, i32) -> bool,
{
    let dir = direction.try_normalize()?;
    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let step_x = if dir.x >= 0.0 { 1 } else { -1 };
    let step_y = if dir.y >= 0.0 { 1 } else { -1 };
    let step_z = if dir.z >= 0.0 { 1 } else { -1 };

    let t_delta_x = if dir.x.abs() > 1.0e-6 { (1.0 / dir.x).abs() } else { f32::INFINITY };
    let t_delta_y = if dir.y.abs() > 1.0e-6 { (1.0 / dir.y).abs() } else { f32::INFINITY };
    let t_delta_z = if dir.z.abs() > 1.0e-6 { (1.0 / dir.z).abs() } else { f32::INFINITY };

    let mut t_max_x = if dir.x >= 0.0 {
        ((x + 1) as f32 - origin.x) * t_delta_x
    } else {
        (origin.x - x as f32) * t_delta_x
    };
    let mut t_max_y = if dir.y >= 0.0 {
        ((y + 1) as f32 - origin.y) * t_delta_y
    } else {
        (origin.y - y as f32) * t_delta_y
    };
    let mut t_max_z = if dir.z >= 0.0 {
        ((z + 1) as f32 - origin.z) * t_delta_z
    } else {
        (origin.z - z as f32) * t_delta_z
    };

    let mut face_normal = [0, 0, 0];
    let mut dist = 0.0;

    for _ in 0..128 {
        if is_solid(x, y, z) {
            return Some(VoxelHit {
                block_x: x,
                block_y: y,
                block_z: z,
                hit_pos: origin + dir * dist,
                face_normal,
                distance: dist,
            });
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                dist = t_max_x;
                if dist > max_distance {
                    break;
                }
                x += step_x;
                t_max_x += t_delta_x;
                face_normal = [-step_x, 0, 0];
            } else {
                dist = t_max_z;
                if dist > max_distance {
                    break;
                }
                z += step_z;
                t_max_z += t_delta_z;
                face_normal = [0, 0, -step_z];
            }
        } else if t_max_y < t_max_z {
            dist = t_max_y;
            if dist > max_distance {
                break;
            }
            y += step_y;
            t_max_y += t_delta_y;
            face_normal = [0, -step_y, 0];
        } else {
            dist = t_max_z;
            if dist > max_distance {
                break;
            }
            z += step_z;
            t_max_z += t_delta_z;
            face_normal = [0, 0, -step_z];
        }
    }

    None
}

/// Physical roomscale shield blocking tracker ported from Vivecraft (`VRPlayer.java:1200-1250`).
#[derive(Debug, Clone, Default)]
pub struct ShieldTracker {
    /// Whether shield is currently held in blocking pose.
    pub is_blocking: bool,
    /// Controller index holding the shield (default 1 for offhand).
    pub shield_controller: usize,
}

impl ShieldTracker {
    pub fn new() -> Self {
        Self {
            is_blocking: false,
            shield_controller: 1,
        }
    }

    /// Evaluates physical shield orientation and returns true if incoming damage from `damage_source_dir`
    /// is deflected within the ±60° cone of the shield facing direction.
    pub fn can_block_attack(
        &mut self,
        world_post: &VrData,
        damage_source_dir: Vec3,
        has_shield: bool,
    ) -> (bool, Option<HapticPulse>) {
        if !has_shield {
            self.is_blocking = false;
            return (false, None);
        }

        let ctrl = world_post.get_controller(self.shield_controller);
        let shield_normal = ctrl.get_direction();

        // Check if shield faces against the incoming damage vector (dot product <= -0.5 is within ±60°)
        let incoming_norm = damage_source_dir.normalize_or_zero();
        let dot = shield_normal.dot(-incoming_norm);
        let blocked = dot >= 0.5;
        self.is_blocking = blocked;

        if blocked {
            (
                true,
                Some(HapticPulse {
                    controller: self.shield_controller,
                    duration_micros: 60_000,
                    frequency_hz: 180.0,
                    amplitude: 0.9,
                }),
            )
        } else {
            (false, None)
        }
    }
}

/// VR Elytra flight tracker ported from Vivecraft (`VRPlayer.java:1320-1390`).
/// Handles arms-out glide posture, tilt steering, and flapping impulse.
#[derive(Debug, Clone, Default)]
pub struct ElytraTracker {
    pub is_gliding: bool,
    pub flap_cooldown: u8,
    pub last_c0_y: f32,
    pub last_c1_y: f32,
}

impl ElytraTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates VR Elytra steering and flapping impulse.
    /// Returns `(pitch_offset_rad, yaw_offset_rad, impulse_vector)`.
    pub fn update(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        is_fall_flying: bool,
        dt_seconds: f32,
    ) -> (f32, f32, Vec3) {
        if !is_fall_flying || !settings.vr_enabled {
            self.is_gliding = false;
            return (0.0, 0.0, Vec3::ZERO);
        }

        if self.flap_cooldown > 0 {
            self.flap_cooldown -= 1;
        }

        let c0_pos = room_pre.c0.pos_room;
        let c1_pos = room_pre.c1.pos_room;
        let arm_dist = c0_pos.distance(c1_pos);

        // Arms out glide: distance between controllers > 0.8m
        self.is_gliding = arm_dist > 0.8;

        // Steering by roll (bank angle between hands):
        // left higher than right -> turn right; right higher -> turn left
        let roll_bank = (c0_pos.y - c1_pos.y) / arm_dist.max(0.5);
        let yaw_rate = roll_bank * 1.5;

        // Pitch steering: both hands raised -> pitch up; both lowered -> dive
        let avg_arm_y = (c0_pos.y + c1_pos.y) * 0.5;
        let hmd_y = room_pre.hmd.pos_room.y;
        let pitch_offset = ((avg_arm_y - (hmd_y - 0.2)) * 1.2).clamp(-0.8, 0.8);

        // Downward flap detection for small boost
        let dt = dt_seconds.max(0.005);
        let dy0 = (c0_pos.y - self.last_c0_y) / dt;
        let dy1 = (c1_pos.y - self.last_c1_y) / dt;
        self.last_c0_y = c0_pos.y;
        self.last_c1_y = c1_pos.y;

        let mut impulse = Vec3::ZERO;
        if self.flap_cooldown == 0 && dy0 < -1.2 && dy1 < -1.2 {
            self.flap_cooldown = 15; // 750ms cooldown
            let forward = world_post.hmd.get_direction();
            impulse = forward * 0.35 + Vec3::new(0.0, 0.25, 0.0);
        }

        (pitch_offset, yaw_rate, impulse)
    }
}
