// ViveCraft VR native mod runtime for Minecraft Horizon (`game::vivecraft_mod`).
// Connects `Game` native hooks to `vivecraft_vr` (re-exported via `crate::gfx::vr_bridge`)
// and provides both the full OpenXR/stereo VR client and the `vivecraft:data` server relay.

use crate::first_person::Hand;
use crate::game::native_block_entity_draw::BlockEntityDraw;
use crate::game::native_hooks::{
    ClientboundPlayPacket, NativeMod, NativeScreenView, NativeShake, NativeZoom,
};
use crate::game::native_mob_draw::{NativeMobRender, NativePlayerRender};
use crate::game::Game;
use crate::gfx::vr_bridge::{
    self as vr, NetworkVersion, Vec3, VivecraftNetworkManager, VivecraftPacket, VrSettings,
    VIVECRAFT_DATA_CHANNEL,
};
use crate::items::ItemStack;
use crate::mod_api::ServerboundPlayPacket;
use crate::world_primitives::WorldPrimitives;
use horizon_protocol::packets::configuration::CustomPayload;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

pub const ID: &str = "vivecraft";
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str =
    "ViveCraft VR for Horizon: OpenXR 6DoF tracking, stereo ViewSet rendering, 3D radial/keyboard GUI, roomscale & teleport locomotion, physical combat/bow, and vivecraft:data multiplayer.";

pub const KEY_TOGGLE_VR: &str = "toggle_vr";
pub const DEFAULT_KEY_TOGGLE_VR: &str = "key.keyboard.f7";

pub const KEY_RECENTER: &str = "recenter_vr";
pub const DEFAULT_KEY_RECENTER: &str = "key.keyboard.f8";

pub const KEY_TELEPORT: &str = "teleport_vr";
pub const DEFAULT_KEY_TELEPORT: &str = "key.keyboard.t";

pub const KEY_RADIAL: &str = "radial_menu";
pub const DEFAULT_KEY_RADIAL: &str = "key.keyboard.r";

pub const KEY_KEYBOARD: &str = "vr_keyboard";
pub const DEFAULT_KEY_KEYBOARD: &str = "key.keyboard.k";

pub const KEY_CALIBRATE: &str = "calibrate_height";
pub const DEFAULT_KEY_CALIBRATE: &str = "key.keyboard.v";

pub const KEY_CAMERA: &str = "vr_camera";
pub const DEFAULT_KEY_CAMERA: &str = "key.keyboard.c";

const CONFIG_KEY: &str = "config_json";

static SERVER_VR_NET: Mutex<Option<VivecraftNetworkManager>> = Mutex::new(None);

/// Server-side AimFix state (`ServerVRPlayers.java`): stores remote VR players' controller aim and original poses
#[derive(Debug, Clone, Default)]
struct ServerAimFixState {
    player_aim: HashMap<u128, (Vec3, Vec3)>, // uuid -> (aim_pos, aim_dir)
}

static SERVER_AIM_FIX: Mutex<ServerAimFixState> = Mutex::new(ServerAimFixState {
    player_aim: HashMap::new(),
});

fn load_settings(game: &Game<'_>) -> VrSettings {
    game.native_setting(ID, CONFIG_KEY)
        .and_then(|json| serde_json::from_str::<VrSettings>(&json).ok())
        .unwrap_or_default()
}

fn save_settings(game: &mut Game<'_>, settings: &VrSettings) {
    if let Ok(json) = serde_json::to_string(settings) {
        game.native_set_setting(ID, CONFIG_KEY, &json);
    }
}

fn ensure_settings_loaded(game: &Game<'_>) {
    vr::with_runtime(|rt| {
        if rt.frame_count == 0 && rt.tick_count == 0 {
            rt.settings = load_settings(game);
        }
    });
}

pub fn list_mod(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: ID.into(),
        title: format!("ViveCraft VR {VERSION}"),
        description: DESCRIPTION.into(),
        keys: vec![
            [
                KEY_TOGGLE_VR.into(),
                "Toggle VR Stereo Mode".into(),
                DEFAULT_KEY_TOGGLE_VR.into(),
            ],
            [
                KEY_RECENTER.into(),
                "Recenter VR Tracking".into(),
                DEFAULT_KEY_RECENTER.into(),
            ],
            [
                KEY_TELEPORT.into(),
                "VR Teleport".into(),
                DEFAULT_KEY_TELEPORT.into(),
            ],
            [
                KEY_RADIAL.into(),
                "VR Radial Menu".into(),
                DEFAULT_KEY_RADIAL.into(),
            ],
            [
                KEY_KEYBOARD.into(),
                "VR 3D Keyboard".into(),
                DEFAULT_KEY_KEYBOARD.into(),
            ],
            [
                KEY_CALIBRATE.into(),
                "Calibrate HMD Height".into(),
                DEFAULT_KEY_CALIBRATE.into(),
            ],
            [
                KEY_CAMERA.into(),
                "VR Handheld Camera".into(),
                DEFAULT_KEY_CAMERA.into(),
            ],
        ],
    });
}

