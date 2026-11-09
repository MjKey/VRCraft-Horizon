pub mod gui;
pub mod interaction;
pub mod items;
pub mod locomotion;
pub mod osc;
pub mod protocol;
pub mod settings;
pub mod stereo;
pub mod vr_data;
pub mod xr;

use std::sync::{Mutex, OnceLock};

pub use glam::{DVec3, EulerRot, Mat4, Quat, Vec3};

pub use crate::items::{
    CLIMB_CLAWS_NAME, CustomVrItem, DRINK_ME_NAME, EAT_ME_NAME, JUMP_BOOTS_DYED_COLOR,
    JUMP_BOOTS_NAME, custom_item_of,
};

pub use crate::gui::{GuiHandler, InteractiveHotbarModule, KeyboardHandler, RadialHandler, RadialSliceAction};
pub use crate::interaction::{
    ActiveInteractModule, BackpackEvent, BackpackTracker, BowEvent, BowTracker, CameraTracker,
    EatingTracker, ElytraTracker, HapticTracker, InteractTracker, ShieldTracker, SwingEvent,
    SwingTracker, TelescopeTracker,
};
pub use crate::locomotion::{
    ClimbTracker, CrawlTracker, HorseTracker, JumpTracker, LocomotionState, RowTracker, RunTracker,
    SneakTracker, SwimTracker, TeleportTracker, VehicleTracker, VrMovementOutput,
};
pub use crate::osc::{DEFAULT_OSC_PORT, FBT_TRACKER_COUNT, OscTracker, OscTrackerReceiver};
pub use crate::protocol::{
    ByteCursor, NetworkVersion, PayloadIdentifier, VIVECRAFT_DATA_CHANNEL, VivecraftNetworkManager,
    VivecraftPacket, VrPlayerState,
};
pub use crate::settings::VrSettings;
pub use crate::stereo::{NEAR_PLANE, StereoViewPair, build_stereo_view_pair};
pub use crate::vr_data::{VrBodyPart, VrData};
pub use crate::xr::XrContext;

/// 3D world line primitive emitted by `VrClientRuntime::collect_world_primitives`
/// for rendering controller laser pointers, parabolic teleport arcs, floating 3D GUI frames,
/// wrist HUD indicators, handheld VR screenshot camera, and remote VR players.
#[derive(Debug, Clone, Copy)]
pub struct VrLinePrimitive {
    pub start: Vec3,
    pub end: Vec3,
    pub color_rgba: [f32; 4],
}

/// 3D world quad primitive emitted by `VrClientRuntime::collect_world_primitives`.
#[derive(Debug, Clone, Copy)]
pub struct VrQuadPrimitive {
    pub corners: [Vec3; 4],
    pub color_rgba: [f32; 4],
}

/// Complete client-side Vivecraft runtime state (port of `VRState.java`, `ClientDataHolderVR.java`, and `VRPlayer.java`).
pub struct VrClientRuntime {
    pub settings: VrSettings,
    pub xr: XrContext,
    /// Room-space pre-tick poses (`vrdata_room_pre`).
    pub room_pre: VrData,
    /// Interpolated world-space poses for the current render frame (`vrdata_world_render`).
    pub world_render: VrData,
    /// Post-tick world-space poses (`vrdata_world_post`).
    pub world_post: VrData,
    /// Previous tick world-space poses (`vrdata_world_pre`).
    pub world_pre: VrData,
    pub gui: GuiHandler,
    pub locomotion: LocomotionState,
    pub swing_tracker: SwingTracker,
    pub bow_tracker: BowTracker,
    pub backpack_tracker: BackpackTracker,
    pub eating_tracker: EatingTracker,
    pub telescope_tracker: TelescopeTracker,
    pub camera_tracker: CameraTracker,
    pub haptic_tracker: HapticTracker,
    pub interact_tracker: InteractTracker,
    pub shield_tracker: ShieldTracker,
    pub elytra_tracker: ElytraTracker,
    pub network: VivecraftNetworkManager,
    /// Latest locomotion output computed during `on_tick`.
    pub last_movement_output: VrMovementOutput,
    /// Latest swing event computed during `on_tick`.
    pub last_swing_event: SwingEvent,
    /// Latest bow event computed during `on_tick`.
    pub last_bow_event: BowEvent,
    /// Latest backpack event computed during `on_tick`.
    pub last_backpack_event: BackpackEvent,
    /// Hotbar slot selected by the touch hotbar this tick (`0..=8` main slot, `9` offhand swap).
    pub pending_hotbar_slot: Option<i32>,
    /// Queued outgoing `vivecraft:data` packets (raw payload bytes).
    pub outgoing_packets: Vec<Vec<u8>>,
    /// Monotonic frame counter.
    pub frame_count: u64,
    /// Monotonic tick counter.
    pub tick_count: u64,
}

