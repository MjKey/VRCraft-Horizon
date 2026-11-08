//! VRChat OSC Tracker UDP receiver (`127.0.0.1:9000`) for Full-Body Tracking (FBT)
//! (ported from `OSCTrackerReceiver.java` and `OSCTracker.java`).
//!
//! Receives OSC packets over UDP:
//! - `/tracking/trackers/{head,1..7}/position` (float x, float y, float z)
//! - `/tracking/trackers/{head,1..7}/rotation` (float pitch_x, float yaw_y, float roll_z in degrees)

use glam::{Mat4, Quat, Vec3};
use std::net::UdpSocket;
use std::time::Instant;

pub const DEFAULT_OSC_PORT: u16 = 9000;
pub const FBT_TRACKER_COUNT: usize = 7;

/// Single OSC tracker pose and last update timestamp.
#[derive(Clone, Copy, Debug)]
pub struct OscTracker {
    pub position: Vec3,
    pub rotation: Quat,
    pub last_update: Option<Instant>,
}

impl Default for OscTracker {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            last_update: None,
        }
    }
}

impl OscTracker {
    pub fn is_tracking(&self, now: Instant, timeout_millis: u64) -> bool {
        if let Some(last) = self.last_update {
            now.duration_since(last).as_millis() <= timeout_millis as u128
        } else {
            false
        }
    }
}

/// UDP receiver for VRChat-compatible OSC trackers.
pub struct OscTrackerReceiver {
    socket: Option<UdpSocket>,
    port: u16,
    pub head_offset: Vec3,
    pub head_rotation_offset: f32,
    pub last_head_update: Option<Instant>,
    /// 7 Body trackers: [0: waist, 1: left_foot, 2: right_foot, 3: left_knee, 4: right_knee, 5: left_elbow, 6: right_elbow]
    pub trackers: [OscTracker; FBT_TRACKER_COUNT],
}

impl Default for OscTrackerReceiver {
    fn default() -> Self {
        Self::new(DEFAULT_OSC_PORT)
    }
}

