use std::collections::HashMap;

use glam::{Quat, Vec3};

use crate::vr_data::{FbtMode, VrData};

/// Official Vivecraft custom payload channel identifier (`CommonNetworkHelper.CHANNEL`).
pub const VIVECRAFT_DATA_CHANNEL: &str = "vivecraft:data";

/// Official Vivecraft `PayloadIdentifier` enum (`0..=31`), matching `PayloadIdentifier.java` byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PayloadIdentifier {
    Version = 0,
    RequestData = 1,
    HeadData = 2,
    Controller0Data = 3,
    Controller1Data = 4,
    WorldScale = 5,
    Draw = 6,
    MoveMode = 7,
    UberPacket = 8,
    Teleport = 9,
    Climbing = 10,
    Jumping = 11,
    Height = 12,
    ActiveHand = 13,
    Crawl = 14,
    NetworkVersion = 15,
    VrSwitching = 16,
    IsVrActive = 17,
    VrPlayerState = 18,
    DualWielding = 19,
    Haptic = 20,
    SettingOverride = 21,
    AimDirOverride = 22,
    AimPosOverride = 23,
    ServerVrChanges = 24,
    DamageDirection = 25,
    AttackWhileBlocking = 26,
    AimOverrideReset = 27,
    RoomscaleAttack = 28,
    RightHanded = 29,
    MainHand = 30,
    LegacyHeadData = 31,
}

impl PayloadIdentifier {
    pub fn from_u8(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::Version,
            1 => Self::RequestData,
            2 => Self::HeadData,
            3 => Self::Controller0Data,
            4 => Self::Controller1Data,
            5 => Self::WorldScale,
            6 => Self::Draw,
            7 => Self::MoveMode,
            8 => Self::UberPacket,
            9 => Self::Teleport,
            10 => Self::Climbing,
            11 => Self::Jumping,
            12 => Self::Height,
            13 => Self::ActiveHand,
            14 => Self::Crawl,
            15 => Self::NetworkVersion,
            16 => Self::VrSwitching,
            17 => Self::IsVrActive,
            18 => Self::VrPlayerState,
            19 => Self::DualWielding,
            20 => Self::Haptic,
            21 => Self::SettingOverride,
            22 => Self::AimDirOverride,
            23 => Self::AimPosOverride,
            24 => Self::ServerVrChanges,
            25 => Self::DamageDirection,
            26 => Self::AttackWhileBlocking,
            27 => Self::AimOverrideReset,
            28 => Self::RoomscaleAttack,
            29 => Self::RightHanded,
            30 => Self::MainHand,
            31 => Self::LegacyHeadData,
            _ => return None,
        })
    }
}

/// Official Vivecraft `NetworkVersion` enum (`NetworkVersion.java`), where `protocolVersion() = ordinal() - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum NetworkVersion {
    Legacy = -1,
    NewNetworking = 0,
    Fbt = 1,
    DualWielding = 2,
    ServerVrChanges = 3,
    Haptic = 4,
    DamageDirection = 5,
    OptionToggle = 6,
    AttackWhileBlocking = 7,
    AimReset = 8,
    RoomscaleAttack = 9,
    HeadQuat = 10,
}

impl NetworkVersion {
    pub const MIN_SUPPORTED: Self = Self::NewNetworking;
    pub const MAX_SUPPORTED: Self = Self::HeadQuat;

    pub fn from_protocol_version(version: i32) -> Option<Self> {
        Some(match version {
            -1 => Self::Legacy,
            0 => Self::NewNetworking,
            1 => Self::Fbt,
            2 => Self::DualWielding,
            3 => Self::ServerVrChanges,
            4 => Self::Haptic,
            5 => Self::DamageDirection,
            6 => Self::OptionToggle,
            7 => Self::AttackWhileBlocking,
            8 => Self::AimReset,
            9 => Self::RoomscaleAttack,
            10 => Self::HeadQuat,
            _ => return None,
        })
    }

    pub fn protocol_version(self) -> i32 {
        self as i32
    }

    pub fn accepts(self, client_version: Self) -> bool {
        client_version >= self
    }
}

/// Networked 6DoF pose (`Pose.java`: `Vec3 position` + `Quaternionf orientation`, 28 bytes on wire).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetPose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl Default for NetPose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        }
    }
}

impl NetPose {
    pub fn serialize(&self, out: &mut Vec<u8>) {
        write_vec3(out, self.position);
        write_quat(out, self.orientation);
    }

    pub fn deserialize(cursor: &mut ByteCursor<'_>) -> Option<Self> {
        let position = cursor.read_vec3()?;
        let orientation = cursor.read_quat()?;
        Some(Self {
            position,
            orientation,
        })
    }
}

