fn vivecraft_client_packet(
    game: &mut crate::game::Game<'_>,
    packet: &crate::game::native_hooks::ClientboundPlayPacket,
) {
    crate::game::vivecraft_mod::on_client_packet(game, packet);
}
