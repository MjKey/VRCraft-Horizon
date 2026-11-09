use glam::{Mat4, Quat, Vec2, Vec3};

use crate::settings::{FreeMoveOrientation, LocomotionMode, TurnMode, VrSettings};
use crate::vr_data::{Vec3History, VrData};
use crate::xr::{ControllerState, HapticPulse};

/// Parabolic teleport arc engine ported from Vivecraft's `TeleportTracker.java`.
#[derive(Debug, Clone)]
pub struct TeleportTracker {
    /// Energy pool (`0.0..=100.0`) used when survival teleport limits are enabled.
    pub movement_teleport_timer: f32,
    /// Sampled world-space points along the parabolic teleport arc (up to 50 points).
    pub arc_points: Vec<Vec3>,
    /// Validated landing destination in world space, if the arc hit a walkable surface.
    pub destination: Option<Vec3>,
    /// Normal of the surface hit by the arc.
    pub hit_normal: Vec3,
    /// Progress (`0.0..=1.0`) while holding the teleport button.
    pub movement_teleport_progress: f32,
    /// Last interpolated arc endpoint.
    pub last_teleport_arc_display: Vec3,
    /// Whether the teleport trigger was held on the previous tick.
    prev_teleport_pressed: bool,
}

impl Default for TeleportTracker {
    fn default() -> Self {
        Self {
            movement_teleport_timer: 100.0,
            arc_points: Vec::with_capacity(50),
            destination: None,
            hit_normal: Vec3::Y,
            movement_teleport_progress: 0.0,
            last_teleport_arc_display: Vec3::ZERO,
            prev_teleport_pressed: false,
        }
    }
}

impl TeleportTracker {
    pub const MAX_ARC_STEPS: usize = 50;
    pub const GRAVITY_PER_STEP: f32 = 0.098;
    pub const BASE_ARC_SPEED: f32 = 0.5;

    pub fn new() -> Self {
        Self::default()
    }

    /// Simulates the parabolic teleport arc in world space (`50` steps, `g = 0.098`, `v0 = 0.5 * world_scale`)
    /// and returns `Some(destination)` on the frame the player releases the teleport button.
    pub fn update<F>(
        &mut self,
        world_post: &VrData,
        teleport_pressed: bool,
        mut is_solid_block: F,
    ) -> Option<Vec3>
    where
        F: FnMut(i32, i32, i32) -> bool,
    {
        self.movement_teleport_timer = (self.movement_teleport_timer + 1.25).min(100.0);

        if teleport_pressed {
            self.movement_teleport_progress = (self.movement_teleport_progress + 0.15).min(1.0);
            self.trace_parabolic_arc(world_post, &mut is_solid_block);
            self.prev_teleport_pressed = true;
            None
        } else {
            let fired = if self.prev_teleport_pressed {
                self.destination.take()
            } else {
                None
            };
            self.prev_teleport_pressed = false;
            self.movement_teleport_progress = 0.0;
            self.arc_points.clear();
            self.destination = None;
            fired
        }
    }

    /// Traces the parabolic curve from controller 1 (or controller 0) exactly as `TeleportTracker.updateTeleportDestinations`.
    pub fn trace_parabolic_arc<F>(&mut self, world_post: &VrData, is_solid_block: &mut F)
    where
        F: FnMut(i32, i32, i32) -> bool,
    {
        self.arc_points.clear();
        self.destination = None;

        let ctrl = &world_post.c1;
        let start = ctrl.get_position_f();
        let dir = ctrl.get_direction();
        let scale = world_post.world_scale.clamp(0.25, 4.0);

        let speed = Self::BASE_ARC_SPEED * scale * (0.65 + 0.35 * self.movement_teleport_progress);
        let mut velocity = dir * speed;
        let gravity = Vec3::new(0.0, -Self::GRAVITY_PER_STEP * scale, 0.0);

        let mut curr = start;
        self.arc_points.push(curr);

        for _ in 0..Self::MAX_ARC_STEPS {
            let next = curr + velocity;
            let bx = next.x.floor() as i32;
            let by = next.y.floor() as i32;
            let bz = next.z.floor() as i32;

            if is_solid_block(bx, by, bz) {
                self.arc_points.push(next);
                self.last_teleport_arc_display = next;

                let head_clear =
                    !is_solid_block(bx, by + 1, bz) && !is_solid_block(bx, by + 2, bz);
                if head_clear {
                    self.destination = Some(Vec3::new(next.x, (by + 1) as f32, next.z));
                    self.hit_normal = Vec3::Y;
                }
                break;
            }

            self.arc_points.push(next);
            self.last_teleport_arc_display = next;
            curr = next;
            velocity += gravity * 0.5;
        }
    }
}

/// Physical crouch tracker ported from `SneakTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct SneakTracker {
    pub sneak_override: bool,
    pub sneak_counter: i32,
    pub crouch_offset: f32,
}

