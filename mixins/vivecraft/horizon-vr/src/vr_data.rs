//! VR spatial pose data, room-to-world coordinate transforms, and pose history buffers
//! (ported from `VRData.java`, `Vector3fHistory.java`, and `QuaternionfHistory.java`).

use crate::settings::{AimDevice, VrSettings};
use crate::xr::{RawXrFrame, RenderEye};
use glam::{DVec3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FbtMode {
    ArmsOnly = 0,
    ArmsAndLegs = 1,
    WithJoints = 2,
}

impl FbtMode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::ArmsAndLegs,
            2 => Self::WithJoints,
            _ => Self::ArmsOnly,
        }
    }

    pub fn body_part_available(self, part: VrBodyPart) -> bool {
        match part {
            VrBodyPart::Head | VrBodyPart::MainHand | VrBodyPart::OffHand => true,
            VrBodyPart::Waist | VrBodyPart::LeftFoot | VrBodyPart::RightFoot => {
                self != Self::ArmsOnly
            }
            VrBodyPart::LeftKnee
            | VrBodyPart::RightKnee
            | VrBodyPart::LeftElbow
            | VrBodyPart::RightElbow => self == Self::WithJoints,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VrBodyPart {
    MainHand = 0,
    OffHand = 1,
    RightFoot = 2,
    LeftFoot = 3,
    Waist = 4,
    RightKnee = 5,
    LeftKnee = 6,
    RightElbow = 7,
    LeftElbow = 8,
    Head = 9,
}

impl VrBodyPart {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::MainHand,
            1 => Self::OffHand,
            2 => Self::RightFoot,
            3 => Self::LeftFoot,
            4 => Self::Waist,
            5 => Self::RightKnee,
            6 => Self::LeftKnee,
            7 => Self::RightElbow,
            8 => Self::LeftElbow,
            _ => Self::Head,
        }
    }
}

/// Time-stamped 3D vector history buffer (`Vector3fHistory.java`).
#[derive(Clone, Debug)]
pub struct Vec3History {
    samples: VecDeque<(f64, Vec3)>,
    max_samples: usize,
}

impl Default for Vec3History {
    fn default() -> Self {
        Self::new(90)
    }
}

impl Vec3History {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn add(&mut self, time_sec: f64, value: Vec3) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back((time_sec, value));
    }

    pub fn latest(&self) -> Vec3 {
        self.samples.back().map_or(Vec3::ZERO, |&(_, v)| v)
    }

    pub fn average_position(&self, window_sec: f64) -> Vec3 {
        let Some(&(latest_t, latest_v)) = self.samples.back() else {
            return Vec3::ZERO;
        };
        if window_sec <= 0.0 {
            return latest_v;
        }
        let cutoff = latest_t - window_sec;
        let mut sum = Vec3::ZERO;
        let mut count = 0u32;
        for &(t, v) in self.samples.iter().rev() {
            if t < cutoff {
                break;
            }
            sum += v;
            count += 1;
        }
        if count == 0 {
            latest_v
        } else {
            sum / (count as f32)
        }
    }

    /// Net movement velocity in meters per second over `window_sec`.
    pub fn velocity(&self, window_sec: f64) -> Vec3 {
        let Some(&(latest_t, latest_v)) = self.samples.back() else {
            return Vec3::ZERO;
        };
        let cutoff = latest_t - window_sec;
        let mut earliest = (latest_t, latest_v);
        for &(t, v) in self.samples.iter().rev() {
            earliest = (t, v);
            if t <= cutoff {
                break;
            }
        }
        let dt = (latest_t - earliest.0) as f32;
        if dt <= 1e-4 {
            Vec3::ZERO
        } else {
            (latest_v - earliest.1) / dt
        }
    }
}

/// Pose of a single tracked VR device in room and world coordinates (`VRData.VRDevicePose`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VrDevicePose {
    /// Room-space position (meters).
    pub pos_room: Vec3,
    /// Room-space forward direction.
    pub dir_room: Vec3,
    /// Room-space orientation matrix.
    pub matrix_room: Mat4,
    /// World room origin (`VRData.origin`).
    pub origin: DVec3,
    /// World rotation around Y in radians (`VRData.rotation_radians`).
    pub rotation_radians: f32,
    /// World scale (`VRData.worldScale`).
    pub world_scale: f32,
}