/// Full player VR pose state (`VrPlayerState.java`), supporting legacy `Seated` bool, `HEAD_QUAT`,
/// and full-body tracking (waist, feet, elbows, knees).
#[derive(Debug, Clone, PartialEq)]
pub struct VrPlayerState {
    pub seated: bool,
    pub hmd: NetPose,
    pub left_handed: bool,
    pub main_hand: NetPose,
    pub reverse_hands: bool,
    pub off_hand: NetPose,
    pub fbt_mode: FbtMode,
    pub waist: Option<NetPose>,
    pub right_foot: Option<NetPose>,
    pub left_foot: Option<NetPose>,
    pub right_elbow: Option<NetPose>,
    pub left_elbow: Option<NetPose>,
    pub right_knee: Option<NetPose>,
    pub left_knee: Option<NetPose>,
}

impl VrPlayerState {
    /// Constructs a player-relative `VrPlayerState` from `world_post` (`VrPlayerState.create(VRPlayer)`).
    pub fn from_vr_data(world_post: &VrData, player_pos: Vec3, seated: bool, left_handed: bool) -> Self {
        let hmd = NetPose {
            position: world_post.hmd.get_position_f() - player_pos,
            orientation: world_post.hmd.get_orientation(),
        };
        let main_hand = NetPose {
            position: world_post.c0.get_position_f() - player_pos,
            orientation: world_post.c0.get_orientation(),
        };
        let off_hand = NetPose {
            position: world_post.c1.get_position_f() - player_pos,
            orientation: world_post.c1.get_orientation(),
        };

        let to_net = |opt: Option<crate::vr_data::VrDevicePose>| -> Option<NetPose> {
            opt.map(|p| NetPose {
                position: p.get_position_f() - player_pos,
                orientation: p.get_orientation(),
            })
        };

        Self {
            seated,
            hmd,
            left_handed,
            main_hand,
            reverse_hands: left_handed,
            off_hand,
            fbt_mode: world_post.fbt_mode,
            waist: to_net(world_post.waist),
            right_foot: to_net(world_post.foot_right),
            left_foot: to_net(world_post.foot_left),
            right_elbow: to_net(world_post.elbow_right),
            left_elbow: to_net(world_post.elbow_left),
            right_knee: to_net(world_post.knee_right),
            left_knee: to_net(world_post.knee_left),
        }
    }

    /// Serializes `VrPlayerState` according to the negotiated `NetworkVersion` (matching `VrPlayerState.serialize`).
    pub fn serialize(&self, out: &mut Vec<u8>, version: NetworkVersion) {
        if !NetworkVersion::HeadQuat.accepts(version) {
            write_bool(out, self.seated);
        }
        self.hmd.serialize(out);
        write_bool(out, self.left_handed);
        self.main_hand.serialize(out);
        write_bool(out, self.reverse_hands);
        self.off_hand.serialize(out);

        if NetworkVersion::Fbt.accepts(version) {
            out.push(self.fbt_mode as u8);
            if self.fbt_mode != FbtMode::ArmsOnly {
                self.waist.unwrap_or_default().serialize(out);
                self.right_foot.unwrap_or_default().serialize(out);
                self.left_foot.unwrap_or_default().serialize(out);
                if self.fbt_mode == FbtMode::WithJoints {
                    self.right_elbow.unwrap_or_default().serialize(out);
                    self.left_elbow.unwrap_or_default().serialize(out);
                    self.right_knee.unwrap_or_default().serialize(out);
                    self.left_knee.unwrap_or_default().serialize(out);
                }
            }
        }
    }

