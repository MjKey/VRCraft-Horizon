use glam::{Mat4, Quat, Vec2, Vec3};

use crate::vr_data::VrData;
use crate::xr::HapticPulse;

/// 8-slice radial menu actions ported from `GuiRadial.java` & `RadialHandler.java`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialSliceAction {
    TogglePerspective = 0,
    OpenInventory = 1,
    DropHeldItem = 2,
    OpenChat = 3,
    ToggleKeyboard = 4,
    ToggleScreenshotCamera = 5,
    CalibrateHeight = 6,
    ToggleSeatedMode = 7,
}

impl RadialSliceAction {
    pub const ALL: [Self; 8] = [
        Self::TogglePerspective,
        Self::OpenInventory,
        Self::DropHeldItem,
        Self::OpenChat,
        Self::ToggleKeyboard,
        Self::ToggleScreenshotCamera,
        Self::CalibrateHeight,
        Self::ToggleSeatedMode,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::TogglePerspective => "Perspective",
            Self::OpenInventory => "Inventory",
            Self::DropHeldItem => "Drop Item",
            Self::OpenChat => "Chat",
            Self::ToggleKeyboard => "VR Keyboard",
            Self::ToggleScreenshotCamera => "VR Camera",
            Self::CalibrateHeight => "Calibrate Height",
            Self::ToggleSeatedMode => "Seated / Standing",
        }
    }
}

/// 3D Radial Menu handler ported from `RadialHandler.java`.
#[derive(Debug, Clone)]
pub struct RadialHandler {
    pub showing: bool,
    pub pos_room: Vec3,
    pub rotation_room: Quat,
    pub active_controller: usize,
    pub hovered_slice: Option<RadialSliceAction>,
    pub shift_layer: bool,
}

impl Default for RadialHandler {
    fn default() -> Self {
        Self {
            showing: false,
            pos_room: Vec3::new(0.0, 1.3, -1.2),
            rotation_room: Quat::IDENTITY,
            active_controller: 0,
            hovered_slice: None,
            shift_layer: false,
        }
    }
}

impl RadialHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Anchors the radial menu in room space (`RadialHandler.orientOverlay`).
    pub fn orient_overlay(&mut self, room_pre: &VrData, controller: usize, hold_mode: bool) {
        self.active_controller = controller.min(1);
        let (pose, distance) = if hold_mode {
            (room_pre.get_controller(self.active_controller), 1.2_f32)
        } else {
            (&room_pre.hmd, 2.0_f32)
        };
        let offset = pose.dir_room * (distance * 0.5);
        self.pos_room = pose.pos_room + offset;
        let len = offset.length().max(1.0e-5);
        let pitch = (offset.y / len).clamp(-1.0, 1.0).asin();
        let yaw = std::f32::consts::PI + offset.x.atan2(offset.z);
        self.rotation_room =
            Quat::from_axis_angle(Vec3::Y, yaw) * Quat::from_axis_angle(Vec3::X, pitch);
    }

    /// Updates radial sector selection from normalized UV cursor coordinates (`0.0..=1.0`).
    pub fn update_slice_from_uv(&mut self, uv: Vec2, cursor_on_gui: bool) -> Option<RadialSliceAction> {
        if !self.showing || !cursor_on_gui {
            self.hovered_slice = None;
            return None;
        }
        let centered = (uv - Vec2::splat(0.5)) * 2.0;
        let radius = centered.length();
        if !(0.18..=0.98).contains(&radius) {
            self.hovered_slice = None;
            return None;
        }
        let angle = (-centered.y).atan2(centered.x);
        let normalized = (angle + std::f32::consts::PI / 8.0).rem_euclid(std::f32::consts::TAU);
        let idx = ((normalized / std::f32::consts::TAU) * 8.0).floor() as usize % 8;
        let slice = RadialSliceAction::ALL[idx];
        self.hovered_slice = Some(slice);
        Some(slice)
    }

    /// Takes the currently hovered slice as the action to execute and closes the menu
    /// (`RadialHandler` trigger-click dispatch). Returns `None` when nothing is selected.
    pub fn take_selected_action(&mut self) -> Option<RadialSliceAction> {
        if !self.showing {
            return None;
        }
        let action = self.hovered_slice.take();
        if action.is_some() {
            // Consumed selection closes the wheel, as Vivecraft's trigger-click does.
            self.showing = false;
        }
        action
    }
}

