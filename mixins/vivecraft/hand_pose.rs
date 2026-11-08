fn vivecraft_hand_pose(
    game: &crate::game::Game<'_>,
    hand: crate::first_person::Hand,
    partial: f32,
    x: &mut f32,
    y: &mut f32,
    z: &mut f32,
    pitch: &mut f32,
    yaw: &mut f32,
    roll: &mut f32,
    scale: &mut f32,
) {
    crate::game::vivecraft_mod::on_hand_pose(game, hand, partial, x, y, z, pitch, yaw, roll, scale);
}
