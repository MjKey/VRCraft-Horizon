fn vivecraft_critical_hit(
    game: &mut crate::game::Game<'_>,
    target_kind: &str,
    vanilla_critical: bool,
    critical: &mut bool,
    damage_multiplier: &mut f32,
) {
    crate::game::vivecraft_mod::on_critical_hit(game, target_kind, vanilla_critical, critical, damage_multiplier);
}