impl SneakTracker {
    pub fn update(&mut self, settings: &VrSettings, hmd_pivot_y: f32) -> bool {
        if settings.seated || !settings.realistic_sneak_enabled {
            self.sneak_override = false;
            self.crouch_offset = 0.0;
            return false;
        }
        let delta = settings.calibrated_height - hmd_pivot_y;
        let threshold = 0.42_f32;
        self.sneak_override = delta >= threshold;
        self.crouch_offset = if self.sneak_override {
            (delta - threshold).clamp(0.0, 0.5)
        } else {
            0.0
        };
        self.sneak_override
    }
}

/// Physical prone/crawl tracker ported from `CrawlTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct CrawlTracker {
    pub crawling: bool,
    pub was_crawling: bool,
    pub crawl_threshold_m: f32,
}

impl CrawlTracker {
    pub const DEFAULT_THRESHOLD_M: f32 = 0.75;

    pub fn new() -> Self {
        Self {
            crawling: false,
            was_crawling: false,
            crawl_threshold_m: Self::DEFAULT_THRESHOLD_M,
        }
    }

    /// Updates crawl state and returns `Some(crawling)` when the state changed (`CrawlPayloadC2S`).
    pub fn update(&mut self, settings: &VrSettings, hmd_pivot_y: f32, server_allows_crawling: bool) -> Option<bool> {
        if settings.seated || !server_allows_crawling {
            let changed = self.crawling;
            self.crawling = false;
            self.was_crawling = false;
            return if changed { Some(false) } else { None };
        }
        self.crawling = hmd_pivot_y <= self.crawl_threshold_m;
        if self.crawling != self.was_crawling {
            self.was_crawling = self.crawling;
            Some(self.crawling)
        } else {
            None
        }
    }
}

/// Physical jump & Climbey Jump-Boots tracker ported from `JumpTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct JumpTracker {
    pub latch_start_room: [Vec3; 2],
    pub latch_start_origin: [Vec3; 2],
    pub latch_start_player: [Vec3; 2],
    pub c0_latched: bool,
    pub c1_latched: bool,
}

impl JumpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates physical upward jump gesture and Climbey Jump-Boots latch-and-throw (`JumpTracker.activeProcess`).
    /// Returns `(jumped, optional_throw_velocity, optional_haptic)`.
    pub fn update(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        hmd_history: &Vec3History,
        c0_history: &Vec3History,
        c1_history: &Vec3History,
        climbey_jump_pressed: bool,
        climbey_boots_equipped: bool,
    ) -> (bool, Option<Vec3>, Option<HapticPulse>) {
        if settings.seated || !settings.realistic_jump_enabled {
            self.c0_latched = false;
            self.c1_latched = false;
            return (false, None, None);
        }

        if climbey_boots_equipped {
            let mut haptic = None;
            let mut throw_vel = None;
            let mut jumped = false;

            let now = [room_pre.c0.pos_room, room_pre.c1.pos_room];
            let was_latched = [self.c0_latched, self.c1_latched];

            if !climbey_jump_pressed && (self.c0_latched || self.c1_latched) {
                // Release Climbey jump-boots throw!
                let mut net = Vec3::ZERO;
                let mut speed = 0.0_f32;
                let mut count = 0.0_f32;
                if self.c0_latched {
                    let vel = c0_history.velocity(0.3);
                    net += vel * 0.3;
                    speed += vel.length();
                    count += 1.0;
                }
                if self.c1_latched {
                    let vel = c1_history.velocity(0.3);
                    net += vel * 0.3;
                    speed += vel.length();
                    count += 1.0;
                }
                if count > 0.0 {
                    net /= count;
                    speed /= count;
                }
                let mut movement = net * (0.33 * speed);
                let limit = 0.66_f32;
                if movement.length() > limit {
                    movement = movement.normalize_or_zero() * limit;
                }
                let hmd_vel = hmd_history.velocity(0.1);
                if movement.y < -0.15 && hmd_vel.y > 0.1 {
                    let world_impulse = Quat::from_rotation_y(world_post.rotation_radians) * (-movement);
                    throw_vel = Some(world_impulse);
                    jumped = true;
                    haptic = Some(HapticPulse {
                        controller: 0,
                        duration_micros: 1_000,
                        frequency_hz: 200.0,
                        amplitude: 0.85,
                    });
                }
                self.c0_latched = false;
                self.c1_latched = false;
            } else if climbey_jump_pressed {
                for c in 0..2 {
                    if !was_latched[c] {
                        self.latch_start_room[c] = now[c];
                        haptic = Some(HapticPulse {
                            controller: c,
                            duration_micros: 400,
                            frequency_hz: 160.0,
                            amplitude: 0.45,
                        });
                    }
                }
                self.c0_latched = true;
                self.c1_latched = true;
            }
            return (jumped, throw_vel, haptic);
        }

        // Standard roomscale physical jump gesture
        let hmd_vel_y = hmd_history.velocity(0.2).y;
        let c0_vel_y = c0_history.velocity(0.2).y;
        let c1_vel_y = c1_history.velocity(0.2).y;
        let physical_jump = hmd_vel_y > 0.9 && c0_vel_y > 1.0 && c1_vel_y > 1.0;
        (physical_jump, None, None)
    }
}

