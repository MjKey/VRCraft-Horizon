# ViveCraft Horizon Audit Remediation & Full Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Achieve 100% functional parity between ViveCraft (Java Multiloader-26.3) and Minecraft Horizon (Rust 26.3), resolving every technical gap and blocker identified in `audit.md`.

**Architecture:** Native source-mixin mod (`mixins/vivecraft`, schema 6) hooking into Horizon's client, render, and server systems. Core VR engine in `vivecraft-vr` with full OpenXR 1.0 session lifecycle, action sets, haptics, unrestricted 360° roll Reversed-Z stereo ViewSet, roomscale physics with step-up & wall anti-clip, complete 10 locomotion trackers, physical combat/archery/eating/shield/elytra, 3D laser GUI & virtual keyboard, remote VR avatars, server AimFix, 3.0x headshots, HRTF spatial audio, and custom items/recipes.

**Tech Stack:** Rust 2024, OpenXR 1.0 C ABI (`libloading`), `glam`, `bytemuck`, `serde`/`serde_json`, `horizon-mixins` schema 6.

**Spec:** `audit.md` and `PLAN.md`.

## Global Constraints
- Pure Rust codebase with zero C/C++ compilation toolchain requirements.
- Strictly adhere to `horizon-mixins` schema 6.
- Maintain deterministic packaging in `tools/package.py`.
- Zero camera freezing: robust fallback to 6DoF simulation (`NullVR`) whenever physical OpenXR is not focused or poses are invalid.
- Infrequent builds and tests to preserve CPU resources per user constraint ("Если возможно делай тесты и сборку реже, т.к. комп слабый").

## Review Focus
1. OpenXR session creation with headless/extension negotiation, avoiding `XR_NULL_HANDLE` and activating real `xrApplyHapticFeedback`.
2. 360° head roll embedded directly into `View.view` matrix in `stereo_views.rs`, bypassing Horizon's 10° shake roll clamp.
3. Roomscale physical collision with voxel step-up (0.1–1.0m) and anti-wall-clipping pushback.
4. Laser UV to screen coordinate mapping for 3D GUI button clicks and text typing.
5. Server AimFix controller transform substitution during player actions and 3.0x headshots.

---

### Task 1: OpenXR 1.0 Real Session Lifecycle, Reference Spaces, Actions & Haptics

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/xr.rs`
- Modify: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Interfaces:**
- Consumes: `libloading::Library`, OpenXR 1.0 C ABI procedure pointers.
- Produces: Active `XrSession`, `XrSpace` (`STAGE`/`LOCAL`/`VIEW`), action spaces for aim/grip, real `xrSyncActions`/`xrLocateSpace`, and `xrApplyHapticFeedback` in `drain_pending_haptics`.

- [ ] **Step 1: Write test verifying OpenXR session structures and haptic dispatch in `mixins/vivecraft/horizon-vr/src/lib.rs`**
- [ ] **Step 2: Implement headless/standard session creation, action set `/actions/gameplay`, action spaces, and haptic feedback in `xr.rs`**
  - Enable `XR_MND_headless` / `XR_KHR_headless` during `xrCreateInstance` when supported.
  - Implement session creation with fallback to headless or graphics-free state.
  - Create reference spaces: `XR_REFERENCE_SPACE_TYPE_STAGE`, `XR_REFERENCE_SPACE_TYPE_LOCAL`, `XR_REFERENCE_SPACE_TYPE_VIEW`.
  - Create Action Set `/actions/gameplay` with actions: `aim_pose`, `grip_pose`, `trigger`, `squeeze`, `thumbstick`, `primary_button`, `secondary_button`, `haptic`.
  - Suggest bindings for Oculus Touch, Valve Index, HTC Vive, and Simple Controller.
  - Attach action sets to session via `xrAttachSessionActionSets` and create action spaces for left/right hands.
  - Implement real `xrApplyHapticFeedback` inside `drain_pending_haptics`.
  - Ensure `hardware_connected` is only true when session is active and poses are valid; otherwise fall back cleanly to `sync_simulated_pose` to prevent camera freeze.

---

### Task 2: Stereo ViewSet Pipeline & Unrestricted 360° Head Roll

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/stereo.rs`
- Modify: `mixins/vivecraft/horizon-vr/src/vr_data.rs`
- Modify: `mixins/vivecraft/horizon-vr/src/lib.rs`
- Modify: `mixins/vivecraft/vr_bridge.rs`
- Modify: `mixins/vivecraft/stereo_views.rs`