    /// Deserializes `VrPlayerState` according to `version` (matching `VrPlayerState.deserialize`).
    pub fn deserialize(cursor: &mut ByteCursor<'_>, version: NetworkVersion) -> Option<Self> {
        let seated = if !NetworkVersion::HeadQuat.accepts(version) {
            cursor.read_bool()?
        } else {
            false
        };
        let hmd = NetPose::deserialize(cursor)?;
        let left_handed = cursor.read_bool()?;
        let main_hand = NetPose::deserialize(cursor)?;
        let reverse_hands = cursor.read_bool()?;
        let off_hand = NetPose::deserialize(cursor)?;

        let mut fbt_mode = FbtMode::ArmsOnly;
        let mut waist = None;
        let mut right_foot = None;
        let mut left_foot = None;
        let mut right_elbow = None;
        let mut left_elbow = None;
        let mut right_knee = None;
        let mut left_knee = None;

        if NetworkVersion::Fbt.accepts(version) && cursor.remaining() > 0 {
            fbt_mode = FbtMode::from_byte(cursor.read_u8()?);
            if fbt_mode != FbtMode::ArmsOnly {
                waist = Some(NetPose::deserialize(cursor)?);
                right_foot = Some(NetPose::deserialize(cursor)?);
                left_foot = Some(NetPose::deserialize(cursor)?);
                if fbt_mode == FbtMode::WithJoints {
                    right_elbow = Some(NetPose::deserialize(cursor)?);
                    left_elbow = Some(NetPose::deserialize(cursor)?);
                    right_knee = Some(NetPose::deserialize(cursor)?);
                    left_knee = Some(NetPose::deserialize(cursor)?);
                }
            }
        }

        Some(Self {
            seated,
            hmd,
            left_handed,
            main_hand,
            reverse_hands,
            off_hand,
            fbt_mode,
            waist,
            right_foot,
            left_foot,
            right_elbow,
            left_elbow,
            right_knee,
            left_knee,
        })
    }
}

/// Decoded `vivecraft:data` packet payload (C2S and S2C).
#[derive(Debug, Clone, PartialEq)]
pub enum VivecraftPacket {
    /// `PayloadIdentifier::Version` (0): Client/Server handshake version string, VR mode flag, and protocol version range.
    Version {
        version: String,
        vr_active: bool,
        max_network_version: NetworkVersion,
        min_network_version: NetworkVersion,
    },
    /// `PayloadIdentifier::RequestData` (1): Server requests continual VR pose updates from client.
    RequestData,
    /// `PayloadIdentifier::WorldScale` (5): Player VR world scale factor.
    WorldScale(f32),
    /// `PayloadIdentifier::Draw` (6): Current two-handed bow draw fraction (`0.0..=1.0`).
    Draw(f32),
    /// `PayloadIdentifier::UberPacket` (8): Server broadcasts another VR player's full pose, scale, and height.
    UberPacket {
        player_uuid: u128,
        state: VrPlayerState,
        world_scale: f32,
        height_scale: f32,
    },
    /// `PayloadIdentifier::Teleport` (9): Client requests direct VR teleport to `(x, y, z)`, or Server permits teleport.
    Teleport(Option<Vec3>),
    /// `PayloadIdentifier::Climbing` (10): Climbing negotiation / state.
    Climbing(bool),
    /// `PayloadIdentifier::Jumping` (11): Physical jump trigger.
    Jumping,
    /// `PayloadIdentifier::Height` (12): Calibrated player height ratio (`height_m / 1.52`).
    Height(f32),
    /// `PayloadIdentifier::ActiveHand` (13): Active VR hand (`0 = mainhand`, `1 = offhand`).
    ActiveHand(u8),
    /// `PayloadIdentifier::Crawl` (14): Roomscale physical crawl state.
    Crawl(bool),
    /// `PayloadIdentifier::NetworkVersion` (15): Negotiated `NetworkVersion` chosen by server.
    NegotiatedNetworkVersion(NetworkVersion),
    /// `PayloadIdentifier::VrSwitching` (16): Whether hot-switching between VR and NonVR is allowed.
    VrSwitching(bool),
    /// `PayloadIdentifier::IsVrActive` (17): Whether a player is currently in VR mode (with optional player UUID on S2C).
    IsVrActive {
        vr_active: bool,
        player_uuid: Option<u128>,
    },
    /// `PayloadIdentifier::VrPlayerState` (18): Client sends its 6DoF HMD/controller/FBT poses for the tick.
    VrPlayerState(VrPlayerState),
    /// `PayloadIdentifier::DualWielding` (19): Dual-wielding state.
    DualWielding(bool),
    /// `PayloadIdentifier::Haptic` (20): Server triggers haptic pulse on client controller.
    Haptic {
        hand: u8,
        frequency_hz: f32,
        amplitude: f32,
        duration_seconds: f32,
        delay_seconds: f32,
    },
    /// `PayloadIdentifier::AimDirOverride` (22): Client overrides server action aim direction.
    AimDirOverride(Vec3),
    /// `PayloadIdentifier::AimPosOverride` (23): Client overrides server action origin offset.
    AimPosOverride(Vec3),
    /// `PayloadIdentifier::DamageDirection` (25): Server sends incoming damage direction vector for directional VR vignette/haptics.
    DamageDirection(Vec3),
    /// `PayloadIdentifier::AttackWhileBlocking` (26): Server allows attacking while blocking with a shield.
    AttackWhileBlocking(bool),
    /// `PayloadIdentifier::AimOverrideReset` (27): Resets aim overrides after `ticks`.
    AimOverrideReset(u8),
    /// `PayloadIdentifier::RoomscaleAttack` (28): Physical swing attack notification.
    RoomscaleAttack(bool),
}

