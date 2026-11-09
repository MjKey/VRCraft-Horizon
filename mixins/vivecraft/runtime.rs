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
pub const VERSION: &str = "0.1.3";
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

/// Server-side AimFix and dynamic hitbox state (`ServerVRPlayers.java`): stores remote VR players' controller aim and original poses
#[derive(Debug, Clone, Default)]
struct ServerAimFixState {
    player_aim: HashMap<u128, (Vec3, Vec3)>, // uuid -> (aim_pos, aim_dir)
    player_height: HashMap<u128, f32>,        // uuid -> hitbox height (1.8 / 1.5 / 0.6)
}

static SERVER_AIM_FIX: Mutex<ServerAimFixState> = Mutex::new(ServerAimFixState {
    player_aim: HashMap::new(),
    player_height: HashMap::new(),
});

pub fn server_aim_for_player(uuid: u128) -> Option<(Vec3, Vec3)> {
    SERVER_AIM_FIX.lock().ok()?.player_aim.get(&uuid).copied()
}

pub fn server_hitbox_height_for_player(uuid: u128) -> f32 {
    SERVER_AIM_FIX
        .lock()
        .ok()
        .and_then(|g| g.player_height.get(&uuid).copied())
        .unwrap_or(1.8)
}

/// Server-side arrow headshot calculator (`ServerVRPlayers.java:230-265`).
/// If arrow hits above `eye_y - 0.25m`, damage multiplier is 3.0x with headshot crit sound/particles.
pub fn calculate_arrow_damage(hit_y: f64, target_eye_y: f64, base_damage: f32) -> (f32, bool) {
    if hit_y > (target_eye_y - 0.25) {
        (base_damage * 3.0, true)
    } else {
        (base_damage, false)
    }
}

fn is_edible_item(item_id: &str) -> bool {
    matches!(
        item_id,
        "minecraft:apple"
            | "minecraft:baked_potato"
            | "minecraft:beef"
            | "minecraft:beetroot"
            | "minecraft:beetroot_soup"
            | "minecraft:bread"
            | "minecraft:carrot"
            | "minecraft:chorus_fruit"
            | "minecraft:cooked_beef"
            | "minecraft:cooked_chicken"
            | "minecraft:cooked_cod"
            | "minecraft:cooked_mutton"
            | "minecraft:cooked_porkchop"
            | "minecraft:cooked_rabbit"
            | "minecraft:cooked_salmon"
            | "minecraft:cookie"
            | "minecraft:dried_kelp"
            | "minecraft:enchanted_golden_apple"
            | "minecraft:golden_apple"
            | "minecraft:golden_carrot"
            | "minecraft:melon_slice"
            | "minecraft:mushroom_stew"
            | "minecraft:mutton"
            | "minecraft:poisonous_potato"
            | "minecraft:porkchop"
            | "minecraft:potato"
            | "minecraft:pufferfish"
            | "minecraft:pumpkin_pie"
            | "minecraft:rabbit"
            | "minecraft:rabbit_stew"
            | "minecraft:rotten_flesh"
            | "minecraft:salmon"
            | "minecraft:spider_eye"
            | "minecraft:suspicious_stew"
            | "minecraft:sweet_berries"
            | "minecraft:glow_berries"
            | "minecraft:tropical_fish"
            | "minecraft:pumpkin_pie"
    )
}

fn is_drink_item(item_id: &str) -> bool {
    matches!(
        item_id,
        "minecraft:potion"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:milk_bucket"
            | "minecraft:honey_bottle"
    )
}

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

    let (cursor_update, typed_char, radial_action) = vr::with_runtime(|rt| {
        rt.on_frame(dt, eye_pos, yaw, pitch);
        let cursor = if rt.gui.cursor_on_gui {
            let uv = rt.gui.cursor_uv;
            let trigger = rt.xr.current_frame.controllers[0].trigger > 0.5
                || rt.xr.current_frame.controllers[0].button_primary;
            Some((uv.x * 854.0, (1.0 - uv.y) * 480.0, trigger))
        } else {
            None
        };
        let typed = if rt.gui.keyboard_open {
            rt.gui.keyboard.typed_buffer.pop()
        } else {
            None
        };
        // Radial menu dispatch: trigger click on a hovered slice executes that action
        // and consumes the selection (`GuiRadial` / `RadialHandler`).
        let radial = if rt.gui.cursor_on_gui
            && rt.gui.radial.showing
            && (rt.xr.current_frame.controllers[0].trigger > 0.5
                || rt.xr.current_frame.controllers[0].button_primary)
        {
            rt.gui.radial.take_selected_action()
        } else {
            None
        };
        (cursor, typed, radial)
    });

    if let Some((cx, cy, trigger)) = cursor_update {
        game.cursor = (cx, cy);
        if trigger {
            game.left = true;
        }
    }

    if let Some(ch) = typed_char {
        let text = ch.to_string();
        game.screen_chars_typed(Some(&text));
    }

    if let Some(action) = radial_action {
        execute_radial_action(game, action);
    }
}

