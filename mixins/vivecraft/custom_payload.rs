fn vivecraft_custom_payload(
    game: &mut crate::game::Game<'_>,
    player: u128,
    channel: &str,
    data: &[u8],
) {
    crate::game::vivecraft_mod::on_custom_payload_from_client(game, player, channel, data);
}
