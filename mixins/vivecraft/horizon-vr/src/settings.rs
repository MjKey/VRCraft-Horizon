//! VR configuration and settings management (ported from `VRSettings.java`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocomotionMode {
    Smooth,
    Teleport,
}

impl LocomotionMode {
    pub fn next(self) -> Self {
        match self {
            Self::Smooth => Self::Teleport,
            Self::Teleport => Self::Smooth,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth (Free Move)",
            Self::Teleport => "Teleport Arc",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreeMoveOrientation {
    Controller,
    Hmd,
    RunInPlace,
    Waist,
}

impl FreeMoveOrientation {
    pub fn next(self) -> Self {
        match self {
            Self::Controller => Self::Hmd,
            Self::Hmd => Self::RunInPlace,
            Self::RunInPlace => Self::Waist,
            Self::Waist => Self::Controller,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Controller => "Controller",
            Self::Hmd => "HMD (Head)",
            Self::RunInPlace => "Run-In-Place (Arm Swing)",
            Self::Waist => "Waist (FBT)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnMode {
    Snap,
    Smooth,
}

impl TurnMode {
    pub fn next(self) -> Self {
        match self {
            Self::Snap => Self::Smooth,
            Self::Smooth => Self::Snap,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Snap => "Snap Turn",
            Self::Smooth => "Smooth Turn",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BowMode {
    TwoHanded,
    Vanilla,
    Off,
}

impl BowMode {
    pub fn next(self) -> Self {
        match self {
            Self::TwoHanded => Self::Vanilla,
            Self::Vanilla => Self::Off,
            Self::Off => Self::TwoHanded,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TwoHanded => "Roomscale 2-Handed",
            Self::Vanilla => "Vanilla",
            Self::Off => "OFF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponCollision {
    Auto,
    On,
    Off,
}

impl WeaponCollision {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Survival)",
            Self::On => "Always ON",
            Self::Off => "OFF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AimDevice {
    Controller,
    Hmd,
}

impl AimDevice {
    pub fn next(self) -> Self {
        match self {
            Self::Controller => Self::Hmd,
            Self::Hmd => Self::Controller,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Controller => "Controller (6DoF)",
            Self::Hmd => "Head Crosshair",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VrSettings {
    /// Master VR active switch.
    pub vr_enabled: bool,
    /// True when playing seated (disables physical crouch/swing/teleport-arc hand offset).
    pub seated: bool,
    /// Left-handed mode (swaps main hand and offhand controllers).
    pub reverse_hands: bool,
    /// Reversed bow shooting hand.
    pub reverse_shooting_eye: bool,
    /// Stereo rendering (two views in `ViewSet`) vs mono HMD view.
    pub stereo_rendering: bool,
    /// World scale factor (`0.5` .. `2.0`).
    pub world_scale: f32,
    /// Physical room walk multiplier (`1.0` .. `2.0`).
    pub walk_multiplier: f32,
    /// Calibrated physical player height in meters (default `1.62`).
    pub calibrated_height: f32,
    /// Interpupillary distance (IPD) in meters for simulated / fallback stereo.
    pub ipd: f32,
    /// Primary locomotion mode.
    pub locomotion_mode: LocomotionMode,
    /// Direction reference for smooth locomotion.
    pub free_move_orientation: FreeMoveOrientation,
    /// Rotation mode (Snap vs Smooth).
    pub turn_mode: TurnMode,
    /// Snap turn step in degrees (`15`, `30`, `45`, `90`).
    pub snap_turn_degrees: f32,
    /// Smooth turn speed in degrees per second.
    pub smooth_turn_rate: f32,
    /// Roomscale bow mode.
    pub bow_mode: BowMode,
    /// Physical swing weapon/tool collision mode.
    pub weapon_collision: WeaponCollision,
    /// Device used for primary raycast aiming.
    pub aim_device: AimDevice,
    /// 3D Floating GUI scale (`0.5` .. `2.0`).
    pub hud_scale: f32,
    /// 3D Floating GUI distance in meters (`0.8` .. `2.5`).
    pub hud_distance: f32,
    /// Render 3D laser pointer & teleport arc in world space.
    pub render_vr_guides: bool,
    /// Realistic physical jump (`JumpTracker.java`).
    pub realistic_jump_enabled: bool,
    /// Realistic physical crouch (`SneakTracker.java`).
    pub realistic_sneak_enabled: bool,
    /// Realistic ladder/vine & Climbey climbing (`ClimbTracker.java`).
    pub realistic_climb_enabled: bool,
    /// Auto-grab ladders when touching them (`climbingAutoGrab`).
    pub climbing_auto_grab: bool,
    /// Realistic breaststroke swimming (`SwimTracker.java`).
    pub realistic_swim_enabled: bool,
    /// Realistic two-oar boat rowing (`RowTracker.java`).
    pub realistic_row_enabled: bool,
    /// Over-the-shoulder backpack slot switching (`BackpackTracker.java`).
    pub backpack_switching: bool,
    /// Physical drum-style touch keyboard (`KeyboardHandler.java`).
    pub physical_keyboard: bool,
    /// Touch-sensitive 3D hotbar selection (`InteractiveHotbarModule.java`).
    pub vr_touch_hotbar: bool,
}

impl Default for VrSettings {
    fn default() -> Self {
        Self {
            vr_enabled: true,
            seated: false,
            reverse_hands: false,
            reverse_shooting_eye: false,
            stereo_rendering: true,
            world_scale: 1.0,
            walk_multiplier: 1.0,
            calibrated_height: 1.62,
            ipd: 0.064,
            locomotion_mode: LocomotionMode::Smooth,
            free_move_orientation: FreeMoveOrientation::Controller,
            turn_mode: TurnMode::Snap,
            snap_turn_degrees: 45.0,
            smooth_turn_rate: 120.0,
            bow_mode: BowMode::TwoHanded,
            weapon_collision: WeaponCollision::Auto,
            aim_device: AimDevice::Controller,
            hud_scale: 1.0,
            hud_distance: 1.25,
            render_vr_guides: true,
            realistic_jump_enabled: true,
            realistic_sneak_enabled: true,
            realistic_climb_enabled: true,
            climbing_auto_grab: true,
            realistic_swim_enabled: true,
            realistic_row_enabled: true,
            backpack_switching: true,
            physical_keyboard: false,
            vr_touch_hotbar: true,
        }
    }
}

impl VrSettings {
    /// Returns the list of interactive button labels for Horizon's `Options > Mods > ViveCraft` screen.
    pub fn settings_buttons(&self, provider_name: &str) -> Vec<String> {
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("VR Mode ({provider_name}): {}", on_off(self.vr_enabled)),
            format!(
                "Play Mode: {}",
                if self.seated { "Seated" } else { "Standing (Roomscale)" }
            ),
            format!("Stereo Rendering: {}", on_off(self.stereo_rendering)),
            format!("Locomotion: {}", self.locomotion_mode.label()),
            format!("Move Direction: {}", self.free_move_orientation.label()),
            format!("Turning: {}", self.turn_mode.label()),
            format!("Snap Turn Angle: {:.0}°", self.snap_turn_degrees),
            format!("Dominant Hand: {}", if self.reverse_hands { "Left" } else { "Right" }),
            format!("World Scale: {:.2}x", self.world_scale),
            format!("Bow Mode: {}", self.bow_mode.label()),
            format!("Weapon Swing Collision: {}", self.weapon_collision.label()),
            format!("Aim Device: {}", self.aim_device.label()),
            format!("Floating 3D HUD Scale: {:.0}%", self.hud_scale * 100.0),
            format!("3D Laser & Arc Guides: {}", on_off(self.render_vr_guides)),
        ]
    }

    /// Applies a click on button `index` in Horizon's `Options > Mods > ViveCraft` screen.
    pub fn click_button(&mut self, index: usize) {
        match index {
            0 => self.vr_enabled = !self.vr_enabled,
            1 => self.seated = !self.seated,
            2 => self.stereo_rendering = !self.stereo_rendering,
            3 => self.locomotion_mode = self.locomotion_mode.next(),
            4 => self.free_move_orientation = self.free_move_orientation.next(),
            5 => self.turn_mode = self.turn_mode.next(),
            6 => {
                self.snap_turn_degrees = match self.snap_turn_degrees as i32 {
                    15 => 30.0,
                    30 => 45.0,
                    45 => 90.0,
                    _ => 15.0,
                };
            }
            7 => self.reverse_hands = !self.reverse_hands,
            8 => {
                const SCALES: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
                let pos = SCALES
                    .iter()
                    .position(|&s| (s - self.world_scale).abs() < 0.05)
                    .unwrap_or(2);
                self.world_scale = SCALES[(pos + 1) % SCALES.len()];
            }
            9 => self.bow_mode = self.bow_mode.next(),
            10 => self.weapon_collision = self.weapon_collision.next(),
            11 => self.aim_device = self.aim_device.next(),
            12 => {
                const HUD_SCALES: [f32; 4] = [0.75, 1.0, 1.25, 1.5];
                let pos = HUD_SCALES
                    .iter()
                    .position(|&s| (s - self.hud_scale).abs() < 0.05)
                    .unwrap_or(1);
                self.hud_scale = HUD_SCALES[(pos + 1) % HUD_SCALES.len()];
            }
            13 => self.render_vr_guides = !self.render_vr_guides,
            _ => {}
        }
    }
}