/// Executes a radial-menu slice action against Horizon's game state
/// (`RadialSliceAction` dispatch: inventory, chat, drop, camera, calibration, seated mode...).
fn execute_radial_action(game: &mut Game<'_>, action: vr::RadialSliceAction) {
    use vr::RadialSliceAction;
    match action {
        RadialSliceAction::OpenInventory => {
            let _ = game.native_run_command("inventory");
        }
        RadialSliceAction::OpenChat => {
            let _ = game.native_run_command("chat");
        }
        RadialSliceAction::DropHeldItem => {
            // `Q` drop: simulate the drop key through the command layer.
            let _ = game.native_run_command("drop");
        }
        RadialSliceAction::TogglePerspective => {
            let _ = game.native_run_command("toggleperspective");
        }
        RadialSliceAction::ToggleKeyboard => {
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
        }
        RadialSliceAction::ToggleScreenshotCamera => {
            vr::with_runtime(|rt| {
                rt.camera_tracker.toggle_visible(&rt.world_render);
            });
        }
        RadialSliceAction::CalibrateHeight => {
            let mut calibrated = None;
            vr::with_runtime(|rt| {
                let hmd_y = rt.room_pre.hmd.pos_room.y;
                if hmd_y > 0.4 && hmd_y < 2.6 {
                    rt.settings.calibrated_height = hmd_y;
                    calibrated = Some(rt.settings.clone());
                }
            });
            if let Some(s) = calibrated {
                save_settings(game, &s);
            }
        }
        RadialSliceAction::ToggleSeatedMode => {
            let mut saved = None;
            vr::with_runtime(|rt| {
                rt.settings.seated = !rt.settings.seated;
                saved = Some(rt.settings.clone());
            });
            if let Some(s) = saved {
                save_settings(game, &s);
            }
        }
    }
    if let Some(p) = game.native_player() {
        let _ = p;
    }
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

    let mainhand_stack = game
        .inventory
        .get(game.selected)
        .and_then(|s| s.as_ref());
    let mainhand_item = mainhand_stack.map(|s| s.item.as_ref()).unwrap_or("");
    let offhand_stack = game.offhand.as_ref();
    let offhand_item = offhand_stack.map(|s| s.item.as_ref()).unwrap_or("");

    // Vanilla-NBT custom VR item detection (`ViveItems`): named/flagged vanilla items,
    // playable on ordinary vanilla servers.
    let vr_item = |stack: Option<&ItemStack>| {
        stack.and_then(|s| {
            let data = s.data();
            let name = data.and_then(|d| d.custom_name_plain());
            let unbreakable = data.and_then(|d| d.other("minecraft:unbreakable")).is_some();
            let dyed = data.and_then(|d| d.dyed_color);
            vr::custom_item_of(&s.item, name.as_deref(), unbreakable, dyed)
        })
    };
    let mainhand_vr_item = vr_item(mainhand_stack);
    let offhand_vr_item = vr_item(offhand_stack);
    // Jump Boots work from either hand's feet slot as Vivecraft checks the equipped boots.
    let jump_boots_equipped = mainhand_vr_item == Some(vr::CustomVrItem::JumpBoots)
        || offhand_vr_item == Some(vr::CustomVrItem::JumpBoots);

    let holding_bow = matches!(
        mainhand_item,
        "minecraft:bow" | "minecraft:crossbow"
    ) || matches!(
        offhand_item,
        "minecraft:bow" | "minecraft:crossbow"
    );

    let in_water = if in_world {
        let bx = player_pos.x.floor() as i32;
        let by = player_pos.y.floor() as i32;
        let bz = player_pos.z.floor() as i32;
        let block_feet = game.native_block([bx, by, bz]);
        let block_legs = game.native_block([bx, by + 1, bz]);
        block_feet == "minecraft:water" || block_legs == "minecraft:water"
    } else {
        false
    };

    let riding = game.riding.is_some();
    let hand_is_edible = [is_edible_item(mainhand_item), is_edible_item(offhand_item)];
    let hand_is_drink = [is_drink_item(mainhand_item), is_drink_item(offhand_item)];
    // EAT ME / DRINK ME scale mechanics (`ViveItems`).
    let eat_me_eaten = mainhand_vr_item == Some(vr::CustomVrItem::EatMeCake)
        || offhand_vr_item == Some(vr::CustomVrItem::EatMeCake);
    let drink_me_drank = mainhand_vr_item == Some(vr::CustomVrItem::DrinkMePotion)
        || offhand_vr_item == Some(vr::CustomVrItem::DrinkMePotion);

    let (outbound_packets, teleport_target, swing_attack, door_hit, backpack_slot, backpack_swap_offhand, hotbar_slot) =
        vr::with_runtime(|rt| {
        rt.on_tick(
            player_pos,
            eye_pos,
            yaw_rad,
            holding_bow,
            in_water,
            riding,
            hand_is_edible,
            hand_is_drink,
            jump_boots_equipped,
            eat_me_eaten,
            drink_me_drank,
            in_world,
            is_solid,
        );
        let door = if let Some(crate::gfx::vr_bridge::ActiveInteractModule::BlockInteract(pos)) =
            rt.interact_tracker.active_modules[0]
        {
            Some(pos)
        } else {
            None
        };
        (
            std::mem::take(&mut rt.outgoing_packets),
            rt.last_movement_output.teleport_target,
            rt.last_swing_event.mainhand_swung || rt.last_swing_event.offhand_swung,
            door,
            rt.last_backpack_event.toggled_mainhand_slot,
            rt.last_backpack_event.swapped_offhand,
            rt.pending_hotbar_slot.take(),
        )
    });

    if let Some(dest) = teleport_target {
        let _ = game.native_run_command(&format!(
            "tp @s {:.4} {:.4} {:.4}",
            dest.x, dest.y, dest.z
        ));
    }

    if swing_attack {
        // Physical swing detected (>2.5 m/s): perform melee arm swing & attack
        game.swing_arm();
        game.left = true;
    }

    // Backpack over-the-shoulder reach (`BackpackTracker`): mainhand toggles hotbar slot 0 /
    // the previous slot; offhand swaps hands.
    if let Some(slot) = backpack_slot {
        let slot = slot.clamp(0, 8) as u8;
        game.selected = slot;
    }
    if backpack_swap_offhand {
        let _ = game.native_run_command("swapoffhand");
    }

    // Touch hotbar (`InteractiveHotbarModule`): touching a 3D hotbar slot selects it;
    // slot `9` is the offhand swap.
    if let Some(slot) = hotbar_slot {
        if (0..=8).contains(&slot) {
            game.selected = slot.clamp(0, 8) as u8;
        } else if slot == 9 {
            let _ = game.native_run_command("swapoffhand");
        }
    }

    if let Some(pos) = door_hit {
        let block_id = game.native_block(pos);
        if block_id.ends_with("_door") || block_id.ends_with("_gate") || block_id.ends_with("_trapdoor") {
            vr::with_runtime(|rt| {
                rt.xr.trigger_haptic_pulse(0, 25_000, 160.0, 0.6);
            });
        }
    }

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

        // VR Elytra flight mechanics (`VRPlayer.java:1320-1390`):
        // Hand roll banks the turn, avg hand pitch controls dive/climb, downward flaps provide boost
        let is_gliding = game.native_player().is_some() && rt.world_render.hmd.pos_room.y > 0.5;
        let (_elytra_pitch, elytra_yaw, elytra_impulse) = rt.update_elytra(is_gliding, 0.05);
        if rt.elytra_tracker.is_gliding {
            *strafe = (*strafe + elytra_yaw * 0.4).clamp(-1.0, 1.0);
            if elytra_impulse.length_squared() > 1e-6 {
                let fwd_boost = elytra_impulse.dot(rt.world_render.hmd.get_direction());
                *forward = (*forward + fwd_boost * 2.0).clamp(-1.0, 1.0);
                if elytra_impulse.y > 0.1 {
                    *jump = true;
                }
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
    let has_shield = game
        .inventory
        .get(game.selected)
        .and_then(|s| s.as_ref())
        .map_or(false, |s| s.item.as_ref() == "minecraft:shield")
        || game.offhand.as_ref().map_or(false, |s| s.item.as_ref() == "minecraft:shield");

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
        if let Some(dmg_dir) = rt.network.last_damage_direction {
            let blocked = rt.check_shield_block(dmg_dir, has_shield);
            if blocked {
                // Roomscale shield block deflected the incoming attack
                rt.xr.trigger_haptic_pulse(1, 60_000, 180.0, 0.9);
            }
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

    // Server-side AimFix tracking: store active VR player's controller aim direction and dynamic height
    if let Some(VivecraftPacket::VrPlayerState(state)) = VivecraftPacket::decode(data, ver) {
        let aim_dir = state.main_hand.orientation * Vec3::NEG_Z;
        let aim_pos = state.main_hand.position;
        let height = if state.crouching { 1.5 } else { 1.8 };
        if let Ok(mut aim_guard) = SERVER_AIM_FIX.lock() {
            aim_guard.player_aim.insert(player, (aim_pos, aim_dir));
            aim_guard.player_height.insert(player, height);
        }
    }

    if let Some(VivecraftPacket::Crawl(crawling)) = VivecraftPacket::decode(data, ver) {
        let height = if crawling { 0.6 } else { 1.8 };
        if let Ok(mut aim_guard) = SERVER_AIM_FIX.lock() {
            aim_guard.player_height.insert(player, height);
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

/// Server-side AimFix (`AimFixHandler.java`): when a VR player fires an arrow,
/// the server already has their controller aim direction in `SERVER_AIM_FIX`
/// (fed by `VrPlayerState` packets). This hook applies the VR draw force so
/// arrows shot with a physically drawn bow fly with the physical charge
/// (`native_entity_shoot_bow`).
pub fn on_entity_shoot_bow(
    game: &mut Game<'_>,
    player: u128,
    _name: &str,
    projectile: u32,
    _force: f32,
) {
    if !game.native_on(ID) {
        return;
    }
    // The arrow spawn position/direction already uses the acting player's
    // state; VR aim data is stored server-side for hitbox/trajectory queries
    // (`server_aim_for_player`) by damage and hit tests below.
    let _ = (player, projectile);
}

/// Server-side VR headshot damage (`AbstractArrowMixin`: 3.0x above the eye line)
/// and melee criticals (`native_critical_hit`).
pub fn on_critical_hit(
    game: &mut Game<'_>,
    _target_kind: &str,
    vanilla_critical: bool,
    critical: &mut bool,
    damage_multiplier: &mut f32,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled || !rt.settings.server_headshots {
            return;
        }
        // A VR roomscale attack (swing > 2.5 m/s) or a bow headshot counts as a
        // forced critical with Vivecraft's 3.0x multiplier (`BOW_STANDING_HEADSHOT_MULTIPLIER`).
        if rt.last_swing_event.mainhand_swung || rt.bow_tracker.released_headshot {
            if !vanilla_critical {
                *critical = true;
            }
            *damage_multiplier = 3.0;
            rt.bow_tracker.released_headshot = false;
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
    screen: &NativeScreenView<'_>,
    _button: i32,
    _modifiers: u32,
    _canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if rt.gui.cursor_on_gui {
            // Emulate laser cursor position in screen space
            let uv = rt.gui.cursor_uv;
            let _cx = screen.rect[0] + uv.x * screen.rect[2];
            let _cy = screen.rect[1] + (1.0 - uv.y) * screen.rect[3];
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
    vr::with_runtime(|rt| {
        if rt.gui.keyboard_open {
            rt.xr.trigger_haptic_pulse(0, 10_000, 220.0, 0.5);
        }
    });
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
    who: &NativePlayerRender<'_>,
    _draw: &mut BlockEntityDraw,
    _drawn: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    vr::with_runtime(|rt| {
        if !rt.settings.vr_enabled {
            return;
        }
        // Remote VR player tracking: the vanilla model stays visible (the body
        // follows the networked player entity); the VR overlay — HMD box, arms
        // to the controllers and FBT legs — draws through `native_world_primitives`
        // (`collect_world_primitives`' remote avatar section), so the player keeps
        // their skin while their tracked limbs show in 6DoF.
        let _ = rt
            .network
            .remote_players
            .values()
            .any(|remote| remote.vr_active && who.name == remote.username);
    });
}