**Interfaces:**
- Consumes: `View`, `ViewSet`, 6DoF HMD rotation quaternion and IPD.
- Produces: `StereoViewPair` with full 6DoF rotation (including 360° roll) and eye offsets in `View.view`.

- [ ] **Step 1: Embed full 6DoF orientation quaternion into `current_stereo_view_pair` in `stereo.rs` and `lib.rs`**
  - Calculate `view_rot = Mat4::from_quat(hmd_rot.conjugate())`.
  - Left eye: `view_rot * Mat4::from_translation(Vec3::new(half_ipd, 0.0, 0.0))`.
  - Right eye: `view_rot * Mat4::from_translation(Vec3::new(-half_ipd, 0.0, 0.0))`.
  - Combined `cull` frustum encompassing both eye fields of view.
- [ ] **Step 2: Update `stereo_views.rs` to pass full 6DoF stereo view matrices directly to Horizon `ViewSet`**
  - Eliminates dependence on Horizon's 10° camera shake roll clamp.

---

### Task 3: Locomotion Pipeline, Roomscale Collision, Steps & All 10 Trackers

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/locomotion.rs`
- Modify: `mixins/vivecraft/runtime.rs`

**Interfaces:**
- Consumes: HMD and controller tracking poses, voxel collision queries.
- Produces: Physical roomscale deltas, auto-step ledge climbing (0.1–1.0m), anti-wall clipping pushback, swim/row/vehicle/horse tracker updates, and teleport execution.

- [ ] **Step 1: Wire all inactive locomotion trackers in `LocomotionState::update_tick` in `locomotion.rs`**
  - Call `self.swim_tracker.update(...)` with arm strokes in water.
  - Call `self.row_tracker.update(...)` with dual oar paddling in boat.
  - Call `self.vehicle_tracker.should_roomscale_dismount(...)` when stepping $>1.0$m away from seat.
  - Complete `HorseTracker`: calculate virtual reins tension and downward whip gesture for speed boost.
- [ ] **Step 2: Implement full roomscale stepping and wall collision in `locomotion.rs` and `runtime.rs`**
  - Check target head/feet voxel collision; if obstructed, step upward in 0.1m increments up to 1.0m.
  - If obstacle is $>1.0$m (solid wall), push back `room_origin` to prevent clipping/viewing through walls.
  - Update dynamic sneaking ($<1.5$m) and crawling ($<0.75$m) flags.
- [ ] **Step 3: Connect teleport destination in `runtime.rs`**
  - When `movement_out.teleport_target` is present, execute teleport command / send `TeleportPayloadC2S`.

---

### Task 4: Combat, Two-Handed Archery, Physical Eating, Shields & VR Elytra

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/interaction.rs`
- Modify: `mixins/vivecraft/runtime.rs`

**Interfaces:**
- Consumes: Controller poses, held items, weapon velocities.
- Produces: Realistic two-handed bow drawing, melee swing damage with $>2.5$ m/s threshold, food consumption at mouth, shield blocking, and VR elytra flight.

- [ ] **Step 1: Replace hardcoded `holding_bow: false` with live held item checks in `runtime.rs`**
  - Check player main hand and offhand items for `minecraft:bow` or `minecraft:crossbow`.
- [ ] **Step 2: Connect physical melee `SwingEvent` into attack hits and mining in `runtime.rs`**
  - Check weapon tip velocity $>2.5$ m/s; trigger haptic feedback and forward attack.
- [ ] **Step 3: Correct mouth position formula in `interaction.rs` and invoke `eating_tracker.update()` in `runtime.rs::on_tick`**
  - Mouth position: `hmd_pos + hmd_rot * Vec3::new(0.0, -0.12, -0.05)`.
  - When edible item is $<0.14$m from mouth, trigger consumption and bite haptics.