/// Roomscale Ladder/Vine & Climbey Claws climbing tracker ported from `ClimbTracker.java`.
#[derive(Debug, Clone)]
pub struct ClimbTracker {
    pub latched: [bool; 2],
    pub was_latched: [bool; 2],
    pub in_block: [bool; 2],
    pub was_in_block: [bool; 2],
    pub was_button: [bool; 2],
    pub latch_start: [Vec3; 2],
    pub latch_start_room: [Vec3; 2],
    pub latch_start_controller: Option<usize>,
    pub gravity_override: bool,
}

impl Default for ClimbTracker {
    fn default() -> Self {
        Self {
            latched: [false; 2],
            was_latched: [false; 2],
            in_block: [false; 2],
            was_in_block: [false; 2],
            was_button: [false; 2],
            latch_start: [Vec3::ZERO; 2],
            latch_start_room: [Vec3::ZERO; 2],
            latch_start_controller: None,
            gravity_override: false,
        }
    }
}

impl ClimbTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_grabbing_ladder(&self) -> bool {
        self.latched[0] || self.latched[1]
    }

    /// Updates ladder/vine & Climbey grabbing (`ClimbTracker.activeProcess`).
    /// Returns `(is_climbing, pull_delta_world, haptic)`.
    pub fn update<F>(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        controllers: &[ControllerState; 2],
        mut is_climbable_block: F,
    ) -> (bool, Vec3, Option<HapticPulse>)
    where
        F: FnMut(i32, i32, i32) -> bool,
    {
        if settings.seated || !settings.realistic_climb_enabled {
            self.latched = [false; 2];
            self.latch_start_controller = None;
            self.gravity_override = false;
            return (false, Vec3::ZERO, None);
        }

        let mut haptic = None;
        let mut nope = false;

        for c in 0..2 {
            let ctrl_world = world_post.get_controller(c).get_position_f();
            let ctrl_dir = world_post.get_controller(c).get_direction();
            let near_pos = ctrl_world - ctrl_dir * 0.2;

            let bx1 = ctrl_world.x.floor() as i32;
            let by1 = ctrl_world.y.floor() as i32;
            let bz1 = ctrl_world.z.floor() as i32;

            let bx2 = near_pos.x.floor() as i32;
            let by2 = near_pos.y.floor() as i32;
            let bz2 = near_pos.z.floor() as i32;

            self.in_block[c] = is_climbable_block(bx1, by1, bz1) || is_climbable_block(bx2, by2, bz2);
            let grab_pressed = controllers[c].squeeze > 0.5 || controllers[c].trigger > 0.5;
            let button = self.in_block[c] && (settings.climbing_auto_grab || grab_pressed);

            self.was_latched[c] = self.latched[c];
            if !button && self.latched[c] {
                self.latched[c] = false;
            }

            if !self.latched[c] && !nope && self.in_block[c] {
                if !self.was_in_block[c] {
                    haptic = Some(HapticPulse {
                        controller: c,
                        duration_micros: 750,
                        frequency_hz: 160.0,
                        amplitude: 0.45,
                    });
                }

                if (!self.was_in_block[c] && button) || (!self.was_button[c] && button) {
                    self.latch_start[c] = ctrl_world;
                    self.latch_start_room[c] = room_pre.get_controller(c).pos_room;
                    self.latch_start_controller = Some(c);
                    self.latched[c] = true;
                    if c == 0 {
                        self.latched[1] = false;
                        nope = true;
                    } else {
                        self.latched[0] = false;
                    }
                    haptic = Some(HapticPulse {
                        controller: c,
                        duration_micros: 2_000,
                        frequency_hz: 200.0,
                        amplitude: 0.85,
                    });
                }
            }

            self.was_button[c] = button;
            self.was_in_block[c] = self.in_block[c];
        }

        if !self.latched[0] && !self.latched[1] {
            self.latch_start_controller = None;
            self.gravity_override = false;
            return (false, Vec3::ZERO, haptic);
        }

        self.gravity_override = true;
        let Some(active_c) = self.latch_start_controller else {
            return (false, Vec3::ZERO, haptic);
        };

        let now_room = room_pre.get_controller(active_c).pos_room;
        let room_delta = now_room - self.latch_start_room[active_c];
        self.latch_start_room[active_c] = now_room;

        let world_delta = Quat::from_rotation_y(world_post.rotation_radians)
            * (room_delta * world_post.world_scale);
        (true, -world_delta, haptic)
    }
}