impl Default for VrDevicePose {
    fn default() -> Self {
        Self {
            pos_room: Vec3::ZERO,
            dir_room: Vec3::NEG_Z,
            matrix_room: Mat4::IDENTITY,
            origin: DVec3::ZERO,
            rotation_radians: 0.0,
            world_scale: 1.0,
        }
    }
}

impl VrDevicePose {
    pub fn new(
        origin: DVec3,
        rotation_radians: f32,
        world_scale: f32,
        matrix_room: Mat4,
        pos_room: Vec3,
        dir_room: Vec3,
    ) -> Self {
        Self {
            pos_room,
            dir_room,
            matrix_room,
            origin,
            rotation_radians,
            world_scale,
        }
    }

    fn rotate_y(v: Vec3, rad: f32) -> Vec3 {
        Quat::from_rotation_y(rad) * v
    }

    /// Position of this device in world space (`VRDevicePose.getPosition`).
    pub fn get_position(&self) -> DVec3 {
        let local = Self::rotate_y(self.pos_room * self.world_scale, self.rotation_radians);
        self.origin + DVec3::new(local.x as f64, local.y as f64, local.z as f64)
    }

    /// Position of this device in world space as `Vec3` (`VRDevicePose.getPositionF`).
    pub fn get_position_f(&self) -> Vec3 {
        let local = Self::rotate_y(self.pos_room * self.world_scale, self.rotation_radians);
        local + Vec3::new(
            self.origin.x as f32,
            self.origin.y as f32,
            self.origin.z as f32,
        )
    }

    /// Direction of this device in world space (`VRDevicePose.getDirection`).
    pub fn get_direction(&self) -> Vec3 {
        Self::rotate_y(self.dir_room, self.rotation_radians).normalize_or_zero()
    }

    /// Transforms a device-local axis into world space (`VRDevicePose.getCustomVector`).
    pub fn get_custom_vector(&self, axis: Vec3) -> Vec3 {
        let local = self.matrix_room.transform_vector3(axis);
        Self::rotate_y(local, self.rotation_radians)
    }

    /// Yaw in world space in radians (`VRDevicePose.getYawRad`).
    pub fn get_yaw_rad(&self) -> f32 {
        let dir = self.get_direction();
        (-dir.x).atan2(dir.z)
    }

    /// Yaw in world space in degrees (`VRDevicePose.getYaw`).
    pub fn get_yaw_deg(&self) -> f32 {
        self.get_yaw_rad().to_degrees()
    }

    /// Pitch in world space in radians (`VRDevicePose.getPitchRad`).
    pub fn get_pitch_rad(&self) -> f32 {
        let dir = self.get_direction();
        let len = dir.length();
        if len <= 1e-6 {
            0.0
        } else {
            (dir.y / len).clamp(-1.0, 1.0).asin()
        }
    }

    /// Pitch in world space in degrees (`VRDevicePose.getPitch`).
    pub fn get_pitch_deg(&self) -> f32 {
        self.get_pitch_rad().to_degrees()
    }

    /// Roll in world space in radians (`VRDevicePose.getRollRad`).
    pub fn get_roll_rad(&self) -> f32 {
        let c0 = self.matrix_room.col(0);
        let c1 = self.matrix_room.col(1);
        -c0.y.atan2(c1.y)
    }

    /// Roll in world space in degrees (`VRDevicePose.getRoll`).
    pub fn get_roll_deg(&self) -> f32 {
        self.get_roll_rad().to_degrees()
    }

    /// World-space orientation matrix (`VRDevicePose.getMatrix`).
    pub fn get_matrix(&self) -> Mat4 {
        Mat4::from_rotation_y(self.rotation_radians) * self.matrix_room
    }

    /// World-space normalized orientation quaternion.
    pub fn get_orientation(&self) -> Quat {
        Quat::from_mat4(&self.get_matrix()).normalize()
    }
}