impl VivecraftPacket {
    pub fn payload_id(&self) -> PayloadIdentifier {
        match self {
            Self::Version { .. } => PayloadIdentifier::Version,
            Self::RequestData => PayloadIdentifier::RequestData,
            Self::WorldScale(_) => PayloadIdentifier::WorldScale,
            Self::Draw(_) => PayloadIdentifier::Draw,
            Self::UberPacket { .. } => PayloadIdentifier::UberPacket,
            Self::Teleport(_) => PayloadIdentifier::Teleport,
            Self::Climbing(_) => PayloadIdentifier::Climbing,
            Self::Jumping => PayloadIdentifier::Jumping,
            Self::Height(_) => PayloadIdentifier::Height,
            Self::ActiveHand(_) => PayloadIdentifier::ActiveHand,
            Self::Crawl(_) => PayloadIdentifier::Crawl,
            Self::NegotiatedNetworkVersion(_) => PayloadIdentifier::NetworkVersion,
            Self::VrSwitching(_) => PayloadIdentifier::VrSwitching,
            Self::IsVrActive { .. } => PayloadIdentifier::IsVrActive,
            Self::VrPlayerState(_) => PayloadIdentifier::VrPlayerState,
            Self::DualWielding(_) => PayloadIdentifier::DualWielding,
            Self::Haptic { .. } => PayloadIdentifier::Haptic,
            Self::AimDirOverride(_) => PayloadIdentifier::AimDirOverride,
            Self::AimPosOverride(_) => PayloadIdentifier::AimPosOverride,
            Self::DamageDirection(_) => PayloadIdentifier::DamageDirection,
            Self::AttackWhileBlocking(_) => PayloadIdentifier::AttackWhileBlocking,
            Self::AimOverrideReset(_) => PayloadIdentifier::AimOverrideReset,
            Self::RoomscaleAttack(_) => PayloadIdentifier::RoomscaleAttack,
        }
    }

    /// Encodes this packet into raw bytes ready for `vivecraft:data` custom payload transmission.
    pub fn encode(&self, negotiated_version: NetworkVersion) -> Vec<u8> {
        let mut out = Vec::with_capacity(96);
        out.push(self.payload_id() as u8);

        match self {
            Self::Version {
                version,
                vr_active,
                max_network_version,
                min_network_version,
            } => {
                // VersionPayloadC2S / VersionPayloadS2C uses Minecraft FriendlyByteBuf.writeUtf:
                // For C2S it encodes "version\nvr or nonvr\nmax\nmin"
                let mode = if *vr_active { "vr" } else { "nonvr" };
                let wire = format!(
                    "{version}\n{mode}\n{}\n{}",
                    max_network_version.protocol_version(),
                    min_network_version.protocol_version()
                );
                write_mc_utf(&mut out, &wire);
            }
            Self::RequestData | Self::Jumping => {}
            Self::WorldScale(scale) => write_f32(&mut out, *scale),
            Self::Draw(draw) => write_f32(&mut out, *draw),
            Self::UberPacket {
                player_uuid,
                state,
                world_scale,
                height_scale,
            } => {
                write_u128(&mut out, *player_uuid);
                state.serialize(&mut out, negotiated_version);
                write_f32(&mut out, *world_scale);
                write_f32(&mut out, *height_scale);
            }
            Self::Teleport(Some(pos)) => write_vec3(&mut out, *pos),
            Self::Teleport(None) => {}
            Self::Climbing(v)
            | Self::Crawl(v)
            | Self::VrSwitching(v)
            | Self::DualWielding(v)
            | Self::AttackWhileBlocking(v)
            | Self::RoomscaleAttack(v) => write_bool(&mut out, *v),
            Self::Height(h) => write_f32(&mut out, *h),
            Self::ActiveHand(hand) | Self::AimOverrideReset(hand) => out.push(*hand),
            Self::NegotiatedNetworkVersion(ver) => {
                out.push(ver.protocol_version().clamp(0, 127) as u8);
            }
            Self::IsVrActive {
                vr_active,
                player_uuid,
            } => {
                write_bool(&mut out, *vr_active);
                if let Some(uuid) = player_uuid {
                    write_u128(&mut out, *uuid);
                }
            }
            Self::VrPlayerState(state) => state.serialize(&mut out, negotiated_version),
            Self::Haptic {
                hand,
                frequency_hz,
                amplitude,
                duration_seconds,
                delay_seconds,
            } => {
                out.push(*hand);
                write_f32(&mut out, *frequency_hz);
                write_f32(&mut out, *amplitude);
                write_f32(&mut out, *duration_seconds);
                write_f32(&mut out, *delay_seconds);
            }
            Self::AimDirOverride(v) | Self::AimPosOverride(v) | Self::DamageDirection(v) => {
                write_vec3(&mut out, *v);
            }
        }

        out
    }