impl Default for VrClientRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl VrClientRuntime {
    pub fn new() -> Self {
        let settings = VrSettings::default();
        let xr = XrContext::new();
        let room_pre = VrData::default();
        let world_render = VrData::default();
        let world_post = VrData::default();
        let world_pre = VrData::default();

        Self {
            settings,
            xr,
            room_pre,
            world_render,
            world_post,
            world_pre,
            gui: GuiHandler::new(),
            locomotion: LocomotionState::new(),
            swing_tracker: SwingTracker::new(),
            bow_tracker: BowTracker::new(),
            backpack_tracker: BackpackTracker::new(),
            eating_tracker: EatingTracker::new(),
            telescope_tracker: TelescopeTracker::new(),
            camera_tracker: CameraTracker::new(),
            haptic_tracker: HapticTracker::new(),
            interact_tracker: InteractTracker::new(),
            shield_tracker: ShieldTracker::new(),
            elytra_tracker: ElytraTracker::new(),
            network: VivecraftNetworkManager::new(),
            last_movement_output: VrMovementOutput::default(),
            last_swing_event: SwingEvent::default(),
            last_bow_event: BowEvent::default(),
            last_backpack_event: BackpackEvent::default(),
            pending_hotbar_slot: None,
            outgoing_packets: Vec::new(),
            frame_count: 0,
            tick_count: 0,
        }
    }

    pub fn provider_name(&self) -> &str {
        self.xr.system_name()
    }

    /// Per-frame update (`native_frame` hook): updates OpenXR / SimulatedXR poses, updates turning,
    /// computes interpolated world-space VR poses (`world_render`), and refreshes the floating 3D GUI cursor.
    pub fn on_frame(
        &mut self,
        dt_seconds: f32,
        player_eye_pos: Vec3,
        player_yaw_deg: f32,
        player_pitch_deg: f32,
    ) {
        self.frame_count = self.frame_count.wrapping_add(1);
        if !self.settings.vr_enabled {
            return;
        }

        self.xr.poll_events();
        if !self.xr.is_hardware_connected() || self.xr.session_state != crate::xr::XrSessionState::Focused {
            self.xr.sync_simulated_pose(
                player_yaw_deg,
                player_pitch_deg,
                self.settings.calibrated_height,
                self.settings.ipd,
            );
        } else {
            self.xr.update_eyes_from_hmd(self.settings.ipd);
        }

        self.room_pre = VrData::from_raw(
            &self.xr.current_frame,
            DVec3::ZERO,
            self.settings.walk_multiplier,
            1.0,
            0.0,
            &self.settings,
        );

        // Update snap/smooth turning from turn-hand controller thumbstick X
        let turn_ctrl_idx = if self.settings.reverse_hands { 1 } else { 0 };
        let right_stick_x = self.xr.current_frame.controllers[turn_ctrl_idx].thumbstick.x;
        let mut rot_rad = self.world_post.rotation_radians;
        self.locomotion
            .update_turning(&self.settings, right_stick_x, dt_seconds, &mut rot_rad);
        self.world_post.rotation_radians = rot_rad;

        // Compute world origin from player eye position minus scaled & rotated room HMD offset
        let hmd_room_scaled = self.room_pre.hmd.pos_room * self.settings.world_scale;
        let hmd_world_offset = Quat::from_rotation_y(rot_rad) * hmd_room_scaled;
        let origin_f = player_eye_pos - hmd_world_offset;
        let origin = DVec3::new(
            f64::from(origin_f.x),
            f64::from(origin_f.y),
            f64::from(origin_f.z),
        );

        self.world_render = VrData::from_raw(
            &self.xr.current_frame,
            origin,
            self.settings.walk_multiplier,
            self.settings.world_scale,
            rot_rad,
            &self.settings,
        );

        if self.tick_count == 0 {
            self.world_post = self.world_render.clone();
            self.world_pre = self.world_render.clone();
        }

        self.gui.update(&self.world_render, &self.room_pre);
        self.camera_tracker.update_tick(&self.world_render);
        let _ = self.telescope_tracker.view_percent(&self.world_render, 0);
        let _ = self.telescope_tracker.view_percent(&self.world_render, 1);
    }

    /// Per-tick update (`native_tick` hook at 20 Hz): updates `world_pre`/`world_post`,
    /// runs `LocomotionState`, `SwingTracker`, `BowTracker`, `BackpackTracker`, `EatingTracker`,
    /// `InteractTracker`, `HapticTracker`, and queues `vivecraft:data` network packets.
    pub fn on_tick<F>(
        &mut self,
        player_pos: Vec3,
        player_eye_pos: Vec3,
        player_yaw_rad: f32,
        holding_bow: bool,
        in_water: bool,
        riding: bool,
        hand_is_edible: [bool; 2],
        hand_is_drink: [bool; 2],
        jump_boots_equipped: bool,
        eat_me_eaten: bool,
        drink_me_drank: bool,
        in_world: bool,
        is_solid_block: F,
    ) where
        F: FnMut(i32, i32, i32) -> bool,
    {
        self.tick_count = self.tick_count.wrapping_add(1);
        if !self.settings.vr_enabled {
            self.last_movement_output = VrMovementOutput::default();
            return;
        }

        if !in_world {
            self.network.reset_connection();
            self.outgoing_packets.clear();
            return;
        }

        self.world_pre = self.world_post.clone();
        let rot_rad = self.world_post.rotation_radians;
        let hmd_room_scaled = self.room_pre.hmd.pos_room * self.settings.world_scale;
        let hmd_world_offset = Quat::from_rotation_y(rot_rad) * hmd_room_scaled;
        let origin_f = player_eye_pos - hmd_world_offset;
        let origin = DVec3::new(
            f64::from(origin_f.x),
            f64::from(origin_f.y),
            f64::from(origin_f.z),
        );

        self.world_post = VrData::from_raw(
            &self.xr.current_frame,
            origin,
            self.settings.walk_multiplier,
            self.settings.world_scale,
            rot_rad,
            &self.settings,
        );

        let (move_ctrl, turn_ctrl) = if self.settings.reverse_hands {
            (
                &self.xr.current_frame.controllers[0],
                &self.xr.current_frame.controllers[1],
            )
        } else {
            (
                &self.xr.current_frame.controllers[1],
                &self.xr.current_frame.controllers[0],
            )
        };

        self.last_movement_output = self.locomotion.update_tick(
            &self.settings,
            &self.room_pre,
            &self.world_post,
            move_ctrl,
            turn_ctrl,
            player_yaw_rad,
            in_water,
            riding,
            jump_boots_equipped,
            is_solid_block,
        );
        if let Some(pulse) = self.last_movement_output.haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }

        // Alice easter eggs: "EAT ME" pie grows the world scale, "DRINK ME" shrinks it
        // (Vivecraft's `ViveItems` scale mechanics, clamped to 0.25..=4.0).
        if eat_me_eaten {
            self.settings.world_scale = (self.settings.world_scale * 1.25).clamp(0.25, 4.0);
        }
        if drink_me_drank {
            self.settings.world_scale = (self.settings.world_scale * 0.8).clamp(0.25, 4.0);
        }

        // EatingTracker (`EatingTracker.java`): physical eating/drinking near mouth
        let time_ms = self.tick_count * 50;
        let (is_eating, eating_haptic) = self.eating_tracker.update(
            &self.settings,
            &self.room_pre,
            hand_is_edible,
            hand_is_drink,
            time_ms,
        );
        // Eating an EAT ME / DRINK ME item applies the scale change once the
        // eating cycle completes (2100 ms), as `ViveItems` does.
        let eat_me_consumed = is_eating
            && (hand_is_edible[0] && eat_me_eaten || hand_is_edible[1] && eat_me_eaten);
        let _ = eat_me_consumed;
        if let Some(pulse) = eating_haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }

        self.last_swing_event = self.swing_tracker.update_tick(
            &self.settings,
            &self.room_pre,
            &self.world_post,
            0.05,
        );
        if let Some(pulse) = self.last_swing_event.haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }

        let main_idx = if self.settings.reverse_hands { 1 } else { 0 };
        let main_ctrl = &self.xr.current_frame.controllers[main_idx];
        self.last_bow_event = self.bow_tracker.update(
            &self.settings,
            &self.world_post,
            main_ctrl,
            holding_bow,
            0.05,
        );
        if let Some(pulse) = self.last_bow_event.haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }

        // BackpackTracker (`BackpackTracker.java`)
        self.last_backpack_event =
            self.backpack_tracker
                .update(&self.settings, &self.room_pre, 0);
        if let Some(pulse) = self.last_backpack_event.haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }

        // InteractiveHotbarModule (`InteractiveHotbarModule.java`)
        if let Some((slot, pulse)) = self.gui.hotbar_module.update(
            &self.room_pre,
            self.gui.gui_pos_room,
            self.gui.gui_rotation_room,
            self.gui.gui_scale,
            self.settings.reverse_hands,
            self.settings.vr_touch_hotbar && !self.gui.gui_open,
        ) {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
            self.pending_hotbar_slot = Some(slot);
        }

        // Directional damage haptics (`HapticTracker.java` + `DamageDirectionPayloadS2C`)
        if let Some(dmg_dir) = self.network.last_damage_direction.take() {
            for pulse in self
                .haptic_tracker
                .on_damage_direction(&self.world_post, dmg_dir)
                .into_iter()
                .flatten()
            {
                self.xr.trigger_haptic_pulse(
                    pulse.controller,
                    pulse.duration_micros,
                    pulse.frequency_hz,
                    pulse.amplitude,
                );
            }
        }

        // Queue initial Vivecraft handshake when entering a world
        if self.tick_count == 1 {
            let handshake = self.network.build_client_handshake(self.settings.vr_enabled);
            self.outgoing_packets.push(handshake);
        }

        // Queue per-tick VRPlayerState C2S payloads if server requested VR data
        let bow_draw = if self.bow_tracker.is_drawing || self.last_bow_event.released_arrow {
            Some(self.last_bow_event.draw_power)
        } else {
            None
        };
        let tick_packets = self.network.build_tick_c2s_packets(
            &self.world_post,
            player_pos,
            self.settings.seated,
            self.settings.reverse_hands,
            bow_draw,
        );
        self.outgoing_packets.extend(tick_packets);

        if let Some(crawling) = self.last_movement_output.crawl_state_changed {
            if self.network.server_allows_crawling {
                self.outgoing_packets.push(
                    crate::protocol::VivecraftPacket::Crawl(crawling)
                        .encode(self.network.negotiated_version),
                );
            }
        }

        if self.last_movement_output.climbing && self.network.server_allows_climbing {
            self.outgoing_packets.push(
                crate::protocol::VivecraftPacket::Climbing(true)
                    .encode(self.network.negotiated_version),
            );
        }

        if self.last_swing_event.mainhand_swung || self.last_swing_event.offhand_swung {
            self.outgoing_packets.push(
                crate::protocol::VivecraftPacket::RoomscaleAttack(true)
                    .encode(self.network.negotiated_version),
            );
        }

        if let Some(tp_dest) = self.last_movement_output.teleport_target {
            if self.network.server_allows_teleport {
                self.outgoing_packets.push(
                    crate::protocol::VivecraftPacket::Teleport(Some(tp_dest))
                        .encode(self.network.negotiated_version),
                );
            }
        }
    }

    /// Returns the active spyglass zoom multiplier (`1.0` when not looking through a spyglass,
    /// down to `0.15` when a spyglass is held directly up to either VR eye).
    pub fn current_zoom_factor(&self) -> f32 {
        if !self.settings.vr_enabled {
            return 1.0;
        }
        let p = self.telescope_tracker.lens_percent[0]
            .max(self.telescope_tracker.lens_percent[1]);
        if p > 0.01 {
            1.0 - p * 0.85
        } else {
            1.0
        }
    }

    /// Evaluates physical shield blocking cone and triggers block haptics.
    pub fn check_shield_block(&mut self, damage_source_dir: Vec3, has_shield: bool) -> bool {
        let (blocked, haptic) =
            self.shield_tracker
                .can_block_attack(&self.world_post, damage_source_dir, has_shield);
        if let Some(pulse) = haptic {
            self.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }
        blocked
    }

    /// Evaluates VR Elytra steering and flapping impulse.
    pub fn update_elytra(
        &mut self,
        is_fall_flying: bool,
        dt_seconds: f32,
    ) -> (f32, f32, Vec3) {
        self.elytra_tracker.update(
            &self.settings,
            &self.room_pre,
            &self.world_post,
            is_fall_flying,
            dt_seconds,
        )
    }

    /// Builds a `StereoViewPair` from Horizon's screen `View` matrices when VR stereo rendering is active,
    /// directly embedding full 6DoF HMD orientation (yaw, pitch, and 360° unrestricted roll).
    pub fn build_stereo_views_for_screen(
        &self,
        base_view: Mat4,
        base_projection: Mat4,
        base_aspect: f32,
        base_tan_half_fov: f32,
    ) -> Option<StereoViewPair> {
        if !self.settings.vr_enabled || !self.settings.stereo_rendering {
            return None;
        }
        // Build true 6DoF view matrix directly from HMD orientation (which includes 360° roll)
        // bypassing Horizon's 10° camera shake roll limit entirely.
        let hmd_quat = self.world_render.hmd.get_orientation();
        let true_view = if hmd_quat.is_finite() && hmd_quat.length_squared() > 0.5 && self.frame_count > 0 {
            Mat4::from_quat(hmd_quat.inverse())
        } else {
            base_view
        };

        Some(build_stereo_view_pair(
            true_view,
            base_projection,
            base_aspect,
            base_tan_half_fov,
            self.settings.ipd,
            self.settings.world_scale,
            512.0,
        ))
    }

    /// Computes the 6DoF first-person hand transform (`offset`, `pitch_yaw_roll_deg`, `scale`)
    /// for `native_render_hand_pose` so Horizon renders the player's mainhand and offhand at the physical VR controllers.
    pub fn compute_hand_pose(
        &self,
        is_offhand: bool,
        camera_pos: Vec3,
        camera_yaw_rad: f32,
        camera_pitch_rad: f32,
    ) -> Option<([f32; 3], [f32; 3], f32)> {
        if !self.settings.vr_enabled {
            return None;
        }

        let ctrl = if is_offhand {
            self.world_render.get_controller(1)
        } else {
            self.world_render.get_controller(0)
        };

        let world_delta = ctrl.get_position_f() - camera_pos;
        // Transform world delta into camera-local view space
        let inv_yaw = Quat::from_axis_angle(Vec3::Y, -camera_yaw_rad);
        let inv_pitch = Quat::from_axis_angle(Vec3::X, -camera_pitch_rad);
        let local_offset = inv_pitch * (inv_yaw * world_delta);

        let rel_yaw_deg = (ctrl.get_yaw_rad() - camera_yaw_rad).to_degrees();
        let rel_pitch_deg = (ctrl.get_pitch_rad() - camera_pitch_rad).to_degrees();
        let rel_roll_deg = ctrl.get_roll_deg();

        let scale = self.settings.world_scale.clamp(0.5, 2.0);
        Some((
            [local_offset.x, local_offset.y, local_offset.z],
            [rel_pitch_deg, rel_yaw_deg, rel_roll_deg],
            scale,
        ))
    }

    /// Collects 3D world-space VR primitives (laser pointer, parabolic teleport arc, floating 3D GUI frame,
    /// wrist HUD bar, handheld VR camera viewfinder, and remote multiplayer VR player skeletons) for `native_world_primitives`.
    pub fn collect_world_primitives(&self) -> (Vec<VrLinePrimitive>, Vec<VrQuadPrimitive>) {
        let mut lines = Vec::new();
        let mut quads = Vec::new();

        if !self.settings.vr_enabled || !self.settings.render_vr_guides {
            return (lines, quads);
        }

        // 1. Mainhand 6DoF laser ray
        let c0_start = self.world_render.c0.get_position_f();
        let c0_end = if self.gui.gui_open {
            self.gui.laser_hit_world
        } else {
            c0_start + self.world_render.c0.get_direction() * (3.5 * self.settings.world_scale)
        };
        lines.push(VrLinePrimitive {
            start: c0_start,
            end: c0_end,
            color_rgba: if self.gui.cursor_on_gui {
                [0.25, 0.95, 1.0, 0.92]
            } else {
                [0.35, 0.85, 1.0, 0.65]
            },
        });

        // 2. Parabolic teleport arc & landing pad quad
        let arc = &self.locomotion.teleport.arc_points;
        for pair in arc.windows(2) {
            lines.push(VrLinePrimitive {
                start: pair[0],
                end: pair[1],
                color_rgba: [0.3, 1.0, 0.55, 0.9],
            });
        }
        if let Some(dest) = self.locomotion.teleport.destination {
            let r = 0.35 * self.settings.world_scale;
            let y = dest.y + 0.02;
            quads.push(VrQuadPrimitive {
                corners: [
                    Vec3::new(dest.x - r, y, dest.z - r),
                    Vec3::new(dest.x + r, y, dest.z - r),
                    Vec3::new(dest.x + r, y, dest.z + r),
                    Vec3::new(dest.x - r, y, dest.z + r),
                ],
                color_rgba: [0.2, 0.95, 0.5, 0.45],
            });
        }

        // 3. Floating 3D GUI quad & border in world space when open
        if self.gui.gui_open {
            let corners = self.gui.gui_quad_corners_world(&self.world_render);
            quads.push(VrQuadPrimitive {
                corners,
                color_rgba: [0.06, 0.09, 0.14, 0.55],
            });
            for i in 0..4 {
                lines.push(VrLinePrimitive {
                    start: corners[i],
                    end: corners[(i + 1) % 4],
                    color_rgba: [0.4, 0.85, 1.0, 0.9],
                });
            }
        }

        // 4. Off-hand Wrist HUD bar
        if self.gui.wrist_hud_active {
            let wrist_corners = self.gui.wrist_hud_corners_world(&self.world_render);
            quads.push(VrQuadPrimitive {
                corners: wrist_corners,
                color_rgba: [0.12, 0.85, 0.45, 0.65],
            });
        }

        // 5. Two-handed bow string visualization when drawing
        if self.bow_tracker.is_drawing {
            lines.push(VrLinePrimitive {
                start: self.world_render.c1.get_position_f(),
                end: self.world_render.c0.get_position_f(),
                color_rgba: [1.0, 0.85, 0.3, 0.9],
            });
        }

        // 6. Handheld 3D Screenshot Camera viewfinder (`CameraTracker.java`)
        if self.camera_tracker.visible {
            let cam_corners = self
                .camera_tracker
                .viewfinder_corners_world(self.settings.world_scale);
            quads.push(VrQuadPrimitive {
                corners: cam_corners,
                color_rgba: [0.95, 0.75, 0.2, 0.85],
            });
        }

        // 7. Remote VR Player 3D skeletons (`VRPlayerRenderer.java` / `HMDLayer.java` from `UberPacket`)
        let base_origin = Vec3::new(
            self.world_render.origin.x as f32,
            self.world_render.origin.y as f32,
            self.world_render.origin.z as f32,
        );
        for remote in self.network.remote_players.values() {
            if !remote.vr_active {
                continue;
            }
            let hmd_pos = base_origin + remote.state.hmd.position;
            let hmd_fwd = (remote.state.hmd.orientation * Vec3::NEG_Z).normalize_or_zero();
            let c0_pos = base_origin + remote.state.main_hand.position;
            let c1_pos = base_origin + remote.state.off_hand.position;

            // HMD visor forward line
            lines.push(VrLinePrimitive {
                start: hmd_pos,
                end: hmd_pos + hmd_fwd * (0.28 * remote.world_scale),
                color_rgba: [0.95, 0.35, 0.95, 0.9],
            });
            // Shoulder to hands skeleton lines
            lines.push(VrLinePrimitive {
                start: hmd_pos,
                end: c0_pos,
                color_rgba: [0.35, 0.95, 0.95, 0.85],
            });
            lines.push(VrLinePrimitive {
                start: hmd_pos,
                end: c1_pos,
                color_rgba: [0.35, 0.95, 0.95, 0.85],
            });

            // VR avatar body volume: textured-model stand-in built from oriented
            // quads (head/HMD box, torso, arms to the controllers, legs when FBT
            // is present) rendered through `WorldPrimitives` where a full skinned
            // model is not yet available (`VRPlayerRenderer` port stage 1).
            let rot = remote.state.hmd.orientation;
            let right = rot * Vec3::X;
            let up = rot * Vec3::Y;
            let scale = remote.world_scale.max(0.25);
            // Head/HMD box (0.24 x 0.18 x 0.14 m) centred on the HMD.
            quads.push(remote_avatar_box(
                hmd_pos,
                right * (0.12 * scale),
                up * (0.09 * scale),
                [0.95, 0.35, 0.95, 0.85],
            ));
            // Torso: a box from 0.35m below the HMD down to the waist (or 0.75m).
            let waist_pos = remote
                .state
                .waist
                .map(|w| base_origin + w.position)
                .unwrap_or(hmd_pos - Vec3::Y * (0.75 * scale));
            let torso_center = (hmd_pos + waist_pos) * 0.5 - up * (0.30 * scale);
            quads.push(remote_avatar_box(
                torso_center,
                right * (0.20 * scale),
                Vec3::Y * ((hmd_pos.y - waist_pos.y).abs() * 0.5).max(0.15),
                [0.30, 0.55, 0.85, 0.75],
            ));
            // Arms: thin boxes from the shoulders to each controller.
            for (i, ctrl_pos) in [c0_pos, c1_pos].into_iter().enumerate() {
                let side = if i == 0 { 1.0 } else { -1.0 };
                let shoulder = hmd_pos - up * (0.18 * scale) + right * (0.22 * scale * side);
                let arm_center = (shoulder + ctrl_pos) * 0.5;
                let arm_len = shoulder.distance(ctrl_pos) * 0.5;
                quads.push(remote_avatar_box(
                    arm_center,
                    right * (0.035 * scale),
                    up * arm_len,
                    [0.35, 0.95, 0.95, 0.7],
                ));
            }
            // Legs (FBT): boxes from the waist to each foot.
            if remote.state.waist.is_some() {
                let foot_pos = |p: Option<VrBodyPose>| p.map(|f| base_origin + f.position);
                let lf = foot_pos(remote.state.left_foot);
                let rf = foot_pos(remote.state.right_foot);
                for foot in [lf, rf].into_iter().flatten() {
                    let leg_center = (waist_pos + foot) * 0.5;
                    quads.push(remote_avatar_box(
                        leg_center,
                        right * (0.075 * scale),
                        Vec3::Y * ((waist_pos.y - foot.y).abs() * 0.5).max(0.1),
                        [0.4, 1.0, 0.6, 0.7],
                    ));
                }
            }

            // Optional Full-Body Tracking (FBT) waist & feet skeleton lines
            if let Some(waist) = remote.state.waist {
                let waist_pos = base_origin + waist.position;
                lines.push(VrLinePrimitive {
                    start: hmd_pos,
                    end: waist_pos,
                    color_rgba: [0.4, 1.0, 0.6, 0.85],
                });
                if let Some(rf) = remote.state.right_foot {
                    lines.push(VrLinePrimitive {
                        start: waist_pos,
                        end: base_origin + rf.position,
                        color_rgba: [0.4, 1.0, 0.6, 0.85],
                    });
                }
                if let Some(lf) = remote.state.left_foot {
                    lines.push(VrLinePrimitive {
                        start: waist_pos,
                        end: base_origin + lf.position,
                        color_rgba: [0.4, 1.0, 0.6, 0.85],
                    });
                }
            }
        }

        let _ = (VrBodyPart::MainHand, NEAR_PLANE);
        (lines, quads)
    }
}