/// 3D Floating & Physical Drum Keyboard handler ported from `KeyboardHandler.java`, `GuiKeyboard.java`, and `PhysicalKeyboard.java`.
#[derive(Debug, Clone)]
pub struct KeyboardHandler {
    pub showing: bool,
    pub physical_mode: bool,
    pub pos_room: Vec3,
    pub rotation_room: Quat,
    pub shift_pressed: bool,
    pub hovered_char: Option<char>,
    pub typed_buffer: String,
    key_latched: [bool; 2],
}

impl Default for KeyboardHandler {
    fn default() -> Self {
        Self {
            showing: false,
            physical_mode: false,
            pos_room: Vec3::new(0.0, 1.0, -1.0),
            rotation_room: Quat::IDENTITY,
            shift_pressed: false,
            hovered_char: None,
            typed_buffer: String::new(),
            key_latched: [false; 2],
        }
    }
}

impl KeyboardHandler {
    pub const ROWS_LOWER: [&'static str; 4] = [
        "1234567890-=",
        "qwertyuiop[]",
        "asdfghjkl;'/",
        "zxcvbnm, .\\`",
    ];

    pub const ROWS_UPPER: [&'static str; 4] = [
        "!@#$%^&*()_+",
        "QWERTYUIOP{}",
        "ASDFGHJKL:\"? ",
        "ZXCVBNM<>?~|",
    ];

    pub fn new() -> Self {
        Self::default()
    }

    /// Orients the 3D keyboard below the floating GUI or in front of the HMD (`KeyboardHandler.orientOverlay`).
    pub fn orient_overlay(&mut self, room_pre: &VrData, gui_pos_room: Vec3, gui_rot_room: Quat, gui_scale: f32, gui_relative: bool) {
        if self.physical_mode {
            let pos = room_pre.hmd.pos_room;
            let yaw = room_pre.hmd.get_yaw_rad();
            let offset = Quat::from_rotation_y(-yaw) * Vec3::new(0.0, -0.5, -0.35);
            self.pos_room = pos + offset;
            self.rotation_room = Quat::from_rotation_y(std::f32::consts::PI - yaw)
                * Quat::from_rotation_x(-30.0_f32.to_radians());
        } else if gui_relative {
            let gui_up = (gui_rot_room * Vec3::Y) * (0.45 * gui_scale);
            let gui_fwd = (gui_rot_room * Vec3::NEG_Z) * (-0.18 * gui_scale);
            self.pos_room = gui_pos_room - gui_up + gui_fwd;
            self.rotation_room = gui_rot_room * Quat::from_rotation_x(-25.0_f32.to_radians());
        } else {
            let hmd_pos = room_pre.hmd.pos_room;
            let look = room_pre.hmd.get_custom_vector(Vec3::new(0.0, -0.25, -1.0));
            self.pos_room = hmd_pos + look;
            let yaw = (-look.x).atan2(-look.z);
            self.rotation_room =
                Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-20.0_f32.to_radians());
        }
    }

    /// Resolves the character at normalized UV coordinates (`0.0..=1.0`) on the virtual keyboard quad.
    pub fn char_at_uv(&self, uv: Vec2) -> Option<char> {
        if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) {
            return None;
        }
        let rows = if self.shift_pressed {
            &Self::ROWS_UPPER
        } else {
            &Self::ROWS_LOWER
        };
        let row_idx = ((uv.y * 4.0).floor() as usize).min(3);
        let row = rows[row_idx];
        let chars: Vec<char> = row.chars().collect();
        let col_idx = ((uv.x * (chars.len() as f32)).floor() as usize).min(chars.len().saturating_sub(1));
        chars.get(col_idx).copied()
    }

    /// Processes physical drum-style touch typing (`PhysicalKeyboard.process`) when a controller tip enters the keyboard plane.
    pub fn update_physical_touch(&mut self, room_pre: &VrData) -> Option<(char, HapticPulse)> {
        if !self.showing {
            return None;
        }
        let inv_rot = self.rotation_room.inverse();
        for c in 0..2 {
            let tip = room_pre.get_controller(c).pos_room + room_pre.get_controller(c).dir_room * 0.08;
            let local = inv_rot * (tip - self.pos_room);
            let inside = local.x.abs() <= 0.42 && local.y.abs() <= 0.16 && local.z.abs() <= 0.045;
            if inside && !self.key_latched[c] {
                self.key_latched[c] = true;
                let u = ((local.x + 0.42) / 0.84).clamp(0.0, 1.0);
                let v = (1.0 - (local.y + 0.16) / 0.32).clamp(0.0, 1.0);
                if let Some(ch) = self.char_at_uv(Vec2::new(u, v)) {
                    self.typed_buffer.push(ch);
                    self.hovered_char = Some(ch);
                    return Some((
                        ch,
                        HapticPulse {
                            controller: c,
                            duration_micros: 1_200,
                            frequency_hz: 180.0,
                            amplitude: 0.65,
                        },
                    ));
                }
            } else if !inside {
                self.key_latched[c] = false;
            }
        }
        None
    }
}