pub fn settings_buttons(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != ID {
        return;
    }
    let loaded = load_settings(game);
    let labels = vr::with_runtime(|rt| {
        rt.settings = loaded;
        rt.settings.settings_buttons(rt.provider_name())
    });
    buttons.extend(labels);
}

pub fn settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != ID {
        return;
    }
    let mut updated = load_settings(game);
    updated.click_button(button);
    save_settings(game, &updated);
    vr::with_runtime(|rt| {
        rt.settings = updated;
    });
}

pub fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(ID) {
        return;
    }
    let toggle_key = game.native_mod_key(ID, KEY_TOGGLE_VR, DEFAULT_KEY_TOGGLE_VR);
    let recenter_key = game.native_mod_key(ID, KEY_RECENTER, DEFAULT_KEY_RECENTER);
    let teleport_key = game.native_mod_key(ID, KEY_TELEPORT, DEFAULT_KEY_TELEPORT);
    let radial_key = game.native_mod_key(ID, KEY_RADIAL, DEFAULT_KEY_RADIAL);
    let keyboard_key = game.native_mod_key(ID, KEY_KEYBOARD, DEFAULT_KEY_KEYBOARD);
    let calibrate_key = game.native_mod_key(ID, KEY_CALIBRATE, DEFAULT_KEY_CALIBRATE);
    let camera_key = game.native_mod_key(ID, KEY_CAMERA, DEFAULT_KEY_CAMERA);

    if key == toggle_key {
        let new_settings = vr::with_runtime(|rt| {
            rt.settings.vr_enabled = !rt.settings.vr_enabled;
            rt.settings.clone()
        });
        save_settings(game, &new_settings);
        let status = if new_settings.vr_enabled {
            "ENABLED"
        } else {
            "DISABLED"
        };
        game.native_chat(&format!("[ViveCraft] VR mode {status}"));
    } else if key == recenter_key && game.native_playing() {
        vr::with_runtime(|rt| {
            rt.xr.recenter();
            rt.world_post.rotation_radians = 0.0;
        });
        game.native_chat("[ViveCraft] Recentered VR Tracking");
    } else if key == teleport_key && game.native_playing() {
        vr::with_runtime(|rt| {
            let dest = rt.world_render.hmd.get_position_f() + rt.world_render.hmd.get_direction() * 4.5;
            rt.outgoing_packets.push(
                crate::protocol::VivecraftPacket::Teleport(Some(dest))
                    .encode(rt.network.negotiated_version),
            );
        });
    } else if key == radial_key && game.native_playing() {
        vr::with_runtime(|rt| {
            rt.gui.radial_open = !rt.gui.radial_open;
            if rt.gui.radial_open && !rt.gui.gui_open {
                let dist = rt.settings.hud_distance;
                let scale = rt.settings.hud_scale;
                rt.gui.open_gui(&rt.room_pre, dist, scale);
            } else if !rt.gui.radial_open && !rt.gui.keyboard_open {
                rt.gui.close_gui();
            }
        });
    } else if key == keyboard_key && game.native_playing() {
        vr::with_runtime(|rt| {
            rt.gui.keyboard_open = !rt.gui.keyboard_open;
            if rt.gui.keyboard_open && !rt.gui.gui_open {
                let dist = rt.settings.hud_distance;
                let scale = rt.settings.hud_scale;
                rt.gui.open_gui(&rt.room_pre, dist, scale);
            } else if !rt.gui.keyboard_open && !rt.gui.radial_open {
                rt.gui.close_gui();
            }
        });
    } else if key == calibrate_key && game.native_playing() {
        let mut calibrated_m = 1.62f32;
        let mut saved = None;
        vr::with_runtime(|rt| {
            let hmd_y = rt.room_pre.hmd.pos_room.y;
            if hmd_y > 0.4 && hmd_y < 2.6 {
                rt.settings.calibrated_height = hmd_y;
                calibrated_m = hmd_y;
                saved = Some(rt.settings.clone());
            }
        });
        if let Some(s) = saved {
            save_settings(game, &s);
            game.native_chat(&format!(
                "[ViveCraft] Calibrated HMD height to {calibrated_m:.2}m"
            ));
        }
    } else if key == camera_key && game.native_playing() {
        let visible = vr::with_runtime(|rt| {
            rt.camera_tracker.toggle_visible(&rt.world_render);
            rt.camera_tracker.visible
        });
        let state_str = if visible { "SHOWN" } else { "HIDDEN" };
        game.native_chat(&format!("[ViveCraft] Handheld VR Camera {state_str}"));
    }
}

