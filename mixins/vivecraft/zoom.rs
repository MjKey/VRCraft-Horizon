fn vivecraft_zoom(game: &crate::game::Game<'_>, zoom: &mut Option<crate::game::native_hooks::NativeZoom>) {
    crate::game::vivecraft_mod::on_zoom(game, zoom);
}