/// Touch-sensitive 3D Hotbar interaction module ported from `InteractiveHotbarModule.java`.
#[derive(Debug, Clone)]
pub struct InteractiveHotbarModule {
    /// Currently hovered hotbar slot (`0..=8` for main bar, `9` for offhand swap, `-1` if inactive).
    pub hotbar_slot: i32,
    previous_hotbar_slot: i32,
}

impl Default for InteractiveHotbarModule {
    fn default() -> Self {
        Self {
            hotbar_slot: -1,
            previous_hotbar_slot: -1,
        }
    }
}

impl InteractiveHotbarModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates whether the main-hand controller is touching the 3D HUD hotbar (`InteractiveHotbarModule.isActive`).
    /// Returns `(slot, haptic)` when a NEW slot is touched (edge trigger); the caller applies
    /// `slot < 9` as the selected hotbar slot and `9` as the offhand swap.
    pub fn update(
        &mut self,
        room_pre: &VrData,
        gui_pos_room: Vec3,
        gui_rot_room: Quat,
        gui_scale: f32,
        reverse_hands: bool,
        enabled: bool,
    ) -> Option<(i32, HapticPulse)> {
        self.previous_hotbar_slot = self.hotbar_slot;
        self.hotbar_slot = -1;
        if !enabled {
            return None;
        }

        let scale = (gui_scale * 0.65).clamp(0.35, 2.0);
        let bar_start = gui_pos_room + gui_rot_room * Vec3::new(-0.32 * scale, -0.38 * scale, 0.0);
        let bar_end = bar_start + (gui_rot_room * Vec3::X) * (0.64 * scale);

        let main = room_pre.c0.pos_room;
        let bar_line = bar_start - bar_end;
        let bar_len = bar_line.length();
        if bar_len <= 1.0e-5 {
            return None;
        }
        let hand_to_bar = bar_start - main;
        let dist = hand_to_bar.cross(bar_line).length() / bar_len;
        if dist > 0.06 {
            return None;
        }

        let fact = hand_to_bar.dot(bar_line) / bar_line.length_squared();
        if fact < -1.0 {
            return None;
        }

        let point = main - (bar_line * fact - hand_to_bar);
        let mut ilen = bar_start.distance(point);
        if fact < 0.0 {
            ilen = -ilen;
        }
        let pos = (ilen / bar_len) * 9.0;
        let mut slot = pos.floor() as i32;

        if slot > 8 {
            if reverse_hands && (9.5..=10.5).contains(&pos) {
                slot = 9;
            } else {
                return None;
            }
        } else if slot < 0 {
            if !reverse_hands && (-1.5..=-0.5).contains(&pos) {
                slot = 9;
            } else {
                return None;
            }
        }

        self.hotbar_slot = slot;
        if self.hotbar_slot != self.previous_hotbar_slot && self.hotbar_slot >= 0 {
            Some((
                self.hotbar_slot,
                HapticPulse {
                    controller: 0,
                    duration_micros: 750,
                    frequency_hz: 160.0,
                    amplitude: 0.5,
                },
            ))
        } else {
            None
        }
    }
}