pub fn on_frame(game: &mut Game<'_>, elapsed: Duration) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    ensure_settings_loaded(game);
    let dt = elapsed.as_secs_f32().clamp(0.0005, 0.1);
    let (eye_pos, yaw, pitch) = if let Some(p) = game.native_player() {
        (
            Vec3::new(p.x as f32, (p.y + 1.62) as f32, p.z as f32),
            p.yaw,
            p.pitch,
        )
    } else {
        (Vec3::new(0.0, 65.62, 0.0), 0.0, 0.0)
    };

    vr::with_runtime(|rt| {
        rt.on_frame(dt, eye_pos, yaw, pitch);
    });
}

pub fn on_tick(game: &mut Game<'_>) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    ensure_settings_loaded(game);
    let playing = game.native_playing();
    let player = game.native_player();
    let is_solid = |bx: i32, by: i32, bz: i32| -> bool {
        let id = game.native_block([bx, by, bz]);
        !matches!(
            id.as_str(),
            "" | "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:water"
        ) && !game.native_block_collision([bx, by, bz]).is_empty()
    };

    let (player_pos, eye_pos, yaw_rad, in_world) = if let Some(p) = &player {
        let pos = Vec3::new(p.x as f32, p.y as f32, p.z as f32);
        let eye = Vec3::new(p.x as f32, (p.y + 1.62) as f32, p.z as f32);
        (pos, eye, p.yaw.to_radians(), playing)
    } else {
        (Vec3::ZERO, Vec3::ZERO, 0.0, false)
    };

    let outbound_packets = vr::with_runtime(|rt| {
        rt.on_tick(player_pos, eye_pos, yaw_rad, false, in_world, is_solid);
        std::mem::take(&mut rt.outgoing_packets)
    });

    for data in outbound_packets {
        let _ = game.native_send_packet(ServerboundPlayPacket::CustomPayload(CustomPayload {
            channel: VIVECRAFT_DATA_CHANNEL.into(),
            data,
        }));
    }
}

pub fn on_camera_pose(
    game: &Game<'_>,
    _partial: f32,
    position: &mut [f64; 3],
    yaw: &mut f32,
    pitch: &mut f32,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled {
            return;
        }
        let hmd = &rt.world_render.hmd;
        let pos = hmd.get_position();
        if pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() && rt.frame_count > 0 {
            *position = [pos.x, pos.y, pos.z];
            *yaw = hmd.get_yaw_deg();
            *pitch = hmd.get_pitch_deg();
        }
    });
}

pub fn on_camera_shake(game: &Game<'_>, _partial: f32, shake: &mut NativeShake) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled {
            return;
        }
        shake.roll += rt.world_render.hmd.get_roll_deg();
    });
}

pub fn on_movement_input(
    game: &Game<'_>,
    forward: &mut f32,
    strafe: &mut f32,
    jump: &mut bool,
    sneak: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled {
            return;
        }
        let mv = &rt.last_movement_output;
        if mv.forward.abs() > 0.01 || mv.strafe.abs() > 0.01 {
            *forward = (*forward + mv.forward).clamp(-1.0, 1.0);
            *strafe = (*strafe + mv.strafe).clamp(-1.0, 1.0);
        }
        if mv.jump {
            *jump = true;
        }
        if mv.sneak {
            *sneak = true;
        }

        // Roomscale physical movement delta & velocity impulse projected into forward/strafe
        let total_impulse = mv.roomscale_world_delta + mv.velocity_impulse;
        if total_impulse.length_squared() > 1e-6 {
            let player_yaw_rad = rt.world_render.hmd.get_yaw_rad();
            let (sin_y, cos_y) = player_yaw_rad.sin_cos();
            let fwd_impulse = -total_impulse.x * sin_y + total_impulse.z * cos_y;
            let str_impulse = total_impulse.x * cos_y + total_impulse.z * sin_y;
            *forward = (*forward + fwd_impulse * 3.5).clamp(-1.0, 1.0);
            *strafe = (*strafe + str_impulse * 3.5).clamp(-1.0, 1.0);
            if total_impulse.y > 0.25 {
                *jump = true;
            }
        }
    });
}