- [ ] **Step 4: Implement roomscale shield angle blocking and VR Elytra flight in `interaction.rs`**
  - Shield blocking: check angle between shield forward and incoming projectile/attack.
  - VR Elytra: calculate glide pitch/roll from outstretched arm angles; downward flaps give vertical lift.
- [ ] **Step 5: Add door/trapdoor haptic feedback and fishing rod bite vibration in `runtime.rs`**

---

### Task 5: 3D Spatial GUI Interaction, Virtual Keyboard, Remote Avatars & 3D Diorama

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/gui.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Modify: `mixins/vivecraft/screen_mouse.rs`
- Modify: `mixins/vivecraft/screen_char.rs`
- Modify: `mixins/vivecraft/player_submit.rs`
- Modify: `mixins/vivecraft/menu_background.rs`

**Interfaces:**
- Consumes: Controller laser ray, 3D GUI quad transform, remote player tracking data.
- Produces: Emulated mouse position/clicks on screen, virtual keyboard typing, remote VR avatars with 3D limb poses, and unblurred menu world diorama.

- [ ] **Step 1: Implement laser ray UV to screen pixel translation in `screen_mouse.rs` and `runtime.rs`**
  - Convert laser intersection $(u, v)$ on 3D GUI quad to screen cursor $(u \cdot width, v \cdot height)$.
  - Emulate mouse clicks on trigger pull with haptic feedback.
- [ ] **Step 2: Implement virtual keyboard character forwarding in `screen_char.rs` and `runtime.rs`**
  - Deliver typed characters into active text fields.
- [ ] **Step 3: Implement remote VR player avatar limb rendering in `player_submit.rs` and `runtime.rs`**
  - Apply head tilt, body rotation, and hand controller positions from `network.remote_players`.
- [ ] **Step 4: Keep unblurred 3D live background for menu diorama in `menu_background.rs`**

---

### Task 6: Multiplayer Server AimFix, 3.0x Headshots, Dynamic Hitbox & HRTF Audio

**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/protocol.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Modify: `mixins/vivecraft/items.rs`

**Interfaces:**
- Consumes: Network packets, HMD quaternion, arrow hit positions.
- Produces: Server AimFix player position overrides, 3.0x headshot damage multiplier, dynamic AABB hitbox heights, 6DoF HRTF listener basis, and custom item registrations.

- [ ] **Step 1: Implement server AimFix coordinate override in `runtime.rs`**
  - Store remote VR player controller aim position and direction; substitute during item use / projectile spawn.
- [ ] **Step 2: Implement server arrow headshot detection with 3.0x damage in `runtime.rs`**
  - Check arrow hit height $y > entity.eye\_y - 0.25$; apply 3.0x multiplier, crit particles, and break sound.
- [ ] **Step 3: Dynamically adjust player hitbox height on server**
  - 1.8m standing, 1.5m sneaking, 0.6m crawling.
- [ ] **Step 4: Inject full HMD roll quaternion into HRTF spatial audio listener basis**
  - Listener basis: `forward = hmd_quat * Vec3::NEG_Z`, `up = hmd_quat * Vec3::Y`, `right = hmd_quat * Vec3::X`.
- [ ] **Step 5: Register Climbey Claws, Jump Boots, and Alice easter eggs in `items.rs`**

---

### Task 7: Verification, AST Validation, Packaging & Git Push

**Files:**
- Output: `target/vivecraft-0.1.2.hnmod`
- Git: Pushed to `origin/main`

**Steps:**
- [ ] **Step 1: Run unit tests: `cargo test -p vivecraft-vr --manifest-path mixins/vivecraft/horizon-vr/Cargo.toml`**
- [ ] **Step 2: Run AST validation: `python tools/test_stage.py`**
- [ ] **Step 3: Package `.hnmod` bundle: `python tools/package.py`**
- [ ] **Step 4: Commit all changes and push to `https://github.com/MjKey/VRCraft-Horizon`**
