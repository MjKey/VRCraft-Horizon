//! Custom VR items for ViveCraft Horizon (`Climb Claws`, `Jump Boots`, Alice easter eggs).
//!
//! As in Java Vivecraft (`org.vivecraft.data.ViveItems`), these are NOT new registry ids:
//! they are standard vanilla items named and flagged, so VR players can play on ordinary
//! vanilla servers without crashes or desynchronisation:
//! - `Climb Claws` = vanilla shears named "Climb Claws", `minecraft:unbreakable`;
//! - `Jump Boots` = vanilla leather boots named "Jump Boots", `minecraft:unbreakable`,
//!   dyed `0x8CE56F`;
//! - `EAT ME` = pumpkin pie named "EAT ME" (grows world scale);
//! - `DRINK ME` = potion named "DRINK ME" (shrinks world scale).

/// Vanilla shears named `"Climb Claws"` (`vivecraft.item.climbclaws`).
pub const CLIMB_CLAWS_NAME: &str = "Climb Claws";
/// Vanilla leather boots named `"Jump Boots"` (`vivecraft.item.jumpboots`).
pub const JUMP_BOOTS_NAME: &str = "Jump Boots";
/// Pumpkin pie named `"EAT ME"` (Alice easter egg).
pub const EAT_ME_NAME: &str = "EAT ME";
/// Potion named `"DRINK ME"` (Alice easter egg).
pub const DRINK_ME_NAME: &str = "DRINK ME";
/// `0x8CE56F` slime-green for the Jump Boots' `minecraft:dyed_color`.
pub const JUMP_BOOTS_DYED_COLOR: i32 = 0x8CE56F;

/// Recognizes the custom VR item of a vanilla stack by id + custom name (+ flags),
/// mirroring Java Vivecraft's `ViveItems` NBT checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomVrItem {
    /// Vanilla `minecraft:shears`, named "Climb Claws", unbreakable.
    ClimbeyClaw,
    /// Vanilla `minecraft:leather_boots`, named "Jump Boots", unbreakable, dyed green.
    JumpBoots,
    /// Vanilla `minecraft:pumpkin_pie`, named "EAT ME".
    EatMeCake,
    /// Vanilla `minecraft:potion`, named "DRINK ME".
    DrinkMePotion,
}

/// Whether the vanilla stack is the given named/flagged VR item
/// (id, plain custom name and component checks, per `ViveItems`).
pub fn custom_item_of(
    item_id: &str,
    custom_name: Option<&str>,
    unbreakable: bool,
    dyed_color: Option<i32>,
) -> Option<CustomVrItem> {
    let name = custom_name?;
    match item_id {
        "minecraft:shears" if unbreakable && name == CLIMB_CLAWS_NAME => {
            Some(CustomVrItem::ClimbeyClaw)
        }
        "minecraft:leather_boots"
            if unbreakable
                && name == JUMP_BOOTS_NAME
                && dyed_color == Some(JUMP_BOOTS_DYED_COLOR) =>
        {
            Some(CustomVrItem::JumpBoots)
        }
        "minecraft:pumpkin_pie" if name == EAT_ME_NAME => Some(CustomVrItem::EatMeCake),
        "minecraft:potion" if name == DRINK_ME_NAME => Some(CustomVrItem::DrinkMePotion),
        _ => None,
    }
}

impl CustomVrItem {
    /// Convenience wrapper for stacks without component data (plain check by name).
    pub fn from_id_and_name(item_id: &str, custom_name: Option<&str>) -> Option<Self> {
        custom_item_of(item_id, custom_name, true, Some(JUMP_BOOTS_DYED_COLOR))
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClimbeyClaw => "Climb Claws",
            Self::JumpBoots => "Jump Boots",
            Self::EatMeCake => "Cake 'EAT ME'",
            Self::DrinkMePotion => "Potion 'DRINK ME'",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climb_claws_are_named_unbreakable_shears() {
        assert_eq!(
            custom_item_of("minecraft:shears", Some(CLIMB_CLAWS_NAME), true, None),
            Some(CustomVrItem::ClimbeyClaw)
        );
        // Breakable or differently-named shears are not claws.
        assert_eq!(custom_item_of("minecraft:shears", Some("Shears"), true, None), None);
        assert_eq!(
            custom_item_of("minecraft:shears", Some(CLIMB_CLAWS_NAME), false, None),
            None
        );
    }

    #[test]
    fn jump_boots_are_named_green_unbreakable_leather_boots() {
        assert_eq!(
            custom_item_of(
                "minecraft:leather_boots",
                Some(JUMP_BOOTS_NAME),
                true,
                Some(JUMP_BOOTS_DYED_COLOR)
            ),
            Some(CustomVrItem::JumpBoots)
        );
        // Wrong colour or missing unbreakable flag: not boots.
        assert_eq!(
            custom_item_of("minecraft:leather_boots", Some(JUMP_BOOTS_NAME), true, None),
            None
        );
    }

    #[test]
    fn eat_me_and_drink_me_are_named_vanilla_items() {
        assert_eq!(
            custom_item_of("minecraft:pumpkin_pie", Some(EAT_ME_NAME), false, None),
            Some(CustomVrItem::EatMeCake)
        );
        assert_eq!(
            custom_item_of("minecraft:potion", Some(DRINK_ME_NAME), false, None),
            Some(CustomVrItem::DrinkMePotion)
        );
        assert_eq!(custom_item_of("minecraft:bread", Some(EAT_ME_NAME), false, None), None);
    }
}