    /// Decodes a raw `vivecraft:data` payload buffer using the negotiated `NetworkVersion`.
    pub fn decode(bytes: &[u8], negotiated_version: NetworkVersion) -> Option<Self> {
        let mut cursor = ByteCursor::new(bytes);
        let id = PayloadIdentifier::from_u8(cursor.read_u8()?)?;

        Some(match id {
            PayloadIdentifier::Version => {
                let text = cursor.read_mc_utf()?;
                let mut lines = text.lines();
                let version = lines.next().unwrap_or("Vivecraft-Horizon").to_owned();
                let mode_line = lines.next().unwrap_or("vr");
                let vr_active = !mode_line.contains("nonvr");
                let max_ver = lines
                    .next()
                    .and_then(|s| s.parse::<i32>().ok())
                    .and_then(NetworkVersion::from_protocol_version)
                    .unwrap_or(NetworkVersion::MAX_SUPPORTED);
                let min_ver = lines
                    .next()
                    .and_then(|s| s.parse::<i32>().ok())
                    .and_then(NetworkVersion::from_protocol_version)
                    .unwrap_or(NetworkVersion::MIN_SUPPORTED);
                Self::Version {
                    version,
                    vr_active,
                    max_network_version: max_ver,
                    min_network_version: min_ver,
                }
            }
            PayloadIdentifier::RequestData => Self::RequestData,
            PayloadIdentifier::WorldScale => Self::WorldScale(cursor.read_f32()?),
            PayloadIdentifier::Draw => Self::Draw(cursor.read_f32()?),
            PayloadIdentifier::UberPacket => {
                let player_uuid = cursor.read_u128()?;
                let state = VrPlayerState::deserialize(&mut cursor, negotiated_version)?;
                let world_scale = cursor.read_f32()?;
                let height_scale = cursor.read_f32()?;
                Self::UberPacket {
                    player_uuid,
                    state,
                    world_scale,
                    height_scale,
                }
            }
            PayloadIdentifier::Teleport => {
                if cursor.remaining() >= 12 {
                    Self::Teleport(Some(cursor.read_vec3()?))
                } else {
                    Self::Teleport(None)
                }
            }
            PayloadIdentifier::Climbing => {
                let enabled = if cursor.remaining() > 0 {
                    cursor.read_bool()?
                } else {
                    true
                };
                Self::Climbing(enabled)
            }
            PayloadIdentifier::Jumping => Self::Jumping,
            PayloadIdentifier::Height => Self::Height(cursor.read_f32()?),
            PayloadIdentifier::ActiveHand => Self::ActiveHand(cursor.read_u8()?),
            PayloadIdentifier::Crawl => Self::Crawl(cursor.read_bool()?),
            PayloadIdentifier::NetworkVersion => {
                let ver_byte = cursor.read_u8()? as i32;
                let ver = NetworkVersion::from_protocol_version(ver_byte)
                    .unwrap_or(NetworkVersion::HeadQuat);
                Self::NegotiatedNetworkVersion(ver)
            }
            PayloadIdentifier::VrSwitching => Self::VrSwitching(cursor.read_bool()?),
            PayloadIdentifier::IsVrActive => {
                let vr_active = cursor.read_bool()?;
                let player_uuid = if cursor.remaining() >= 16 {
                    Some(cursor.read_u128()?)
                } else {
                    None
                };
                Self::IsVrActive {
                    vr_active,
                    player_uuid,
                }
            }
            PayloadIdentifier::VrPlayerState => {
                Self::VrPlayerState(VrPlayerState::deserialize(&mut cursor, negotiated_version)?)
            }
            PayloadIdentifier::DualWielding => Self::DualWielding(cursor.read_bool()?),
            PayloadIdentifier::Haptic => Self::Haptic {
                hand: cursor.read_u8()?,
                frequency_hz: cursor.read_f32()?,
                amplitude: cursor.read_f32()?,
                duration_seconds: cursor.read_f32()?,
                delay_seconds: cursor.read_f32()?,
            },
            PayloadIdentifier::AimDirOverride => Self::AimDirOverride(cursor.read_vec3()?),
            PayloadIdentifier::AimPosOverride => Self::AimPosOverride(cursor.read_vec3()?),
            PayloadIdentifier::DamageDirection => Self::DamageDirection(cursor.read_vec3()?),
            PayloadIdentifier::AttackWhileBlocking => {
                Self::AttackWhileBlocking(cursor.read_bool()?)
            }
            PayloadIdentifier::AimOverrideReset => Self::AimOverrideReset(cursor.read_u8()?),
            PayloadIdentifier::RoomscaleAttack => {
                let flag = if cursor.remaining() > 0 {
                    cursor.read_bool()?
                } else {
                    true
                };
                Self::RoomscaleAttack(flag)
            }
            _ => return None,
        })
    }
}

