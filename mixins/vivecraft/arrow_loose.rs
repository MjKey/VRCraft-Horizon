fn vivecraft_arrow_loose(
    game: &mut crate::game::Game<'_>,
    stack: &crate::items::ItemStack,
    has_ammo: bool,
    charge: &mut i32,
    canceled: &mut bool,
) {
    crate::game::vivecraft_mod::on_arrow_loose(game, stack, has_ammo, charge, canceled);
}
