// The "VR: On/Off" button on the main menu (spec 0.1.6 "VR toggle").
//
// Drawn through the title's `native_screen_render_post` (API 326: the title's
// own screens fire it after they drew, before the toasts) — the mod draws
// over the title screen exactly where the engine tells native mods to. When
// the screen id is "title" (Mode::Main) and the mod is on, this draws a
// vanilla-styled 200x20 button under Options/Quit with `ui::button`, the
// REGISTERING helper (CLAUDE.md "Controllers reach what registers"): its
// rect is recorded for the pad's focus, the narrator and the widget cursor,
// so a controller's A presses it too.
//
// The label is translation-ready: `vivecraft.vr_toggle` ("VR Mode") plus
// vanilla's `options.on` / `options.off` keys — no hardcoded UI strings.
//
// Clicks come back through the existing `screen_mouse` injection
// (`native_screen_mouse_button_pressed_pre`, which fires for the title's
// screens too): a left press inside the stored rect flips `vr_enabled`,
// persists the mod's settings, does the session teardown when turning off
// (ending any pending OpenXR frame — the session itself stays alive and is
// re-used when VR is turned back on; a full xrDestroySession is unnecessary
// for a settings toggle) and cancels the press so the title takes it as
// nothing. The rect is stored in the runtime (shared global state), keyed to
// the GUI size it was drawn at.
fn vivecraft_title_vr_button(
    game: &crate::game::Game<'_>,
    batch: &mut crate::hud::HudBatch,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
) {
    crate::game::vivecraft_mod::on_title_screen_render_post(game, batch, screen);
}