/// Remote VR player state tracked by the client (via `UberPacketPayloadS2C`) or server.
#[derive(Debug, Clone)]
pub struct RemoteVrPlayer {
    pub player_uuid: u128,
    pub vr_active: bool,
    pub state: VrPlayerState,
    pub world_scale: f32,
    pub height_scale: f32,
    pub negotiated_version: NetworkVersion,
}

/// Client-side and Server-side `vivecraft:data` protocol manager (`ClientNetworking.java` + `ServerNetworking.java`).
#[derive(Debug, Clone)]
pub struct VivecraftNetworkManager {
    pub server_supports_vivecraft: bool,
    pub server_wants_data: bool,
    pub negotiated_version: NetworkVersion,
    pub server_allows_teleport: bool,
    pub server_allows_climbing: bool,
    pub server_allows_crawling: bool,
    pub server_allows_vr_switching: bool,
    pub server_allows_dual_wielding: bool,
    pub server_allows_attack_while_blocking: bool,
    pub last_damage_direction: Option<Vec3>,
    pub remote_players: HashMap<u128, RemoteVrPlayer>,
    handshake_sent: bool,
}

impl Default for VivecraftNetworkManager {
    fn default() -> Self {
        Self {
            server_supports_vivecraft: false,
            server_wants_data: false,
            negotiated_version: NetworkVersion::HeadQuat,
            server_allows_teleport: false,
            server_allows_climbing: false,
            server_allows_crawling: false,
            server_allows_vr_switching: true,
            server_allows_dual_wielding: false,
            server_allows_attack_while_blocking: false,
            last_damage_direction: None,
            remote_players: HashMap::new(),
            handshake_sent: false,
        }
    }
}