/// Physical breaststroke swimming tracker ported from `SwimTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct SwimTracker {
    pub motion: Vec3,
    pub last_dist: f32,
}

impl SwimTracker {
    pub const FRICTION: f32 = 0.9;
    pub const RISE_SPEED: f32 = 0.005;
    pub const SWIM_SPEED: f32 = 1.3;

    pub fn new() -> Self {
        Self::default()
    }

    /// Computes breaststroke propulsion velocity when in water (`SwimTracker.activeProcess`).
    pub fn update(&mut self, settings: &VrSettings, world_post: &VrData, in_water: bool) -> Vec3 {
        if settings.seated || !settings.realistic_swim_enabled || !in_water {
            self.motion *= Self::FRICTION;
            return Vec3::ZERO;
        }

        let ctrl_r = world_post.c0.get_position_f();
        let ctrl_l = world_post.c1.get_position_f();
        let middle = (ctrl_l - ctrl_r) * 0.5 + ctrl_r;

        let pivot = world_post.get_head_pivot();
        let hmd_pos = Vec3::new(pivot.x as f32, (pivot.y - 0.3) as f32, pivot.z as f32);

        let move_dir = ((middle - hmd_pos).normalize_or_zero() + world_post.hmd.get_direction()) * 0.5;
        let ctrl_dir = (world_post.c0.get_custom_vector(Vec3::Z)
            + world_post.c1.get_custom_vector(Vec3::Z))
            * 0.5;

        let dir_factor = (ctrl_dir + move_dir).length() * 0.5;
        let distance = hmd_pos.distance(middle);
        let dist_delta = self.last_dist - distance;

        if dist_delta > 0.0 {
            let velocity = move_dir * (dist_delta * Self::SWIM_SPEED * dir_factor * 0.15);
            self.motion += velocity;
        }

        self.last_dist = distance;
        let push = self.motion;
        self.motion *= Self::FRICTION;
        push
    }
}

/// Arm-swing Run-In-Place locomotion tracker ported from `RunTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct RunTracker {
    pub direction: Vec3,
    pub speed: f32,
}

impl RunTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates arm-swing speed and direction when `FreeMoveOrientation::RunInPlace` is active.
    pub fn update(
        &mut self,
        settings: &VrSettings,
        world_post: &VrData,
        c0_history: &Vec3History,
        c1_history: &Vec3History,
    ) -> Option<(f32, f32)> {
        if settings.seated || settings.free_move_orientation != FreeMoveOrientation::RunInPlace {
            self.speed = 0.0;
            return None;
        }

        let c0_move = c0_history.velocity(0.33).length();
        let c1_move = c1_history.velocity(0.33).length();
        let v = c0_move.min(c1_move);
        if v <= 0.1 {
            self.speed = 0.0;
            return None;
        }

        if c0_move - c1_move > 0.5 || c1_move - c0_move > 0.5 {
            self.speed = 0.0;
            return None;
        }

        let c0_dir = world_post.c0.get_direction();
        let c1_dir = world_post.c1.get_direction();
        let blended = c0_dir * (c0_move / (c0_move + c1_move)) + c1_dir * (c1_move / (c0_move + c1_move));
        self.direction = blended;
        self.speed = (c0_move + c1_move) * 0.5 * 1.3;
        let yaw_rad = (-blended.x).atan2(blended.z);
        Some((yaw_rad, self.speed.clamp(0.0, 1.5)))
    }
}

/// Two-handed boat oar rowing tracker ported from `RowTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct RowTracker {
    pub loar: f32,
    pub roar: f32,
    pub foar: f32,
    pub forces: [f32; 2],
    last_underwater_pos: [Vec3; 2],
}

impl RowTracker {
    pub const TRANSMISSION_EFFICIENCY: f32 = 0.9;
    pub const ROW_SCALE: f32 = 0.05;
    pub const MAX_SPEED: f32 = 0.5;

    pub fn new() -> Self {
        Self::default()
    }

