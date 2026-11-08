# ViveCraft Horizon Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Rust port of ViveCraft (Multiloader-26.3) for Minecraft Horizon (26.3) with 100% functional parity.

**Architecture:** Native source-mixin bundle (`.hnmod`, schema 6) hooking into Horizon's client, render, and server crates via 20 native hooks. Core VR engine implemented in `vivecraft-vr` with dynamic OpenXR 1.0 loader, reversed-Z stereo projection, roomscale locomotion, spatial audio, and multiplayer `vivecraft:data` protocol codec.

**Tech Stack:** Rust 2024, OpenXR 1.0 C ABI (`libloading`), `glam`, `bytemuck`, `serde`/`serde_json`, `horizon-mixins` schema 6.

**Spec:** `docs/superpowers/specs/2026-11-09-vivecraft-horizon-port-design.md` and `PLAN.md`.

## Global Constraints
- Pure Rust codebase with zero C/C++ compilation toolchain requirements.
- Strictly adhere to `horizon-mixins` schema 6.
- Maintain deterministic packaging in `tools/package.py`.
- Zero camera freezing: fallback to 6DoF simulation (`NullVR`) when OpenXR hardware is not connected or session is inactive.

## Review Focus
- OpenXR session state machine and reference space (`STAGE`/`LOCAL`/`VIEW`) stability.
- Two-eye Reversed-Z projection matrix math and camera roll without 10° clamping.
- Roomscale voxel collision: stepping up blocks (0.1–1.0m) without clipping into solid walls.
- Multiplayer `vivecraft:data` protocol codec binary parity (32 packet IDs).
- Server AimFix: proper override and restoration of player transform during item/bow use.

---

### Task 1: Complete OpenXR 1.0 Runtime and OSC Receiver (`xr.rs`, `osc.rs`)
**Files:**
- Create: `mixins/vivecraft/horizon-vr/src/osc.rs`
- Modify: `mixins/vivecraft/horizon-vr/src/xr.rs`
- Modify: `mixins/vivecraft/horizon-vr/src/lib.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Implement `osc.rs` UDP non-blocking socket listener on `127.0.0.1:9000` for VRChat OSC tracker packets (`/tracking/trackers/{1..7}/position` and `/rotation`).
- [x] Expand `xr.rs` with full OpenXR 1.0 function pointer table: session lifecycle (`xrCreateSession`, `xrBeginSession`, `xrEndSession`), reference spaces (`STAGE`, `LOCAL`), action sets (`/actions/gameplay`), and haptics (`xrApplyHapticFeedback`).
- [x] Ensure robust fallback to `SimulatedXrProvider` so camera never freezes at `(0,0,0)` if session is not `FOCUSED`.
- [x] Run `cargo test -p vivecraft-vr` and verify all tests pass.

---

### Task 2: Stereo Rendering Pipeline, Reversed-Z and Head Roll (`stereo.rs`, `stereo_views.rs`, `camera_shake.rs`)
**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/stereo.rs`
- Modify: `mixins/vivecraft/stereo_views.rs`
- Modify: `mixins/vivecraft/camera_shake.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Verify asymmetric Reversed-Z OpenXR projection matrix in `stereo.rs`.
- [x] In `stereo_views.rs`, pass two distinct view matrices with IPD offset and full 6DoF orientation.
- [x] In `camera_shake.rs`, remove the 10° clamp on roll so player head can tilt 360°.
- [x] Add stencil mesh (hidden area mask) calculation and comfort vignette helpers.
- [x] Run `cargo test -p vivecraft-vr` and verify stereo math tests.

---

### Task 3: Roomscale Physics, Anti-Wall Clipping and Locomotion Trackers (`locomotion.rs`, `runtime.rs`)
**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/locomotion.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] In `runtime.rs::on_tick`, apply `roomscale_world_delta` and `velocity_impulse` via `game.native_push_player`.
- [x] Implement `VRPlayer::doPlayerMoveInRoom` roomscale collision physics:
  - Check collision with voxels via `is_solid_block`.
  - Auto-step up 0.1–1.0m ledges.
  - Push back if colliding with solid walls to prevent wall-hacking.
- [x] Add realistic sneak (`<= 1.5m`) and crawl (`<= 0.75m` swimming pose) thresholds.
- [x] Complete specialized trackers: `ClimbTracker`, `SwimTracker`, `RowTracker`, `HorseTracker`, `VehicleTracker`, and `JumpTracker`.
- [x] Run `cargo test -p vivecraft-vr` and verify locomotion tests pass.

---

### Task 4: Combat, Archery, Interactions & VR Elytra (`interaction.rs`, `runtime.rs`)
**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/interaction.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Implement two-handed bow and crossbow drawing with haptic tension.
- [x] Implement physical melee and mining with tip velocity $>2.5$ m/s and DDA raycasting.
- [x] Implement roomscale shield angle blocking.
- [x] Implement physical food/potion eating at mouth ($<0.14$m) with crunch haptics.
- [x] Implement VR Elytra flight (arms out glide, pitch/roll steering, flap impulse).
- [x] Implement spyglass zoom, backpack slot 0 reach, screenshot camera, door haptics, and enderman gaze.
- [x] Run `cargo test -p vivecraft-vr`.

---

### Task 5: 3D Spatial Audio HRTF Orientation (`mixins/vivecraft/runtime.rs`)
**Files:**
- Modify: `mixins/vivecraft/runtime.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Bind audio listener position to central HMD eye world coordinates.
- [x] Calculate full 6DoF listener basis (forward, up, right) using HMD orientation quaternion including roll.
- [x] Route directional damage haptic feedback to the respective controller.
- [x] Run `cargo test -p vivecraft-vr`.