/// Complete spatial VR snapshot in room or world space (`VRData.java`).
#[derive(Clone, Debug)]
pub struct VrData {
    pub origin: DVec3,
    pub rotation_radians: f32,
    pub world_scale: f32,
    pub hmd: VrDevicePose,
    pub center: VrDevicePose,
    pub eye0: VrDevicePose,
    pub eye1: VrDevicePose,
    pub c0: VrDevicePose,
    pub c1: VrDevicePose,
    pub c2: VrDevicePose,
    pub h0: VrDevicePose,
    pub h1: VrDevicePose,
    pub t0: VrDevicePose,
    pub t1: VrDevicePose,
    pub cam: VrDevicePose,
    pub fbt_mode: FbtMode,
    pub waist: Option<VrDevicePose>,
    pub foot_left: Option<VrDevicePose>,
    pub foot_right: Option<VrDevicePose>,
    pub knee_left: Option<VrDevicePose>,
    pub knee_right: Option<VrDevicePose>,
    pub elbow_left: Option<VrDevicePose>,
    pub elbow_right: Option<VrDevicePose>,
}

impl Default for VrData {
    fn default() -> Self {
        Self::from_raw(
            &RawXrFrame::default(),
            DVec3::ZERO,
            1.0,
            1.0,
            0.0,
            &VrSettings::default(),
        )
    }
}

impl VrData {
    /// Constructs a `VrData` snapshot from raw OpenXR frame poses, room origin, walk multiplier,
    /// world scale, and room rotation (`VRData(Vec3 origin, float walkMul, float worldScale, float rotation)`).
    pub fn from_raw(
        raw: &RawXrFrame,
        origin: DVec3,
        walk_mul: f32,
        world_scale: f32,
        rotation_radians: f32,
        settings: &VrSettings,
    ) -> Self {
        let hmd_raw = raw.eye_position(RenderEye::Center);
        let scaled_pos = Vec3::new(hmd_raw.x * walk_mul, hmd_raw.y, hmd_raw.z * walk_mul);
        let scale_offset = Vec3::new(scaled_pos.x - hmd_raw.x, 0.0, scaled_pos.z - hmd_raw.z);

        let hmd_dir = raw.hmd_forward();
        let hmd = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            raw.hmd_matrix(),
            scaled_pos,
            hmd_dir,
        );
        let center = hmd;