impl VivecraftNetworkManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_connection(&mut self) {
        *self = Self::default();
    }

    /// Builds the initial C2S `VersionPayloadC2S` packet when joining a world/server.
    pub fn build_client_handshake(&mut self, vr_active: bool) -> Vec<u8> {
        self.handshake_sent = true;
        VivecraftPacket::Version {
            version: "Vivecraft-Horizon-0.1.0".to_owned(),
            vr_active,
            max_network_version: NetworkVersion::MAX_SUPPORTED,
            min_network_version: NetworkVersion::MIN_SUPPORTED,
        }
        .encode(self.negotiated_version)
    }

    /// Processes an incoming S2C `vivecraft:data` packet on the client and returns any response C2S payloads
    /// plus an optional haptic pulse.
    pub fn handle_s2c_packet(
        &mut self,
        payload: &[u8],
        vr_active: bool,
        world_scale: f32,
        calibrated_height_m: f32,
    ) -> (Vec<Vec<u8>>, Option<crate::xr::HapticPulse>) {
        let mut responses = Vec::new();
        let mut haptic = None;

        let Some(packet) = VivecraftPacket::decode(payload, self.negotiated_version) else {
            return (responses, haptic);
        };

        match packet {
            VivecraftPacket::Version { .. } => {
                self.server_supports_vivecraft = true;
            }
            VivecraftPacket::NegotiatedNetworkVersion(version) => {
                self.negotiated_version = version;
            }
            VivecraftPacket::RequestData => {
                self.server_supports_vivecraft = true;
                self.server_wants_data = true;
                responses.push(
                    VivecraftPacket::IsVrActive {
                        vr_active,
                        player_uuid: None,
                    }
                    .encode(self.negotiated_version),
                );
                responses.push(
                    VivecraftPacket::WorldScale(world_scale).encode(self.negotiated_version),
                );
                responses.push(
                    VivecraftPacket::Height(calibrated_height_m / 1.52)
                        .encode(self.negotiated_version),
                );
            }
            VivecraftPacket::UberPacket {
                player_uuid,
                state,
                world_scale,
                height_scale,
            } => {
                self.remote_players.insert(
                    player_uuid,
                    RemoteVrPlayer {
                        player_uuid,
                        vr_active: true,
                        state,
                        world_scale,
                        height_scale,
                        negotiated_version: self.negotiated_version,
                    },
                );
            }
            VivecraftPacket::IsVrActive {
                vr_active,
                player_uuid: Some(uuid),
            } => {
                if !vr_active {
                    self.remote_players.remove(&uuid);
                }
            }
            VivecraftPacket::Teleport(_) => self.server_allows_teleport = true,
            VivecraftPacket::Climbing(allowed) => self.server_allows_climbing = allowed,
            VivecraftPacket::Crawl(allowed) => self.server_allows_crawling = allowed,
            VivecraftPacket::VrSwitching(allowed) => self.server_allows_vr_switching = allowed,
            VivecraftPacket::DualWielding(allowed) => self.server_allows_dual_wielding = allowed,
            VivecraftPacket::AttackWhileBlocking(allowed) => {
                self.server_allows_attack_while_blocking = allowed;
            }
            VivecraftPacket::DamageDirection(dir) => {
                self.last_damage_direction = Some(dir);
            }
            VivecraftPacket::Haptic {
                hand,
                frequency_hz,
                amplitude,
                duration_seconds,
                ..
            } => {
                haptic = Some(crate::xr::HapticPulse {
                    controller: usize::from(hand),
                    duration_micros: (duration_seconds.clamp(0.0, 5.0) * 1_000_000.0) as u32,
                    frequency_hz,
                    amplitude,
                });
            }
            _ => {}
        }

        (responses, haptic)
    }

    /// Builds the per-tick C2S `VrPlayerState` packet (and optional bow draw / scale packets) when connected to a Vivecraft server.
    pub fn build_tick_c2s_packets(
        &self,
        world_post: &VrData,
        player_pos: Vec3,
        seated: bool,
        left_handed: bool,
        bow_draw: Option<f32>,
    ) -> Vec<Vec<u8>> {
        if !self.server_wants_data {
            return Vec::new();
        }
        let mut packets = Vec::with_capacity(2);
        let state = VrPlayerState::from_vr_data(world_post, player_pos, seated, left_handed);
        packets.push(VivecraftPacket::VrPlayerState(state).encode(self.negotiated_version));

        if let Some(draw) = bow_draw {
            packets.push(VivecraftPacket::Draw(draw).encode(self.negotiated_version));
        }
        packets
    }

    /// Server-side handler for an incoming C2S `vivecraft:data` packet from `player_uuid`:
    /// updates the player's server-side VR state and returns `(direct_reply_s2c, broadcast_uber_packet_s2c)`.
    pub fn handle_server_c2s_packet(
        &mut self,
        player_uuid: u128,
        payload: &[u8],
    ) -> (Vec<Vec<u8>>, Option<Vec<u8>>) {
        let current_ver = self
            .remote_players
            .get(&player_uuid)
            .map(|p| p.negotiated_version)
            .unwrap_or(NetworkVersion::HeadQuat);

        let Some(packet) = VivecraftPacket::decode(payload, current_ver) else {
            return (Vec::new(), None);
        };

        let mut direct_replies = Vec::new();
        let mut broadcast = None;

        match packet {
            VivecraftPacket::Version {
                vr_active,
                max_network_version,
                min_network_version,
                ..
            } => {
                let chosen = if max_network_version >= NetworkVersion::MIN_SUPPORTED
                    && min_network_version <= NetworkVersion::MAX_SUPPORTED
                {
                    max_network_version.min(NetworkVersion::MAX_SUPPORTED)
                } else {
                    NetworkVersion::Legacy
                };

                self.remote_players
                    .entry(player_uuid)
                    .or_insert_with(|| RemoteVrPlayer {
                        player_uuid,
                        vr_active,
                        state: VrPlayerState {
                            seated: false,
                            hmd: NetPose::default(),
                            left_handed: false,
                            main_hand: NetPose::default(),
                            reverse_hands: false,
                            off_hand: NetPose::default(),
                            fbt_mode: FbtMode::ArmsOnly,
                            waist: None,
                            right_foot: None,
                            left_foot: None,
                            right_elbow: None,
                            left_elbow: None,
                            right_knee: None,
                            left_knee: None,
                        },
                        world_scale: 1.0,
                        height_scale: 1.0,
                        negotiated_version: chosen,
                    })
                    .negotiated_version = chosen;

                direct_replies.push(
                    VivecraftPacket::Version {
                        version: "Vivecraft-Horizon-Server-0.1.0".to_owned(),
                        vr_active: true,
                        max_network_version: NetworkVersion::MAX_SUPPORTED,
                        min_network_version: NetworkVersion::MIN_SUPPORTED,
                    }
                    .encode(chosen),
                );
                if chosen != NetworkVersion::Legacy {
                    direct_replies.push(
                        VivecraftPacket::NegotiatedNetworkVersion(chosen).encode(chosen),
                    );
                }
                direct_replies.push(VivecraftPacket::RequestData.encode(chosen));
                direct_replies.push(VivecraftPacket::Teleport(None).encode(chosen));
                direct_replies.push(VivecraftPacket::Climbing(true).encode(chosen));
                direct_replies.push(VivecraftPacket::Crawl(true).encode(chosen));
                direct_replies.push(VivecraftPacket::VrSwitching(true).encode(chosen));
                direct_replies.push(VivecraftPacket::DualWielding(true).encode(chosen));
            }
            VivecraftPacket::WorldScale(scale) => {
                if let Some(player) = self.remote_players.get_mut(&player_uuid) {
                    player.world_scale = scale.clamp(0.1, 10.0);
                }
            }
            VivecraftPacket::Height(height) => {
                if let Some(player) = self.remote_players.get_mut(&player_uuid) {
                    player.height_scale = height.clamp(0.3, 3.0);
                }
            }
            VivecraftPacket::VrPlayerState(state) => {
                if let Some(player) = self.remote_players.get_mut(&player_uuid) {
                    player.state = state.clone();
                    broadcast = Some(
                        VivecraftPacket::UberPacket {
                            player_uuid,
                            state,
                            world_scale: player.world_scale,
                            height_scale: player.height_scale,
                        }
                        .encode(player.negotiated_version),
                    );
                }
            }
            _ => {}
        }

        (direct_replies, broadcast)
    }
}