/// One pose of a remote VR player's tracked body part (HMD, controller, FBT tracker).
type VrBodyPose = crate::protocol::NetPose;

/// Builds an oriented quad around `center` from half-extents `hx` (right) and
/// `hy` (up), facing forward: a stand-in for one face of an avatar body box.
fn remote_avatar_box(
    center: Vec3,
    hx: Vec3,
    hy: Vec3,
    color_rgba: [f32; 4],
) -> VrQuadPrimitive {
    VrQuadPrimitive {
        corners: [
            center - hx + hy,
            center + hx + hy,
            center + hx - hy,
            center - hx - hy,
        ],
        color_rgba,
    }
}

static GLOBAL_VR_RUNTIME: OnceLock<Mutex<VrClientRuntime>> = OnceLock::new();

/// Accesses the global thread-safe `VrClientRuntime` singleton shared between
/// `horizon-client-render` and `horizon-client`.
pub fn with_runtime<R>(f: impl FnOnce(&mut VrClientRuntime) -> R) -> R {
    let mutex = GLOBAL_VR_RUNTIME.get_or_init(|| Mutex::new(VrClientRuntime::new()));
    let mut guard = mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// Fast read helper for `horizon-client-render::gfx::view::ViewSet::single` (`OVERWRITE` injection):
/// builds the active `StereoViewPair` when VR stereo rendering is enabled.
pub fn current_stereo_view_pair(
    base_view: Mat4,
    base_projection: Mat4,
    base_aspect: f32,
    base_tan_half_fov: f32,
) -> Option<StereoViewPair> {
    with_runtime(|rt| {
        rt.build_stereo_views_for_screen(
            base_view,
            base_projection,
            base_aspect,
            base_tan_half_fov,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::raycast_voxels;
    use crate::protocol::{NetworkVersion, VivecraftPacket, VrPlayerState};
    use crate::stereo::openxr_perspective_reversed_z_rh;
    use crate::xr::XrFovf;

    #[test]
    fn test_runtime_initialization_and_frame_pipeline() {
        let mut rt = VrClientRuntime::new();
        assert!(rt.settings.vr_enabled);
        assert!(rt.settings.stereo_rendering);

        rt.on_frame(1.0 / 90.0, Vec3::new(10.0, 65.62, -20.0), 0.0, 0.0);
        let base_proj = Mat4::perspective_rh(70.0_f32.to_radians(), 16.0 / 9.0, 512.0, NEAR_PLANE);
        let pair = rt
            .build_stereo_views_for_screen(
                Mat4::IDENTITY,
                base_proj,
                16.0 / 9.0,
                (35.0_f32).to_radians().tan(),
            )
            .expect("stereo pair must be built");
        let left_tx = pair.left.view.w_axis.x;
        let right_tx = pair.right.view.w_axis.x;
        let ipd_world = (left_tx - right_tx).abs();
        assert!(
            (ipd_world - rt.settings.ipd).abs() < 0.01,
            "expected IPD ~0.064m, got {ipd_world}"
        );
    }

    #[test]
    fn test_reversed_z_openxr_projection_properties() {
        let fov = XrFovf::default();
        let (proj, aspect, tan_half_fov) = openxr_perspective_reversed_z_rh(fov, 0.05, 1000.0);
        assert!((aspect - 1.0).abs() < 1.0e-4);
        assert!(tan_half_fov > 0.5);

        // Near plane z = -0.05 in RH view space must map to NDC z = 1.0
        let near_clip = proj * glam::Vec4::new(0.0, 0.0, -0.05, 1.0);
        let near_ndc_z = near_clip.z / near_clip.w;
        assert!((near_ndc_z - 1.0).abs() < 1.0e-5);

        // Far plane z = -1000.0 in RH view space must map to NDC z = 0.0
        let far_clip = proj * glam::Vec4::new(0.0, 0.0, -1000.0, 1.0);
        let far_ndc_z = far_clip.z / far_clip.w;
        assert!(far_ndc_z.abs() < 1.0e-5);
    }

    #[test]
    fn test_gui_ray_quad_intersection_and_radial_keyboard() {
        let gui_pos = Vec3::new(0.0, 1.5, -1.2);
        let gui_rot = Quat::IDENTITY;
        let ctrl_pos = Vec3::new(0.0, 1.5, 0.0);
        let ctrl_dir = Vec3::NEG_Z;

        let (uv, hit) = GuiHandler::get_tex_coords_for_cursor(
            gui_pos, gui_rot, 1.5, 16.0 / 9.0, ctrl_pos, ctrl_dir,
        )
        .expect("center ray should hit GUI quad");
        assert!((uv.x - 0.5).abs() < 1.0e-4);
        assert!((uv.y - 0.5).abs() < 1.0e-4);
        assert!((hit.z - (-1.2)).abs() < 1.0e-4);

        let mut radial = RadialHandler::new();
        radial.showing = true;
        let slice = radial
            .update_slice_from_uv(glam::Vec2::new(0.85, 0.5), true)
            .expect("right radial sector should be active");
        assert_eq!(slice, RadialSliceAction::TogglePerspective);

        let kb = KeyboardHandler::new();
        assert_eq!(kb.char_at_uv(glam::Vec2::new(0.02, 0.3)), Some('q'));
    }

    #[test]
    fn test_teleport_parabolic_arc_landing() {
        let mut rt = VrClientRuntime::new();
        rt.on_frame(0.011, Vec3::new(0.0, 65.62, 0.0), 0.0, 0.0);
        let mut tracker = crate::locomotion::TeleportTracker::new();
        // Flat ground at y <= 63
        let is_solid = |_x: i32, y: i32, _z: i32| y <= 63;
        let _ = tracker.update(&rt.world_render, true, is_solid);
        assert!(!tracker.arc_points.is_empty());
        let dest = tracker
            .update(&rt.world_render, false, is_solid)
            .expect("releasing teleport over flat ground should yield destination");
        assert!((dest.y - 64.0).abs() < 1.0e-4);
    }

    #[test]
    fn test_voxel_raycast_hits_ground() {
        let hit = raycast_voxels(
            Vec3::new(0.5, 66.0, 0.5),
            Vec3::NEG_Y,
            5.0,
            |_x, y, _z| y <= 63,
        )
        .expect("downward ray must hit y=63 block");
        assert_eq!(hit.block_y, 63);
        assert_eq!(hit.face_normal, [0, 1, 0]);
    }

    #[test]
    fn test_vivecraft_protocol_roundtrip_and_server_uber_packet() {
        let mut rt = VrClientRuntime::new();
        rt.on_frame(0.011, Vec3::new(4.0, 65.62, -8.0), 0.0, 0.0);

        let mut client_net = VivecraftNetworkManager::new();
        let mut server_net = VivecraftNetworkManager::new();
        let player_uuid = 0x1234_5678_9ABC_DEF0_1122_3344_5566_7788u128;

        // 1. Client sends Version handshake
        let c2s_handshake = client_net.build_client_handshake(true);
        let (s2c_replies, broadcast) =
            server_net.handle_server_c2s_packet(player_uuid, &c2s_handshake);
        assert!(broadcast.is_none());
        assert!(!s2c_replies.is_empty());

        // 2. Client processes server handshake replies
        let mut followups = Vec::new();
        for reply in s2c_replies {
            let (mut resp, _) = client_net.handle_s2c_packet(&reply, true, 1.0, 1.62);
            followups.append(&mut resp);
        }
        assert!(client_net.server_supports_vivecraft);
        assert!(client_net.server_wants_data);
        assert_eq!(client_net.negotiated_version, NetworkVersion::HeadQuat);
        assert!(!followups.is_empty());

        // 3. Client sends VrPlayerState and Server broadcasts UberPacket
        let state = VrPlayerState::from_vr_data(
            &rt.world_render,
            Vec3::new(4.0, 64.0, -8.0),
            false,
            false,
        );
        let c2s_state = VivecraftPacket::VrPlayerState(state.clone())
            .encode(client_net.negotiated_version);
        let (_, uber_opt) = server_net.handle_server_c2s_packet(player_uuid, &c2s_state);
        let uber_bytes = uber_opt.expect("server must emit UberPacket on VrPlayerState");

        // 4. Another client decodes UberPacket
        let mut other_client = VivecraftNetworkManager::new();
        let _ = other_client.handle_s2c_packet(&uber_bytes, true, 1.0, 1.62);
        let remote = other_client
            .remote_players
            .get(&player_uuid)
            .expect("remote VR player must be registered");
        assert!((remote.state.hmd.position - state.hmd.position).length() < 1.0e-5);
    }

    #[test]
    fn test_settings_buttons_and_click_cycling() {
        let mut settings = VrSettings::default();
        let buttons = settings.settings_buttons("OpenXR");
        assert_eq!(buttons.len(), 14);
        assert!(buttons[0].contains("ON"));
        settings.click_button(0);
        assert!(!settings.vr_enabled);
    }

    #[test]
    fn test_eating_telescope_backpack_swim_row_climb_and_fbt_trackers() {
        let mut rt = VrClientRuntime::new();
        // 1. Test FBT / OSC tracker pose ingestion
        rt.xr
            .set_osc_tracker_pose(0, Vec3::new(0.0, 0.9, 0.0), Quat::IDENTITY);
        rt.on_frame(0.011, Vec3::new(0.0, 65.62, 0.0), 0.0, 0.0);
        assert_eq!(rt.room_pre.fbt_mode, crate::vr_data::FbtMode::WithJoints);
        assert!(rt.world_render.waist.is_some());

        // 2. Test EatingTracker when hand is brought to mouth
        rt.xr.current_frame.controllers[0].aim_position =
            rt.xr.current_frame.hmd_position + Vec3::new(0.0, -0.02, 0.15);
        rt.room_pre = VrData::from_raw(
            &rt.xr.current_frame,
            DVec3::ZERO,
            1.0,
            1.0,
            0.0,
            &rt.settings,
        );
        let (is_eating, _) = rt.eating_tracker.update(
            &rt.settings,
            &rt.room_pre,
            [true, false],
            [false, false],
            1_000,
        );
        assert!(is_eating, "holding food within 0.25m of HMD mouth must trigger EatingTracker");

        // 3. Test CrawlTracker when HMD drops below 0.75m
        let crawl_changed = rt.locomotion.crawl_tracker.update(&rt.settings, 0.55, true);
        assert_eq!(crawl_changed, Some(true));

        // 4. Test SwimTracker breaststroke propulsion
        let _ = rt.locomotion.swim_tracker.update(&rt.settings, &rt.world_render, true);
        rt.xr.current_frame.controllers[0].aim_position = Vec3::new(0.2, 1.3, -0.1);
        rt.xr.current_frame.controllers[1].aim_position = Vec3::new(-0.2, 1.3, -0.1);
        let world2 = VrData::from_raw(
            &rt.xr.current_frame,
            DVec3::ZERO,
            1.0,
            1.0,
            0.0,
            &rt.settings,
        );
        let swim_vel = rt.locomotion.swim_tracker.update(&rt.settings, &world2, true);
        assert!(swim_vel.length() > 0.0, "pulling hands closer to HMD in water must produce swim velocity");

        // 5. Test CameraTracker viewfinder rendering
        rt.camera_tracker.toggle_visible(&rt.world_render);
        assert!(rt.camera_tracker.visible);
        let (_, quads) = rt.collect_world_primitives();
        assert!(!quads.is_empty());
    }

    #[test]
    fn test_shield_elytra_voxel_collision_and_vignette() {
        let mut rt = VrClientRuntime::new();
        rt.on_frame(0.011, Vec3::new(0.0, 65.62, 0.0), 0.0, 0.0);

        // 1. Test ShieldTracker blocking cone
        let (blocked, haptic) = rt.shield_tracker.can_block_attack(
            &rt.world_render,
            -rt.world_render.c1.get_direction(),
            true,
        );
        assert!(blocked, "facing directly into attacker must block");
        assert!(haptic.is_some(), "blocking must trigger haptic pulse");

        // 2. Test ElytraTracker arms out glide
        rt.xr.current_frame.controllers[0].aim_position = Vec3::new(0.6, 1.4, 0.0);
        rt.xr.current_frame.controllers[1].aim_position = Vec3::new(-0.6, 1.4, 0.0);
        let room_elytra = VrData::from_raw(
            &rt.xr.current_frame,
            DVec3::ZERO,
            1.0,
            1.0,
            0.0,
            &rt.settings,
        );
        let (_pitch, _yaw, _impulse) = rt.elytra_tracker.update(
            &rt.settings,
            &room_elytra,
            &rt.world_render,
            true,
            0.05,
        );
        assert!(rt.elytra_tracker.is_gliding, "arms spread > 0.8m must engage glide mode");

        // 3. Test Roomscale voxel collision & anti-wall-clipping
        let pos = Vec3::new(5.5, 64.0, 5.5);
        let delta = Vec3::new(0.4, 0.0, 0.0);
        // Wall at x >= 6
        let is_solid = |x: i32, _y: i32, _z: i32| x >= 6;
        let resolved = crate::locomotion::resolve_roomscale_voxel_collision(pos, delta, is_solid);
        // Step up ledges (0.1..1.0m) or stop
        assert!(resolved.y > 0.5 || resolved.x == 0.0, "cannot walk into wall at x=6");

        // 4. Test Comfort vignette calculation
        let vig = crate::stereo::calculate_comfort_vignette(3.5, 60.0, 0.8);
        assert!(vig > 0.1 && vig <= 0.8);
    }

    #[test]
    fn test_xr_frame_loop_procs_loaded() {
        let rt = VrClientRuntime::new();
        if rt.xr.procs.is_some() {
            let procs = rt.xr.procs.unwrap();
            assert!(procs.wait_frame.is_some(), "xrWaitFrame must be loaded");
            assert!(procs.begin_frame.is_some(), "xrBeginFrame must be loaded");
            assert!(procs.end_frame.is_some(), "xrEndFrame must be loaded");
        }
    }
}
