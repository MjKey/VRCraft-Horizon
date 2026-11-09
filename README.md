# ViveCraft VR for Minecraft Horizon (`vivecraft`)

An independent Rust port of [**Vivecraft**](https://github.com/Vivecraft/VivecraftMod) (`Multiloader-26.3`) for [**Minecraft Horizon**](https://github.com/horizon-rust/horizon) (`26.3`), built as a native source mixin bundle (`.hnmod`) using `horizon-mixins` schema 6.

---

## Features

- **OpenXR 1.0 & Simulated 6DoF Stereo Rendering (`src/xr.rs`, `src/stereo.rs`)**:
  - Dynamic Khronos OpenXR 1.0 C ABI loader (`openxr_loader.dll` / `libopenxr_loader.so.1` / `libopenxr_loader.dylib`) with zero external C/C++ build dependencies.
  - Automatic fallback to a 6DoF roomscale simulator (`NullVR`) when no VR runtime or headset is connected.
  - Asymmetric eye-FOV (`tan_left`, `tan_right`, `tan_up`, `tan_down`) reversed-Z right-handed perspective projection (`z_near -> 1.0`, `z_far -> 0.0`) injected into `horizon_client_render::gfx::view::ViewSet::single` (`len = 2` for Left & Right eyes).
- **Full-Body Tracking (FBT) & Haptics (`src/xr.rs`, `src/vr_data.rs`)**:
  - Waist, left/right foot, left/right knee, and left/right elbow trackers (`FbtMode::ArmsOnly`, `WithJoin`, `WithKneel`) via OpenXR and OSC (`OSCTracker`).
  - Per-controller microsecond haptic pulse scheduling (`HapticPulse`) and directional damage haptics (`HapticTracker`).
- **All 10 VR Locomotion Trackers (`src/locomotion.rs`)**:
  - `TeleportTracker` — 50-step parabolic arc trajectory solver (`g = 0.098`, `v0 = 0.5`) with landing surface validation.
  - `SneakTracker` — Physical head-height crouch threshold (`<= 1.22m`).
  - `CrawlTracker` — Physical prone crawling (`<= 0.75m`) with server synchronization (`CrawlPayloadC2S`).
  - `JumpTracker` — Roomscale upward jump gesture and Climbey Jump-Boots latch-and-throw propulsion (`JumpingPayloadC2S`).
  - `ClimbTracker` — Physical ladder/vine grabbing and Climbey Claws block climbing (`750/2000 us` haptics + `ClimbingPayloadC2S`).
  - `SwimTracker` — Breaststroke underwater propulsion (`SWIM_SPEED = 1.3`, `FRICTION = 0.9`).
  - `RunTracker` — Arm-swing run-in-place locomotion (`FreeMoveOrientation::RunInPlace`).
  - `RowTracker` — Physical two-oar boat paddling (`LOar`, `ROar`, `FOar`) with differential yaw torque.
  - `VehicleTracker` — Vehicle rotation alignment and roomscale dismount gesture (`> 1.0m`).
  - `HorseTracker` — Physical reins pull and whip boost.
- **Roomscale Combat, Archery & Interactions (`src/interaction.rs`)**:
  - `SwingTracker` — Tip-velocity melee and block mining (`>= 2.5 m/s`) with 3D Amanatides-Woo DDA voxel raycasting (`raycast_voxels`) and `RoomscaleAttackPayloadC2S`.
  - `BowTracker` — Two-handed roomscale bow drawing, draw tension (`0.0..=1.0`), and arrow release (`DrawPayloadC2S`).
  - `BackpackTracker` — Over-the-shoulder reach behind the HMD for hotbar slot 0 and offhand swapping (`1500 us` haptic feedback).
  - `EatingTracker` — Physical mouth-distance (`< 0.25m`) eating and bottle-tilt drinking (`2100 ms` cycle with `700 us` crunch haptics).
  - `TelescopeTracker` — Spyglass eye-to-lens distance and alignment zoom factor (`0.0..=1.0`).
  - `CameraTracker` — Handheld 3D screenshot camera widget with grab handle, shutter button, and 3D viewfinder quad.
  - `InteractTracker` — Priority-sorted interaction dispatcher (`InteractiveHotbar` -> `RoomscaleBow` -> `ScreenshotCamera` -> `EntityInteract` -> `BlockInteract`).
- **3D Spatial GUI, Radial Menu, Keyboard & Wrist HUD (`src/gui.rs`)**:
  - `GuiHandler` — 3D floating HUD/GUI quad solver with controller ray-quad intersection, `0.7/0.3` cursor smoothing, and wrist HUD glance detection.
  - `RadialHandler` — 8-slice interactive radial menu wheel.
  - `KeyboardHandler` — Floating 3D QWERTY keyboard and physical drum-touch keyboard typing.
  - `InteractiveHotbarModule` — Touch-sensitive 3D hotbar slots (`0..=8`) and offhand slot (`9`).
- **Multiplayer `vivecraft:data` Protocol & 3D Remote VR Skeletons (`src/protocol.rs`, `runtime.rs`)**:
  - Binary wire-compatible `vivecraft:data` protocol (`PayloadIdentifier` `0..=31`, `NetworkVersion` `-1..=10`, `VrPlayerState`, `UberPacket`) C2S and S2C codec.
  - 3D rendering of remote VR players (HMD visor, left/right controllers, FBT waist and feet trackers) in `collect_world_primitives`.

---

## Install & Play

### Option 1: Install from GitHub via Horizon Launcher

```sh
horizon-launcher mods add-github https://github.com/MjKey/VRCraft-Horizon --trust-native-code
horizon-launcher native-build --trust-native-code
horizon-launcher play
```

### Option 2: Build from a Local Checkout

```sh
horizon-launcher native-build mixins/vivecraft --trust-native-code
horizon-launcher play
```

### Option 3: Import a Pre-Built `horizon-client` Binary

If a friend sends you their compiled `horizon-client.exe` (or `horizon-client` binary):

```sh
horizon-launcher import-build path/to/horizon-client.exe --trust-unsigned
horizon-launcher play
```

---

## Default VR Keybindings & Controls

Configure settings in-game under **Options > Mods > ViveCraft**, or use the default hotkeys:

| Key | Action |
| --- | --- |
| `F7` | Toggle VR Stereo Mode On/Off |
| `F8` | Recenter HMD Yaw & Seated Origin |
| `R` | Open / Close 8-Slice Radial Menu |
| `K` | Open / Close 3D Virtual VR Keyboard |
| `C` | Toggle Handheld 3D Screenshot Camera |
| `T` | Trigger Parabolic VR Teleport |

---

## Development & Verification

Compatible revisions pinned in [`compatibility.json`](compatibility.json):
- **Horizon**: `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` (`nightly-2026-11-08-fdc87ae`, Minecraft `26.3`)
- **Horizon Mixins**: `5a64af938ec86b00d53d16a408aabc0038fb3e57` (`0.6.0`, plan schema 6)

```sh
# 1. Run standalone VR engine unit tests (OpenXR, Stereo, Locomotion, Interaction, GUI, Protocol)
cargo test -p vivecraft-vr --manifest-path mixins/vivecraft/horizon-vr/Cargo.toml

# 2. Package deterministic source-only bundle (target/vivecraft-0.1.3.hnmod)
python3 tools/package.py

# 3. Stage and audit AST with a local Horizon & Horizon Mixins checkout
python3 tools/horizon.py stage --horizon ../horizon
```

---

## Credits & License

- **License**: [MIT](LICENSE)
- **Original Mod**: Inspired by and protocol-compatible with [Vivecraft](https://github.com/Vivecraft/VivecraftMod) (`Multiloader-26.3`) by the Vivecraft contributors.
- Independently written Rust code for Horizon; no upstream Java code, Mojang code, or game assets are bundled.