        let eye0 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            Mat4::from_quat(raw.eye_rotation(RenderEye::Left)),
            raw.eye_position(RenderEye::Left) + scale_offset,
            hmd_dir,
        );
        let eye1 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            Mat4::from_quat(raw.eye_rotation(RenderEye::Right)),
            raw.eye_position(RenderEye::Right) + scale_offset,
            hmd_dir,
        );

        let (main_idx, off_idx) = if settings.reverse_hands {
            (1, 0)
        } else {
            (0, 1)
        };
        let main_ctrl = &raw.controllers[main_idx];
        let off_ctrl = &raw.controllers[off_idx];

        let main_aim_pos = main_ctrl.aim_position + scale_offset;
        let off_aim_pos = off_ctrl.aim_position + scale_offset;

        let c0 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            main_ctrl.aim_matrix(),
            main_aim_pos,
            main_ctrl.aim_direction(),
        );
        let c1 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            off_ctrl.aim_matrix(),
            off_aim_pos,
            off_ctrl.aim_direction(),
        );

        let h0 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            main_ctrl.hand_matrix(),
            main_ctrl.grip_position + scale_offset,
            main_ctrl.hand_direction(),
        );
        let h1 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            off_ctrl.hand_matrix(),
            off_ctrl.grip_position + scale_offset,
            off_ctrl.hand_direction(),
        );

        let (t0, t1) = if settings.seated {
            (eye0, eye1)
        } else {
            (c0, c1)
        };

        let c2 = VrDevicePose::new(
            origin,
            rotation_radians,
            world_scale,
            Mat4::IDENTITY,
            Vec3::new(0.0, 1.5, -1.5) + scale_offset,
            Vec3::NEG_Z,
        );
        let cam = c2;

        let (fbt_mode, waist, foot_left, foot_right, knee_left, knee_right, elbow_left, elbow_right) =
            if let Some(trackers) = &raw.fbt_trackers {
                let make = |(pos, quat): (Vec3, Quat)| {
                    VrDevicePose::new(
                        origin,
                        rotation_radians,
                        world_scale,
                        Mat4::from_quat(quat),
                        pos + scale_offset,
                        (quat * Vec3::NEG_Z).normalize_or_zero(),
                    )
                };
                (
                    FbtMode::WithJoints,
                    Some(make(trackers[0])),
                    Some(make(trackers[1])),
                    Some(make(trackers[2])),
                    Some(make(trackers[3])),
                    Some(make(trackers[4])),
                    Some(make(trackers[5])),
                    Some(make(trackers[6])),
                )
            } else {
                (FbtMode::ArmsOnly, None, None, None, None, None, None, None)
            };

        Self {
            origin,
            rotation_radians,
            world_scale,
            hmd,
            center,
            eye0,
            eye1,
            c0,
            c1,
            c2,
            h0,
            h1,
            t0,
            t1,
            cam,
            fbt_mode,
            waist,
            foot_left,
            foot_right,
            knee_left,
            knee_right,
            elbow_left,
            elbow_right,
        }
    }

    pub fn get_controller(&self, index: usize) -> &VrDevicePose {
        match index {
            1 => &self.c1,
            2 => &self.c2,
            _ => &self.c0,
        }
    }

    pub fn get_hand(&self, index: usize) -> &VrDevicePose {
        match index {
            1 => &self.h1,
            _ => &self.h0,
        }
    }

    pub fn get_eye(&self, eye: RenderEye) -> &VrDevicePose {
        match eye {
            RenderEye::Left => &self.eye0,
            RenderEye::Right => &self.eye1,
            RenderEye::Center => &self.center,
        }
    }

    pub fn get_body_part(&self, part: VrBodyPart) -> Option<&VrDevicePose> {
        match part {
            VrBodyPart::MainHand => Some(&self.c0),
            VrBodyPart::OffHand => Some(&self.c1),
            VrBodyPart::Waist => self.waist.as_ref(),
            VrBodyPart::LeftFoot => self.foot_left.as_ref(),
            VrBodyPart::RightFoot => self.foot_right.as_ref(),
            VrBodyPart::LeftKnee => self.knee_left.as_ref(),
            VrBodyPart::RightKnee => self.knee_right.as_ref(),
            VrBodyPart::LeftElbow => self.elbow_left.as_ref(),
            VrBodyPart::RightElbow => self.elbow_right.as_ref(),
            VrBodyPart::Head => Some(&self.hmd),
        }
    }

    pub fn get_aim(&self, aim_device: AimDevice) -> &VrDevicePose {
        match aim_device {
            AimDevice::Controller => &self.c0,
            AimDevice::Hmd => &self.hmd,
        }
    }

    /// Estimates the player body yaw in radians from HMD and hands (`VRData.getBodyYawRad`).
    pub fn get_body_yaw_rad(&self, seated: bool, reverse_hands: bool) -> f32 {
        if seated {
            return self.hmd.get_yaw_rad();
        }
        let head = self.hmd.get_position_f();
        let main = self.c0.get_position_f() - head;
        let off = self.c1.get_position_f() - head;
        let arms = (off - main).normalize_or_zero();
        if arms.length_squared() < 1e-4 {
            return self.hmd.get_yaw_rad();
        }
        let rotated = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * arms;
        let hand_yaw = if reverse_hands {
            rotated.x.atan2(-rotated.z)
        } else {
            (-rotated.x).atan2(rotated.z)
        };
        let head_yaw = self.hmd.get_yaw_rad();
        // Blend head yaw and shoulder/hand yaw
        let diff = (hand_yaw - head_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        head_yaw + diff * 0.5
    }

    pub fn get_body_yaw_deg(&self, seated: bool, reverse_hands: bool) -> f32 {
        self.get_body_yaw_rad(seated, reverse_hands).to_degrees()
    }

    /// Estimates the head pivot point in world space (`VRData.getHeadPivot`).
    pub fn get_head_pivot(&self) -> DVec3 {
        let offset = self
            .hmd
            .get_matrix()
            .transform_point3(Vec3::new(0.0, -0.1 * self.world_scale, 0.1 * self.world_scale));
        self.hmd.get_position() + DVec3::new(offset.x as f64, offset.y as f64, offset.z as f64)
    }

    /// Estimates the point behind the player's head (`VRData.getHeadRear`).
    pub fn get_head_rear(&self) -> DVec3 {
        let offset = self
            .hmd
            .get_matrix()
            .transform_point3(Vec3::new(0.0, -0.2 * self.world_scale, 0.2 * self.world_scale));
        self.hmd.get_position() + DVec3::new(offset.x as f64, offset.y as f64, offset.z as f64)
    }
}
