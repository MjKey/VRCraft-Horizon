//! Custom VR items for ViveCraft Horizon (`vivecraft:climbey_claw`, `vivecraft:jump_boots`, Alice easter eggs).

pub const ITEM_CLIMBEY_CLAW: &str = "vivecraft:climbey_claw";
pub const ITEM_JUMP_BOOTS: &str = "vivecraft:jump_boots";
pub const ITEM_EAT_ME_CAKE: &str = "vivecraft:eat_me";
pub const ITEM_DRINK_ME_POTION: &str = "vivecraft:drink_me";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomVrItem {
    ClimbeyClaw,
    JumpBoots,
    EatMeCake,
    DrinkMePotion,
}

impl CustomVrItem {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            ITEM_CLIMBEY_CLAW => Some(Self::ClimbeyClaw),
            ITEM_JUMP_BOOTS => Some(Self::JumpBoots),
            ITEM_EAT_ME_CAKE => Some(Self::EatMeCake),
            ITEM_DRINK_ME_POTION => Some(Self::DrinkMePotion),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::ClimbeyClaw => ITEM_CLIMBEY_CLAW,
            Self::JumpBoots => ITEM_JUMP_BOOTS,
            Self::EatMeCake => ITEM_EAT_ME_CAKE,
            Self::DrinkMePotion => ITEM_DRINK_ME_POTION,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClimbeyClaw => "Climbey Claw",
            Self::JumpBoots => "Jump Boots",
            Self::EatMeCake => "Cake 'EAT ME'",
            Self::DrinkMePotion => "Potion 'DRINK ME'",
        }
    }
}