/// Big-endian binary reader matching Minecraft's `FriendlyByteBuf`.
pub struct ByteCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ByteCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_u8()? != 0)
    }

    pub fn read_f32(&mut self) -> Option<f32> {
        let slice = self.bytes.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(f32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    pub fn read_u128(&mut self) -> Option<u128> {
        let slice = self.bytes.get(self.pos..self.pos + 16)?;
        self.pos += 16;
        let mut arr = [0u8; 16];
        arr.copy_from_slice(slice);
        Some(u128::from_be_bytes(arr))
    }

    pub fn read_vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(
            self.read_f32()?,
            self.read_f32()?,
            self.read_f32()?,
        ))
    }

    pub fn read_quat(&mut self) -> Option<Quat> {
        // Vivecraft CommonNetworkHelper.deserializeVivecraftQuaternion reads (w, x, y, z)
        let w = self.read_f32()?;
        let x = self.read_f32()?;
        let y = self.read_f32()?;
        let z = self.read_f32()?;
        Some(Quat::from_xyzw(x, y, z, w).normalize())
    }

    pub fn read_varint(&mut self) -> Option<usize> {
        let mut value = 0usize;
        let mut shift = 0u32;
        loop {
            let byte = self.read_u8()?;
            value |= usize::from(byte & 0x7F) << shift;
            if (byte & 0x80) == 0 {
                return Some(value);
            }
            shift += 7;
            if shift > 28 {
                return None;
            }
        }
    }

    pub fn read_mc_utf(&mut self) -> Option<String> {
        let len = self.read_varint()?;
        let slice = self.bytes.get(self.pos..self.pos + len)?;
        self.pos += len;
        std::str::from_utf8(slice).ok().map(str::to_owned)
    }
}

fn write_bool(out: &mut Vec<u8>, v: bool) {
    out.push(u8::from(v));
}

fn write_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_u128(out: &mut Vec<u8>, v: u128) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_vec3(out: &mut Vec<u8>, v: Vec3) {
    write_f32(out, v.x);
    write_f32(out, v.y);
    write_f32(out, v.z);
}

fn write_quat(out: &mut Vec<u8>, q: Quat) {
    // Vivecraft CommonNetworkHelper.serialize(FriendlyByteBuf, Quaternionf) writes (w, x, y, z)
    let q = q.normalize();
    write_f32(out, q.w);
    write_f32(out, q.x);
    write_f32(out, q.y);
    write_f32(out, q.z);
}

fn write_varint(out: &mut Vec<u8>, mut value: usize) {
    loop {
        if (value & !0x7F) == 0 {
            out.push(value as u8);
            return;
        }
        out.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}

fn write_mc_utf(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_varint(out, bytes.len());
    out.extend_from_slice(bytes);
}