    /// Computes boat rowing forces and steering yaw delta (`RowTracker.activeProcess`).
    pub fn update(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        boat_pos: Vec3,
        boat_forward: Vec3,
        in_boat: bool,
    ) -> (Vec3, f32) {
        if settings.seated || !settings.realistic_row_enabled || !in_boat {
            self.loar = 0.0;
            self.roar = 0.0;
            self.foar = 0.0;
            self.forces = [0.0; 2];
            return (Vec3::ZERO, 0.0);
        }

        let c0_pad = room_pre.c0.pos_room + room_pre.c0.dir_room * 0.4;
        let c1_pad = room_pre.c1.pos_room + room_pre.c1.dir_room * 0.4;
        let pads = [c0_pad, c1_pad];

        for arm in 0..2 {
            let world_pad = world_post.get_controller(arm).get_position_f()
                + world_post.get_controller(arm).get_direction() * 0.4;
            // Consider paddle underwater when below boat waterline
            if world_pad.y <= boat_pos.y + 0.15 {
                let move_vec = -(pads[arm] - self.last_underwater_pos[arm]);
                let world_move = Quat::from_rotation_y(world_post.rotation_radians) * move_vec;
                let f = (world_move.dot(boat_forward.normalize_or_zero())
                    * Self::TRANSMISSION_EFFICIENCY
                    / 5.0)
                    .max(0.0);
                self.forces[arm] = f;
            } else {
                self.forces[arm] = 0.0;
            }
            self.last_underwater_pos[arm] = pads[arm];
        }

        self.roar = self.forces[0];
        self.loar = self.forces[1];
        self.foar = (self.roar + self.loar).min(Self::MAX_SPEED);
        let push = boat_forward.normalize_or_zero() * (self.foar * Self::ROW_SCALE);
        let yaw_delta_deg = (self.loar - self.roar) * 12.0;
        (push, yaw_delta_deg)
    }
}

/// Vehicle mounting, rotation, and roomscale dismount tracker ported from `VehicleTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct VehicleTracker {
    pub premount_pos_room: Vec3,
    pub vehicle_initial_rotation: f32,
    pub dismount_delay_ticks: u32,
}

impl VehicleTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks whether the player physically walked > 1.0m away from the vehicle seat to dismount (`canRoomscaleDismount`).
    pub fn should_roomscale_dismount(&self, settings: &VrSettings, room_pre: &VrData, riding: bool) -> bool {
        if settings.seated || !riding || self.dismount_delay_ticks > 0 {
            return false;
        }
        let curr = Vec2::new(room_pre.hmd.pos_room.x, room_pre.hmd.pos_room.z);
        let start = Vec2::new(self.premount_pos_room.x, self.premount_pos_room.z);
        curr.distance(start) > 1.0
    }
}

/// Horse reins & whip tracker ported from `HorseTracker.java`.
#[derive(Debug, Clone, Default)]
pub struct HorseTracker {
    pub speed_level: i32,
    pub boost_cooldown: u32,
    pub last_ctrl_y: [f32; 2],
    pub reins_steer_yaw: f32,
}

impl HorseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates horse reins steering and downward whip gestures. Returns `(steer_yaw_delta_deg, speed_boost)`.
    pub fn update(
        &mut self,
        _settings: &VrSettings,
        world_post: &VrData,
        controllers: &[ControllerState; 2],
    ) -> (f32, bool) {
        if self.boost_cooldown > 0 {
            self.boost_cooldown -= 1;
        }

        // 1. Reins steering: lateral difference between left and right hand positions
        let c0_pos = world_post.c0.get_position_f();
        let c1_pos = world_post.c1.get_position_f();
        let reins_vector = c0_pos - c1_pos;
        let steer_factor = (reins_vector.z * 15.0).clamp(-45.0, 45.0);
        self.reins_steer_yaw = steer_factor;

        // 2. Whip gesture: rapid downward motion of either hand
        let mut boost = false;
        let c0_y = controllers[0].aim_position.y;
        let c1_y = controllers[1].aim_position.y;
        let vel0_y = (c0_y - self.last_ctrl_y[0]) / 0.05;
        let vel1_y = (c1_y - self.last_ctrl_y[1]) / 0.05;
        self.last_ctrl_y[0] = c0_y;
        self.last_ctrl_y[1] = c1_y;

        if (vel0_y < -1.8 || vel1_y < -1.8) && self.boost_cooldown == 0 {
            boost = true;
            self.boost_cooldown = 20; // 1 second cooldown
            self.speed_level = (self.speed_level + 1).min(3);
        }

        (steer_factor, boost)
    }
}

/// Complete VR Locomotion state aggregating all 10 ViveCraft locomotion trackers:
/// `TeleportTracker`, `SneakTracker`, `CrawlTracker`, `JumpTracker`, `ClimbTracker`,
/// `SwimTracker`, `RunTracker`, `RowTracker`, `VehicleTracker`, and `HorseTracker`.
#[derive(Debug, Clone)]
pub struct LocomotionState {
    pub teleport: TeleportTracker,
    pub sneak_tracker: SneakTracker,
    pub crawl_tracker: CrawlTracker,
    pub jump_tracker: JumpTracker,
    pub climb_tracker: ClimbTracker,
    pub swim_tracker: SwimTracker,
    pub run_tracker: RunTracker,
    pub row_tracker: RowTracker,
    pub vehicle_tracker: VehicleTracker,
    pub horse_tracker: HorseTracker,
    /// Previous room-space HMD XZ position for physical roomscale walking delta.
    pub last_room_hmd_xz: Vec2,
    /// Whether the snap-turn stick was deflected on the previous frame (edge trigger).
    pub snap_turn_latched: bool,
    /// Whether physical crouching is currently active (`HMD.y < calibrate_height * crouch_threshold`).
    pub physical_sneaking: bool,
    /// Whether physical upward jump gesture triggered this tick.
    pub physical_jumping: bool,
    /// Whether ladder climbing is active with climbing claws / controller grab.
    pub climbing: bool,
    /// Whether roomscale prone crawling is active (`CrawlTracker`).
    pub crawling: bool,
    /// Calibrated standing height in meters.
    pub calibrated_height_m: f32,
    /// Room-space pose history buffers for physical jump / velocity gestures (`JumpTracker` & `RunTracker`).
    pub hmd_history: Vec3History,
    pub c0_history: Vec3History,
    pub c1_history: Vec3History,
    pub elapsed_time_sec: f64,
}