pub fn on_hand_pose(
    game: &Game<'_>,
    hand: Hand,
    _partial: f32,
    x: &mut f32,
    y: &mut f32,
    z: &mut f32,
    pitch: &mut f32,
    yaw: &mut f32,
    roll: &mut f32,
    scale: &mut f32,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled {
            return;
        }
        let is_offhand = matches!(hand, Hand::Off);
        let cam_pos = rt.world_render.hmd.get_position_f();
        let cam_yaw = rt.world_render.hmd.get_yaw_rad();
        let cam_pitch = rt.world_render.hmd.get_pitch_rad();
        if let Some((offset, angles, hand_scale)) =
            rt.compute_hand_pose(is_offhand, cam_pos, cam_yaw, cam_pitch)
        {
            *x += (offset[0] * 0.45).clamp(-1.5, 1.5);
            *y += (offset[1] * 0.45).clamp(-1.5, 1.5);
            *z += (offset[2] * 0.45).clamp(-1.5, 1.5);
            *pitch += angles[0] * 0.6;
            *yaw += angles[1] * 0.6;
            *roll += angles[2] * 0.6;
            *scale *= hand_scale;
        }
    });
}

pub fn on_world_primitives(
    game: &Game<'_>,
    _partial: f32,
    _camera: [f64; 3],
    primitives: &mut WorldPrimitives,
) {
    if !game.native_on(ID) {
        return;
    }
    let (lines, quads) = vr::with_runtime(|rt| rt.collect_world_primitives());
    for line in lines {
        let color = rgba_u32(line.color_rgba);
        primitives.line(
            [
                f64::from(line.start.x),
                f64::from(line.start.y),
                f64::from(line.start.z),
            ],
            [
                f64::from(line.end.x),
                f64::from(line.end.y),
                f64::from(line.end.z),
            ],
            color,
            2.2,
        );
    }
    for quad in quads {
        let color = rgba_u32(quad.color_rgba);
        for i in 0..4 {
            let a = quad.corners[i];
            let b = quad.corners[(i + 1) % 4];
            primitives.line(
                [f64::from(a.x), f64::from(a.y), f64::from(a.z)],
                [f64::from(b.x), f64::from(b.y), f64::from(b.z)],
                color,
                2.0,
            );
        }
    }
}

fn rgba_u32(rgba: [f32; 4]) -> u32 {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u32;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u32;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u32;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

pub fn on_client_packet(game: &mut Game<'_>, packet: &ClientboundPlayPacket) {
    if !game.native_on(ID) {
        return;
    }
    let ClientboundPlayPacket::CustomPayload(payload) = packet else {
        return;
    };
    if payload.channel != VIVECRAFT_DATA_CHANNEL {
        return;
    }
    let responses = vr::with_runtime(|rt| {
        let (replies, haptic) = rt.network.handle_s2c_packet(
            &payload.data,
            rt.settings.vr_enabled,
            rt.settings.world_scale,
            rt.settings.calibrated_height,
        );
        if let Some(pulse) = haptic {
            rt.xr.trigger_haptic_pulse(
                pulse.controller,
                pulse.duration_micros,
                pulse.frequency_hz,
                pulse.amplitude,
            );
        }
        replies
    });
    for data in responses {
        let _ = game.native_send_packet(ServerboundPlayPacket::CustomPayload(CustomPayload {
            channel: VIVECRAFT_DATA_CHANNEL.into(),
            data,
        }));
    }
}

pub fn on_custom_payload_from_client(
    game: &mut Game<'_>,
    player: u128,
    channel: &str,
    data: &[u8],
) {
    if channel != VIVECRAFT_DATA_CHANNEL {
        return;
    }
    let mut guard = SERVER_VR_NET.lock().unwrap_or_else(|e| e.into_inner());
    let net = guard.get_or_insert_with(VivecraftNetworkManager::new);

    // Check for direct Teleport packet before or alongside state handling.
    let ver = net
        .remote_players
        .get(&player)
        .map(|p| p.negotiated_version)
        .unwrap_or(NetworkVersion::HeadQuat);
    if let Some(VivecraftPacket::Teleport(Some(dest))) = VivecraftPacket::decode(data, ver) {
        if dest.x.is_finite() && dest.y.is_finite() && dest.z.is_finite() {
            if let Some(p) = game
                .native_server_players()
                .into_iter()
                .find(|p| p.uuid == player)
            {
                let dx = (f64::from(dest.x) - p.position[0]).abs();
                let dy = (f64::from(dest.y) - p.position[1]).abs();
                let dz = (f64::from(dest.z) - p.position[2]).abs();
                if dx <= 32.0 && dy <= 32.0 && dz <= 32.0 {
                    let _ = game.native_run_command(&format!(
                        "tp {} {:.4} {:.4} {:.4}",
                        p.name, dest.x, dest.y, dest.z
                    ));
                }
            }
        }
    }

    let (direct_replies, broadcast) = net.handle_server_c2s_packet(player, data);
    drop(guard);

    for reply in direct_replies {
        let _ =
            game.native_send_custom_payload_to_player(player, VIVECRAFT_DATA_CHANNEL, &reply);
    }
    if let Some(uber_bytes) = broadcast {
        for other in game.native_server_players() {
            if other.uuid != player {
                let _ = game.native_send_custom_payload_to_player(
                    other.uuid,
                    VIVECRAFT_DATA_CHANNEL,
                    &uber_bytes,
                );
            }
        }
    }

    // Server-side AimFix tracking: store active VR player's controller aim direction
    if let Some(VivecraftPacket::VrPlayerState(state)) = VivecraftPacket::decode(data, ver) {
        let aim_dir = state.main_hand.orientation * Vec3::NEG_Z;
        let aim_pos = state.main_hand.position;
        if let Ok(mut aim_guard) = SERVER_AIM_FIX.lock() {
            aim_guard.player_aim.insert(player, (aim_pos, aim_dir));
        }
    }
}

/// Zoom hook for spyglass / telescope in VR (`native_zoom`).
pub fn on_zoom(game: &Game<'_>, zoom: &mut Option<NativeZoom>) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        let factor = rt.current_zoom_factor();
        if factor < 0.98 {
            *zoom = Some(NativeZoom {
                factor,
                smooth: true,
                turn: factor,
            });
        }
    });
}