/// 3D Floating GUI, Radial Menu, 3D Keyboard, Interactive Hotbar, and Wrist HUD state
/// ported from Vivecraft's `GuiHandler.java`, `RadialHandler.java`, and `KeyboardHandler.java`.
#[derive(Debug, Clone)]
pub struct GuiHandler {
    /// Whether a 2D menu / inventory / pause screen is currently open in floating 3D VR space.
    pub gui_open: bool,
    /// Whether the 8-slice VR Radial menu overlay is open (`RadialHandler.isShowing()`).
    pub radial_open: bool,
    /// Whether the 3D VR Keyboard overlay is open (`KeyboardHandler.SHOWING`).
    pub keyboard_open: bool,
    /// Alias kept for compatibility with `KeyboardHandler.SHOWING`.
    pub keyboard_showing: bool,
    /// Room-space position of the floating 3D GUI quad center.
    pub gui_pos_room: Vec3,
    /// Room-space orientation of the floating 3D GUI quad.
    pub gui_rotation_room: Quat,
    /// Base width scale of the floating GUI quad in meters (`1.5` default, `2.0` on larger screens).
    pub gui_scale: f32,
    /// Aspect ratio (`width / height`) of the virtual GUI surface (`16:9` default).
    pub aspect_ratio: f32,
    /// Smoothed normalized UV coordinates (`0.0..=1.0`) of the controller laser on the GUI quad.
    pub cursor_uv: Vec2,
    /// Whether the controller ray currently intersects the floating GUI quad.
    pub cursor_on_gui: bool,
    /// World-space intersection point of the laser on the GUI quad (or laser endpoint if miss).
    pub laser_hit_world: Vec3,
    /// Whether the wrist HUD is currently glanced at by the HMD.
    pub wrist_hud_active: bool,
    /// 8-slice radial menu state (`RadialHandler.java`).
    pub radial: RadialHandler,
    /// 3D floating & physical drum keyboard state (`KeyboardHandler.java`).
    pub keyboard: KeyboardHandler,
    /// Touch-sensitive 3D hotbar selector (`InteractiveHotbarModule.java`).
    pub hotbar_module: InteractiveHotbarModule,
}

impl Default for GuiHandler {
    fn default() -> Self {
        Self {
            gui_open: false,
            radial_open: false,
            keyboard_open: false,
            keyboard_showing: false,
            gui_pos_room: Vec3::new(0.0, 1.3, -1.2),
            gui_rotation_room: Quat::IDENTITY,
            gui_scale: 1.5,
            aspect_ratio: 16.0 / 9.0,
            cursor_uv: Vec2::splat(-1.0),
            cursor_on_gui: false,
            laser_hit_world: Vec3::ZERO,
            wrist_hud_active: false,
            radial: RadialHandler::new(),
            keyboard: KeyboardHandler::new(),
            hotbar_module: InteractiveHotbarModule::new(),
        }
    }
}

