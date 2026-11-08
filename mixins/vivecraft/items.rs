//! Custom VR items for ViveCraft Horizon.

pub const ITEM_CLIMBEY_CLAW: &str = "vivecraft:climbey_claw";
pub const ITEM_JUMP_BOOTS: &str = "vivecraft:jump_boots";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomVrItem {
    ClimbeyClaw,
    JumpBoots,
}

impl CustomVrItem {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            ITEM_CLIMBEY_CLAW => Some(Self::ClimbeyClaw),
            ITEM_JUMP_BOOTS => Some(Self::JumpBoots),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::ClimbeyClaw => ITEM_CLIMBEY_CLAW,
            Self::JumpBoots => ITEM_JUMP_BOOTS,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClimbeyClaw => "Climbey Claw",
            Self::JumpBoots => "Jump Boots",
        }
    }
}