impl Default for LocomotionState {
    fn default() -> Self {
        Self {
            teleport: TeleportTracker::new(),
            sneak_tracker: SneakTracker::default(),
            crawl_tracker: CrawlTracker::new(),
            jump_tracker: JumpTracker::new(),
            climb_tracker: ClimbTracker::new(),
            swim_tracker: SwimTracker::new(),
            run_tracker: RunTracker::new(),
            row_tracker: RowTracker::new(),
            vehicle_tracker: VehicleTracker::new(),
            horse_tracker: HorseTracker::new(),
            last_room_hmd_xz: Vec2::ZERO,
            snap_turn_latched: false,
            physical_sneaking: false,
            physical_jumping: false,
            climbing: false,
            crawling: false,
            calibrated_height_m: 1.62,
            hmd_history: Vec3History::new(90),
            c0_history: Vec3History::new(90),
            c1_history: Vec3History::new(90),
            elapsed_time_sec: 0.0,
        }
    }
}

/// Movement output applied to Horizon's `native_movement_input` and `native_tick` hooks.
#[derive(Debug, Clone, Copy, Default)]
pub struct VrMovementOutput {
    /// Additional forward/strafe input in `[-1.0, 1.0]` relative to player yaw.
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    /// Roomscale world-space XZ delta to shift player / origin.
    pub roomscale_world_delta: Vec3,
    /// Optional teleport destination in world space.
    pub teleport_target: Option<Vec3>,
    /// Physical velocity impulse from `SwimTracker`, `ClimbTracker`, `RowTracker`, or `JumpTracker`.
    pub velocity_impulse: Vec3,
    /// Whether the player is currently grabbing a climbable block (`ClimbingPayloadC2S`).
    pub climbing: bool,
    /// If `Some(crawling)`, the player's crawl state changed this tick (`CrawlPayloadC2S`).
    pub crawl_state_changed: Option<bool>,
    /// Optional haptic pulse triggered by climbing or jump-boots.
    pub haptic: Option<HapticPulse>,
}