impl GuiHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Anchors the floating 3D GUI plane in front of the HMD in room space
    /// (ports `GuiHandler.onScreenChanged` from Vivecraft).
    pub fn open_gui(&mut self, room_pre: &VrData, hud_distance: f32, hud_scale: f32) {
        self.gui_open = true;
        self.gui_scale = 1.5 * hud_scale.clamp(0.5, 2.5);
        let hmd_pos = room_pre.hmd.pos_room;
        let forward = room_pre.hmd.dir_room;
        // Flatten pitch slightly so the screen stays comfortable to read
        let look = Vec3::new(forward.x, (forward.y * 0.5).clamp(-0.4, 0.4), forward.z)
            .try_normalize()
            .unwrap_or(Vec3::NEG_Z);
        let dist = hud_distance.clamp(0.6, 3.0);
        self.gui_pos_room = hmd_pos + look * dist;

        let yaw = (-look.x).atan2(-look.z);
        let pitch = look.y.clamp(-1.0, 1.0).asin();
        self.gui_rotation_room =
            Quat::from_axis_angle(Vec3::Y, yaw) * Quat::from_axis_angle(Vec3::X, pitch);

        if self.radial_open {
            self.radial.showing = true;
            self.radial.orient_overlay(room_pre, 0, false);
        }
        if self.keyboard_open || self.keyboard_showing {
            self.keyboard_open = true;
            self.keyboard_showing = true;
            self.keyboard.showing = true;
            self.keyboard.orient_overlay(
                room_pre,
                self.gui_pos_room,
                self.gui_rotation_room,
                self.gui_scale,
                true,
            );
        }
    }

    /// Convenience helper using default HUD distance (`1.25m`) and scale (`1.0`).
    pub fn open_gui_default(&mut self, room_pre: &VrData) {
        self.open_gui(room_pre, 1.25, 1.0);
    }

    pub fn close_gui(&mut self) {
        self.gui_open = false;
        self.radial_open = false;
        self.keyboard_open = false;
        self.keyboard_showing = false;
        self.radial.showing = false;
        self.keyboard.showing = false;
        self.cursor_on_gui = false;
        self.cursor_uv = Vec2::splat(-1.0);
    }

    /// Toggles the floating 3D GUI plane in front of the HMD.
    pub fn toggle_gui(&mut self, room_pre: &VrData, hud_distance: f32, hud_scale: f32) {
        if self.gui_open {
            self.close_gui();
        } else {
            self.open_gui(room_pre, hud_distance, hud_scale);
        }
    }

    /// Ports `GuiHandler.getTexCoordsForCursor` exactly:
    /// computes the intersection of a controller ray (`controller_pos`, `controller_dir`)
    /// with the 3D GUI plane (`gui_pos`, `gui_rot`, `gui_scale`, `aspect_ratio`).
    pub fn get_tex_coords_for_cursor(
        gui_pos: Vec3,
        gui_rot: Quat,
        gui_scale: f32,
        aspect_ratio: f32,
        controller_pos: Vec3,
        controller_dir: Vec3,
    ) -> Option<(Vec2, Vec3)> {
        let dir = controller_dir.try_normalize()?;
        let gui_normal = (gui_rot * Vec3::NEG_Z).normalize_or_zero();
        let gui_right = (gui_rot * Vec3::X).normalize_or_zero();
        let gui_up = (gui_rot * Vec3::Y).normalize_or_zero();

        let denom = gui_normal.dot(dir);
        // Must point towards the front or back of the GUI quad and not be parallel
        if denom.abs() <= 1.0e-5 {
            return None;
        }

        let t = gui_normal.dot(gui_pos - controller_pos) / denom;
        if !(0.0..=6.0).contains(&t) {
            return None;
        }

        let hit_pos = controller_pos + dir * t;
        let relative = hit_pos - gui_pos;

        let width = gui_scale.max(0.2);
        let height = (width / aspect_ratio.max(0.25)).max(0.2);
        let half_w = width * 0.5;
        let half_h = height * 0.5;

        let local_x = relative.dot(gui_right);
        let local_y = relative.dot(gui_up);

        let u = 0.5 + local_x / (2.0 * half_w);
        let v = 0.5 - local_y / (2.0 * half_h);

        if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
            Some((Vec2::new(u, v), hit_pos))
        } else {
            None
        }
    }

    /// Updates the floating GUI cursor UV (with Vivecraft's `0.7 / 0.3` cursor smoothing),
    /// updates `RadialHandler` and `KeyboardHandler`, and checks whether the player is looking at the off-hand wrist HUD.
    pub fn update(&mut self, world_post: &VrData, room_pre: &VrData) {
        self.keyboard_showing = self.keyboard_open;
        self.radial.showing = self.radial_open;
        self.keyboard.showing = self.keyboard_open;

        // Check wrist HUD glance (angle between HMD forward and offhand -> HMD vector)
        let hmd_pos = world_post.hmd.get_position_f();
        let hmd_dir = world_post.hmd.get_direction();
        let wrist_pos = world_post.c1.get_position_f();
        let to_wrist = (wrist_pos - hmd_pos).try_normalize().unwrap_or(Vec3::NEG_Z);
        let wrist_up = world_post.c1.get_custom_vector(Vec3::Y);
        let looking_at_wrist = hmd_dir.dot(to_wrist) > 0.72 && wrist_up.dot(-hmd_dir) > 0.25;
        self.wrist_hud_active = looking_at_wrist && hmd_pos.distance(wrist_pos) < 0.75;

        if !self.gui_open {
            self.cursor_on_gui = false;
            self.radial.hovered_slice = None;
            self.keyboard.hovered_char = None;
            return;
        }

        let ctrl_pos_room = room_pre.c0.pos_room;
        let ctrl_dir_room = room_pre.c0.dir_room;

        if let Some((raw_uv, hit_room)) = Self::get_tex_coords_for_cursor(
            self.gui_pos_room,
            self.gui_rotation_room,
            self.gui_scale,
            self.aspect_ratio,
            ctrl_pos_room,
            ctrl_dir_room,
        ) {
            self.cursor_on_gui = true;
            if self.cursor_uv.x < 0.0 || self.cursor_uv.y < 0.0 {
                self.cursor_uv = raw_uv;
            } else {
                // Vivecraft's cursor smoothing filter: old * 0.7 + new * 0.3
                self.cursor_uv = self.cursor_uv * 0.7 + raw_uv * 0.3;
            }
            self.laser_hit_world = Self::room_point_to_world(hit_room, world_post);
        } else {
            self.cursor_on_gui = false;
            let end_room = ctrl_pos_room + ctrl_dir_room * 2.5;
            self.laser_hit_world = Self::room_point_to_world(end_room, world_post);
        }

        if self.radial_open {
            self.radial
                .update_slice_from_uv(self.cursor_uv, self.cursor_on_gui);
        }
        if self.keyboard_open {
            self.keyboard.hovered_char = if self.cursor_on_gui {
                self.keyboard.char_at_uv(self.cursor_uv)
            } else {
                None
            };
            let _ = self.keyboard.update_physical_touch(room_pre);
        }
    }

    /// Returns the 4 world-space corners `[top_left, top_right, bottom_right, bottom_left]`
    /// of the floating 3D GUI quad for rendering in `native_world_primitives`.
    pub fn gui_quad_corners_world(&self, world_post: &VrData) -> [Vec3; 4] {
        let width = self.gui_scale.max(0.2);
        let height = (width / self.aspect_ratio.max(0.25)).max(0.2);
        let hw = width * 0.5;
        let hh = height * 0.5;

        let right = self.gui_rotation_room * Vec3::X;
        let up = self.gui_rotation_room * Vec3::Y;

        let tl_room = self.gui_pos_room - right * hw + up * hh;
        let tr_room = self.gui_pos_room + right * hw + up * hh;
        let br_room = self.gui_pos_room + right * hw - up * hh;
        let bl_room = self.gui_pos_room - right * hw - up * hh;

        [
            Self::room_point_to_world(tl_room, world_post),
            Self::room_point_to_world(tr_room, world_post),
            Self::room_point_to_world(br_room, world_post),
            Self::room_point_to_world(bl_room, world_post),
        ]
    }

    /// Returns the 4 world-space corners of the off-hand wrist HUD bar.
    pub fn wrist_hud_corners_world(&self, world_post: &VrData) -> [Vec3; 4] {
        let scale = world_post.world_scale.max(0.1);
        let ctrl = &world_post.c1;
        let forward = ctrl.get_direction();
        let up = ctrl.get_custom_vector(Vec3::Y);
        let right = forward.cross(up).try_normalize().unwrap_or(Vec3::X);

        let center = ctrl.get_position_f() - forward * (0.09 * scale) + up * (0.045 * scale);
        let hw = 0.065 * scale;
        let hh = 0.024 * scale;

        [
            center - right * hw + forward * hh,
            center + right * hw + forward * hh,
            center + right * hw - forward * hh,
            center - right * hw - forward * hh,
        ]
    }

    pub fn room_point_to_world(room_point: Vec3, world_post: &VrData) -> Vec3 {
        let rot = Mat4::from_rotation_y(world_post.rotation_radians);
        let scaled = room_point * world_post.world_scale;
        let origin_f = Vec3::new(
            world_post.origin.x as f32,
            world_post.origin.y as f32,
            world_post.origin.z as f32,
        );
        origin_f + rot.transform_point3(scaled)
    }
}
