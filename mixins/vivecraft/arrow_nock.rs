fn vivecraft_arrow_nock(
    game: &mut crate::game::Game<'_>,
    player: u128,
    name: &str,
    bow: &crate::items::ItemStack,
    hand: crate::first_person::Hand,
    has_ammo: bool,
    cancel: &mut bool,
) {
    crate::game::vivecraft_mod::on_arrow_nock(game, player, name, bow, hand, has_ammo, cancel);
}