impl LocomotionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates VR world yaw (`rotation_radians`) based on the right controller thumbstick X axis
    /// and the configured `TurnMode` (`Snap` or `Smooth`).
    pub fn update_turning(
        &mut self,
        settings: &VrSettings,
        right_stick_x: f32,
        dt_seconds: f32,
        rotation_radians: &mut f32,
    ) {
        let deadzone = 0.45;
        match settings.turn_mode {
            TurnMode::Snap => {
                if right_stick_x.abs() >= deadzone {
                    if !self.snap_turn_latched {
                        self.snap_turn_latched = true;
                        let step = settings.snap_turn_degrees.to_radians();
                        if right_stick_x > 0.0 {
                            *rotation_radians -= step;
                        } else {
                            *rotation_radians += step;
                        }
                    }
                } else {
                    self.snap_turn_latched = false;
                }
            }
            TurnMode::Smooth => {
                self.snap_turn_latched = false;
                if right_stick_x.abs() > 0.12 {
                    let angular_speed = settings.smooth_turn_rate.to_radians();
                    *rotation_radians -= right_stick_x * angular_speed * dt_seconds.clamp(0.0, 0.1);
                }
            }
        }
        *rotation_radians = rotation_radians.rem_euclid(std::f32::consts::TAU);
    }

    /// Computes VR locomotion output for the current tick, combining:
    /// - Left controller thumbstick smooth movement (oriented by Controller 0, HMD, Waist, or RunInPlace)
    /// - Parabolic teleport arc release (`TeleportTracker`)
    /// - Physical roomscale walking delta with DDA ledge step-up and anti-wall-clipping
    /// - Physical crouching (`SneakTracker`), prone crawling (`CrawlTracker`), and physical jump gesture (`JumpTracker`)
    /// - Swimming strokes (`SwimTracker`), boat rowing (`RowTracker`), vehicle dismount, and horse control (`HorseTracker`)
    pub fn update_tick<F>(
        &mut self,
        settings: &VrSettings,
        room_pre: &VrData,
        world_post: &VrData,
        left_ctrl: &ControllerState,
        right_ctrl: &ControllerState,
        player_yaw_rad: f32,
        in_water: bool,
        riding: bool,
        mut is_solid_block: F,
    ) -> VrMovementOutput
    where
        F: FnMut(i32, i32, i32) -> bool,
    {
        let mut out = VrMovementOutput::default();
        self.elapsed_time_sec += 0.05;
        self.hmd_history
            .add(self.elapsed_time_sec, room_pre.hmd.pos_room);
        self.c0_history
            .add(self.elapsed_time_sec, room_pre.c0.pos_room);
        self.c1_history
            .add(self.elapsed_time_sec, room_pre.c1.pos_room);

        // 1. Physical roomscale HMD XZ delta in world space with anti-wall-clipping
        let curr_xz = Vec2::new(room_pre.hmd.pos_room.x, room_pre.hmd.pos_room.z);
        let room_delta_xz = curr_xz - self.last_room_hmd_xz;
        self.last_room_hmd_xz = curr_xz;

        if !settings.seated && room_delta_xz.length_squared() < 0.25 {
            let rot = Mat4::from_rotation_y(world_post.rotation_radians);
            let raw_delta = rot.transform_vector3(Vec3::new(
                room_delta_xz.x * world_post.world_scale,
                0.0,
                room_delta_xz.y * world_post.world_scale,
            ));
            let player_pos_f = Vec3::new(
                world_post.origin.x as f32,
                world_post.origin.y as f32,
                world_post.origin.z as f32,
            );
            out.roomscale_world_delta = resolve_roomscale_voxel_collision(
                player_pos_f,
                raw_delta,
                &mut is_solid_block,
            );
        }

        // 2. Swimming tracker: breaststroke propulsion in water
        if in_water {
            let swim_vel = self.swim_tracker.update(settings, world_post, in_water);
            out.velocity_impulse += swim_vel;
        }

        // 3. Vehicle & riding tracker: auto-dismount when stepping away, horse reins, and boat rowing
        let controllers_arr = [*left_ctrl, *right_ctrl];
        if riding {
            if self.vehicle_tracker.should_roomscale_dismount(settings, room_pre, riding) {
                out.sneak = true;
            }
            let (_horse_steer, horse_boost) = self.horse_tracker.update(settings, world_post, &controllers_arr);
            if horse_boost {
                out.sprint = true;
            }
            let boat_fwd = world_post.hmd.get_direction();
            let boat_pos = world_post.hmd.get_position_f();
            let (row_push, _row_yaw) = self.row_tracker.update(
                settings,
                room_pre,
                world_post,
                boat_pos,
                boat_fwd,
                riding,
            );
            out.velocity_impulse += row_push;
        }

        // 4. Physical climb tracker (Ladders, Vines, Scaffolding, Climbey Claws)
        let (climbing, climb_pull, climb_haptic) = self.climb_tracker.update(
            settings,
            room_pre,
            world_post,
            &controllers_arr,
            &mut is_solid_block,
        );
        self.climbing = climbing;
        out.climbing = climbing;
        if climbing {
            out.velocity_impulse += climb_pull;
            if climb_haptic.is_some() {
                out.haptic = climb_haptic;
            }
        }

        // 5. Physical crouch / sneak (`SneakTracker`) & prone crawl (`CrawlTracker`)
        let hmd_height = room_pre.hmd.pos_room.y;
        if hmd_height > self.calibrated_height_m {
            self.calibrated_height_m = self.calibrated_height_m * 0.98 + hmd_height * 0.02;
        }
        let sneak_threshold = (self.calibrated_height_m * 0.68).max(0.95);
        let tracker_sneak = self.sneak_tracker.update(settings, hmd_height);
        self.physical_sneaking =
            !settings.seated && (tracker_sneak || hmd_height < sneak_threshold);
        out.sneak = self.physical_sneaking || right_ctrl.button_secondary;

        out.crawl_state_changed = self.crawl_tracker.update(settings, hmd_height, true);
        self.crawling = self.crawl_tracker.crawling;

        // 3. Physical jump (`JumpTracker`: rapid upward velocity of HMD + both controllers or Climbey Jump-Boots)
        let (jumped, throw_vel, jump_haptic) = self.jump_tracker.update(
            settings,
            room_pre,
            world_post,
            &self.hmd_history,
            &self.c0_history,
            &self.c1_history,
            right_ctrl.button_primary && left_ctrl.squeeze > 0.7,
            false,
        );
        self.physical_jumping = jumped;
        out.jump = self.physical_jumping || right_ctrl.button_primary;
        if let Some(vel) = throw_vel {
            out.velocity_impulse += vel;
        }
        if jump_haptic.is_some() {
            out.haptic = jump_haptic;
        }

        // 4. Locomotion mode: Smooth Free-Move vs Teleport
        match settings.locomotion_mode {
            LocomotionMode::Smooth => {
                if settings.free_move_orientation == FreeMoveOrientation::RunInPlace {
                    if let Some((run_yaw, run_speed)) = self.run_tracker.update(
                        settings,
                        world_post,
                        &self.c0_history,
                        &self.c1_history,
                    ) {
                        let delta_yaw = run_yaw - player_yaw_rad;
                        let (sin_y, cos_y) = delta_yaw.sin_cos();
                        out.forward = (run_speed * cos_y).clamp(-1.0, 1.0);
                        out.strafe = (run_speed * sin_y).clamp(-1.0, 1.0);
                        out.sprint = run_speed > 0.95;
                    }
                } else {
                    let stick = left_ctrl.thumbstick;
                    if stick.length_squared() > 0.02 {
                        let ref_yaw = match settings.free_move_orientation {
                            FreeMoveOrientation::Controller | FreeMoveOrientation::RunInPlace => {
                                world_post.c0.get_yaw_rad()
                            }
                            FreeMoveOrientation::Hmd => world_post.hmd.get_yaw_rad(),
                            FreeMoveOrientation::Waist => world_post
                                .waist
                                .map_or_else(|| world_post.hmd.get_yaw_rad(), |w| w.get_yaw_rad()),
                        };
                        let delta_yaw = ref_yaw - player_yaw_rad;
                        let (sin_y, cos_y) = delta_yaw.sin_cos();

                        // Rotate thumbstick vector by (ref_yaw - player_yaw) so movement aligns with VR hand/head
                        let forward = stick.y * cos_y - stick.x * sin_y;
                        let strafe = stick.x * cos_y + stick.y * sin_y;
                        let mult = settings.walk_multiplier.clamp(0.25, 2.0);
                        out.forward = (forward * mult).clamp(-1.0, 1.0);
                        out.strafe = (strafe * mult).clamp(-1.0, 1.0);
                        out.sprint = left_ctrl.squeeze > 0.75 && stick.y > 0.7;
                    }
                }
                // Still allow teleport if explicitly pressing left primary button
                out.teleport_target = self.teleport.update(
                    world_post,
                    left_ctrl.button_primary,
                    is_solid_block,
                );
            }
            LocomotionMode::Teleport => {
                let tp_pressed = left_ctrl.button_primary || left_ctrl.thumbstick.y > 0.65;
                out.teleport_target =
                    self.teleport.update(world_post, tp_pressed, is_solid_block);
            }
        }

        out
    }
}