/// Two-handed VR archery: start nocking an arrow (`native_arrow_nock`).
pub fn on_arrow_nock(
    game: &mut Game<'_>,
    _player: u128,
    _name: &str,
    _bow: &ItemStack,
    _hand: Hand,
    _has_ammo: bool,
    _cancel: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.settings.vr_enabled {
            rt.bow_tracker.is_drawing = true;
        }
    });
}

/// Two-handed VR archery: release an arrow with physical charge (`native_arrow_loose`).
pub fn on_arrow_loose(
    game: &mut Game<'_>,
    _stack: &ItemStack,
    _has_ammo: bool,
    charge: &mut i32,
    _canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.settings.vr_enabled && rt.bow_tracker.is_drawing {
            let power = rt.bow_tracker.current_draw;
            *charge = (power * 20.0).round() as i32;
            rt.bow_tracker.is_drawing = false;
        }
    });
}

/// Physical melee and weapon attack swing feedback (`native_client_attack`).
pub fn on_client_attack(game: &mut Game<'_>, _target: u32, _player: bool) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.settings.vr_enabled {
            rt.xr.trigger_haptic_pulse(0, 30_000, 180.0, 0.7);
        }
    });
}

/// 3D Floating GUI mouse button click emulation from controller laser pointer (`native_screen_mouse_button_pressed_pre`).
pub fn on_screen_mouse_button_pressed_pre(
    game: &Game<'_>,
    _screen: &NativeScreenView<'_>,
    _button: i32,
    _modifiers: u32,
    _canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.gui.cursor_on_gui {
            rt.xr.trigger_haptic_pulse(0, 15_000, 200.0, 0.45);
        }
    });
}

/// 3D VR Virtual Keyboard char typed pre-hook (`native_screen_char_typed_pre`).
pub fn on_screen_char_typed_pre(
    game: &Game<'_>,
    _screen: &NativeScreenView<'_>,
    _c: char,
    _modifiers: u32,
    _canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
}

/// 3D Menu World background diorama rendering hook (`native_menu_background`).
pub fn on_menu_background(game: &Game<'_>, radius: &mut Option<f32>) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.settings.vr_enabled {
            // Keep background unblurred in VR so the player can view the live 3D world
            *radius = None;
        }
    });
}

/// Remote VR player 3D skeleton & avatar rendering hook (`native_player_submit`).
pub fn on_player_submit(
    game: &Game<'_>,
    _player: &NativeMobRender<'_>,
    _who: &NativePlayerRender<'_>,
    _draw: &mut BlockEntityDraw,
    _drawn: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
}