---

### Task 6: 3D Spatial GUI, Radial Menu, Virtual Keyboard & Menu Worlds (`gui.rs`, `runtime.rs`, `world_primitives.rs`)
**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/gui.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Modify: `mixins/vivecraft/world_primitives.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Implement floating 3D screen with laser pointer raycasting and mouse input emulation.
- [x] Implement 8-slice radial menu with analog stick selection and tactile ticks.
- [x] Implement 3D virtual keyboard with both laser click and physical drumstick touch typing.
- [x] Implement wrist HUD bar with glance detection.
- [x] Render 3D Menu Worlds diorama for main menu background.
- [x] Run `cargo test -p vivecraft-vr`.

---

### Task 7: Multiplayer `vivecraft:data` Protocol, Server AimFix & Headshots (`protocol.rs`, `runtime.rs`)
**Files:**
- Modify: `mixins/vivecraft/horizon-vr/src/protocol.rs`
- Modify: `mixins/vivecraft/runtime.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Ensure complete codec for all 32 `vivecraft:data` packets (C2S and S2C).
- [x] Implement server-side `AimFixHandler`: override player position/pitch/yaw during item use and restore immediately after.
- [x] Implement arrow headshot multiplier (3.0x damage, crit particles, ding sound).
- [x] Implement dynamic server hitbox height (1.8m standing, 1.5m sneak, 0.6m crawl).
- [x] Render 3D avatars for remote VR players in `collect_world_primitives`.
- [x] Run `cargo test -p vivecraft-vr`.

---

### Task 8: Custom Items, Recipes & Tags
**Files:**
- Create: `mixins/vivecraft/items.rs`
- Create: `mixins/vivecraft/recipes.rs`
- Create: data JSON recipes for Climb Claws & Jump Boots
- Modify: `mixins/vivecraft/runtime.rs`
- Test: `mixins/vivecraft/horizon-vr/src/lib.rs`

**Steps:**
- [x] Register Climb Claws (`vivecraft:climbey_claw`) and Jump Boots (`vivecraft:jump_boots`).
- [x] Add recipes in `data/vivecraft/recipe/`.
- [x] Add tag definitions (`climbable`, `bows`, `shields`).
- [x] Run `cargo test -p vivecraft-vr`.

---

### Task 9: Expand `horizon-mixin.toml` to 20 Native Hooks
**Files:**
- Modify: `mixins/vivecraft/horizon-mixin.toml`
- Create helper `.rs` files for any new injections (`arrow_nock.rs`, `arrow_loose.rs`, `client_attack.rs`, `screen_mouse.rs`, `screen_char.rs`, `menu_background.rs`, `zoom.rs`, `player_submit.rs`)
- Modify: `tools/test_stage.py` (adjust expected injection count)
- Test: `tools/package.py` and `tools/test_stage.py`

**Steps:**
- [x] Add all 20 injection points in `horizon-mixin.toml`.
- [x] Create missing helper files with correct bindings and forward calls to `runtime.rs`.
- [x] Update `tools/test_stage.py` injection assertions.

---

### Task 10: Verification, Packaging & GitHub Push
**Files:**
- Run: `cargo test -p vivecraft-vr --manifest-path mixins/vivecraft/horizon-vr/Cargo.toml`
- Run: `python tools/package.py`
- Run: `git status`, `git add`, `git commit`
- Push: `git push origin main`

**Steps:**
- [x] Verify 100% passing tests in `vivecraft-vr`.
- [x] Package reproducible `.hnmod` bundle via `tools/package.py`.
- [x] Review git diff and ensure clean tree.
- [ ] Commit all changes and push to `https://github.com/MjKey/VRCraft-Horizon`.