/// Resolves physical roomscale walking against world voxels (`VRPlayer.doPlayerMoveInRoom`).
/// Performs 0.1-1.0m ledge step-up and slides along unblocked axes, preventing wall clipping.
pub fn resolve_roomscale_voxel_collision<F>(
    current_pos: Vec3,
    delta: Vec3,
    mut is_solid: F,
) -> Vec3
where
    F: FnMut(i32, i32, i32) -> bool,
{
    if delta.length_squared() < 1e-6 {
        return Vec3::ZERO;
    }

    let target = current_pos + delta;
    let feet_y = current_pos.y.floor() as i32;
    let head_y = feet_y + 1;

    // Player AABB horizontal radius: 0.3m
    let step_x = if delta.x > 0.0 { 0.3 } else if delta.x < 0.0 { -0.3 } else { 0.0 };
    let step_z = if delta.z > 0.0 { 0.3 } else if delta.z < 0.0 { -0.3 } else { 0.0 };

    let target_bx = (target.x + step_x).floor() as i32;
    let target_bz = (target.z + step_z).floor() as i32;

    let blocked_at_feet = is_solid(target_bx, feet_y, target_bz);
    let blocked_at_head = is_solid(target_bx, head_y, target_bz);

    if !blocked_at_feet && !blocked_at_head {
        return delta;
    }

    // Step-up check (0.1m - 1.0m ledge): feet block is solid, but block above head is free
    if blocked_at_feet && !blocked_at_head && !is_solid(target_bx, head_y + 1, target_bz) {
        return Vec3::new(delta.x, 1.0, delta.z);
    }

    // Wall collision! Try sliding along unblocked X or Z axis
    let slide_x_bx = (target.x + step_x).floor() as i32;
    let slide_x_bz = current_pos.z.floor() as i32;
    let x_free = !is_solid(slide_x_bx, feet_y, slide_x_bz) && !is_solid(slide_x_bx, head_y, slide_x_bz);

    let slide_z_bx = current_pos.x.floor() as i32;
    let slide_z_bz = (target.z + step_z).floor() as i32;
    let z_free = !is_solid(slide_z_bx, feet_y, slide_z_bz) && !is_solid(slide_z_bx, head_y, slide_z_bz);

    if x_free && !z_free {
        Vec3::new(delta.x, 0.0, 0.0)
    } else if z_free && !x_free {
        Vec3::new(0.0, 0.0, delta.z)
    } else {
        Vec3::ZERO
    }
}