impl OscTrackerReceiver {
    pub fn new(port: u16) -> Self {
        let socket = UdpSocket::bind(("127.0.0.1", port)).ok();
        if let Some(ref s) = socket {
            let _ = s.set_nonblocking(true);
        }
        Self {
            socket,
            port,
            head_offset: Vec3::ZERO,
            head_rotation_offset: 0.0,
            last_head_update: None,
            trackers: [OscTracker::default(); FBT_TRACKER_COUNT],
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_listening(&self) -> bool {
        self.socket.is_some()
    }

    /// Rebinds to a different UDP port if desired.
    pub fn change_port(&mut self, port: u16) {
        self.port = port;
        self.socket = UdpSocket::bind(("127.0.0.1", port)).ok();
        if let Some(ref s) = self.socket {
            let _ = s.set_nonblocking(true);
        }
    }

    /// Polls incoming UDP packets without blocking, updates tracker states,
    /// and returns the 7 FBT tracker poses if any tracker is active.
    pub fn poll_trackers(&mut self, hmd_forward: Vec3) -> Option<[(Vec3, Quat); FBT_TRACKER_COUNT]> {
        let now = Instant::now();
        let mut packets = Vec::new();
        if let Some(ref socket) = self.socket {
            let mut buf = [0u8; 2048];
            while let Ok((amt, _src)) = socket.recv_from(&mut buf) {
                packets.push(buf[..amt].to_vec());
            }
        }
        for packet in packets {
            self.parse_osc_packet(&packet, now, hmd_forward);
        }

        let any_active = self.trackers.iter().any(|t| t.is_tracking(now, 1000));
        if !any_active {
            return None;
        }

        let mut poses = [(Vec3::ZERO, Quat::IDENTITY); FBT_TRACKER_COUNT];
        for i in 0..FBT_TRACKER_COUNT {
            poses[i] = (self.trackers[i].position, self.trackers[i].rotation);
        }
        Some(poses)
    }

    /// Parses an OSC message from raw bytes (`/tracking/trackers/...`).
    pub fn parse_osc_packet(&mut self, data: &[u8], now: Instant, hmd_forward: Vec3) {
        if data.len() < 8 {
            return;
        }

        // 1. Read null-terminated 4-byte padded address string
        let Some(addr_end) = data.iter().position(|&b| b == 0) else {
            return;
        };
        let Ok(address) = std::str::from_utf8(&data[..addr_end]) else {
            return;
        };
        let type_tag_start = (addr_end + 4) & !3;
        if type_tag_start >= data.len() || data[type_tag_start] != b',' {
            return;
        }

        // 2. Read type tags
        let Some(tag_end) = data[type_tag_start..].iter().position(|&b| b == 0) else {
            return;
        };
        let tags_bytes = &data[type_tag_start + 1..type_tag_start + tag_end];
        let args_start = (type_tag_start + tag_end + 4) & !3;

        // Extract float arguments
        let mut floats = Vec::with_capacity(3);
        let mut cur = args_start;
        for &tag in tags_bytes {
            if tag == b'f' && cur + 4 <= data.len() {
                let val = f32::from_be_bytes([data[cur], data[cur + 1], data[cur + 2], data[cur + 3]]);
                floats.push(val);
                cur += 4;
            }
        }

        if floats.len() < 3 {
            return;
        }

        // Determine tracker index and message kind
        let is_pos = address.ends_with("/position");
        let is_rot = address.ends_with("/rotation");
        if !is_pos && !is_rot {
            return;
        }

        let tracker_id = if address.contains("/head/") {
            0
        } else if let Some(idx) = address.find("/trackers/") {
            let rest = &address[idx + 10..];
            if let Some(slash) = rest.find('/') {
                rest[..slash].parse::<usize>().unwrap_or(99)
            } else {
                99
            }
        } else {
            99
        };

        if tracker_id == 0 {
            // Head calibration data
            if is_pos {
                self.head_offset = Vec3::new(-floats[0], -floats[1], floats[2]);
            } else if is_rot {
                let rx = floats[0].to_radians();
                let ry = floats[1].to_radians();
                let rz = (-floats[2]).to_radians();
                let rot_mat = Mat4::from_rotation_z(rz)
                    * Mat4::from_rotation_x(rx)
                    * Mat4::from_rotation_y(ry);
                let fwd = rot_mat.transform_vector3(Vec3::NEG_Z);
                let new_rot = (-fwd.x).atan2(fwd.z) - (-hmd_forward.x).atan2(hmd_forward.z);

                if let Some(last) = self.last_head_update {
                    if now.duration_since(last).as_millis() < 300 {
                        self.head_rotation_offset = self.head_rotation_offset + 0.01 * (new_rot - self.head_rotation_offset);
                    } else {
                        self.head_rotation_offset = new_rot;
                    }
                } else {
                    self.head_rotation_offset = new_rot;
                }
                self.last_head_update = Some(now);
            }
        } else if (1..=FBT_TRACKER_COUNT).contains(&tracker_id) {
            let target = &mut self.trackers[tracker_id - 1];
            if is_pos {
                let mut trans = Vec3::new(floats[0], floats[1], -floats[2]);
                trans += self.head_offset;
                trans = Quat::from_rotation_y(self.head_rotation_offset) * trans;
                target.position = trans;
                target.last_update = Some(now);
            } else if is_rot {
                let rx = floats[0].to_radians();
                let ry = floats[1].to_radians();
                let rz = (-floats[2]).to_radians();
                let rot = Quat::from_rotation_y(self.head_rotation_offset)
                    * (Quat::from_rotation_z(rz) * Quat::from_rotation_x(rx) * Quat::from_rotation_y(ry));
                target.rotation = rot;
                target.last_update = Some(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osc_packet_parser() {
        let mut osc = OscTrackerReceiver::new(19876);
        let now = Instant::now();

        // 1. Build an OSC packet for /tracking/trackers/1/position (x=0.5, y=1.0, z=0.2)
        let mut packet = Vec::new();
        let addr = b"/tracking/trackers/1/position\0\0\0";
        packet.extend_from_slice(addr);
        let tags = b",fff\0\0\0\0";
        packet.extend_from_slice(tags);
        packet.extend_from_slice(&0.5_f32.to_be_bytes());
        packet.extend_from_slice(&1.0_f32.to_be_bytes());
        packet.extend_from_slice(&0.2_f32.to_be_bytes());

        osc.parse_osc_packet(&packet, now, Vec3::NEG_Z);
        assert!(osc.trackers[0].is_tracking(now, 100));
        assert!((osc.trackers[0].position.x - 0.5).abs() < 1e-4);
        assert!((osc.trackers[0].position.y - 1.0).abs() < 1e-4);
        assert!((osc.trackers[0].position.z - (-0.2)).abs() < 1e-4);
    }
}
