#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CooldownStatus {
    pub active: bool,
    /// Milliseconds remaining when the client retains an expiry tick.
    pub remaining_ms: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryItem {
    pub slot: u8,
    pub sprite: u16,
    pub dye_color: u8,
    pub name: Option<String>,
    pub quantity: u32,
    pub can_stack: bool,
    pub durability: u32,
    pub max_durability: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquipmentItem {
    pub slot: EquipmentSlot,
    pub sprite: u16,
    pub dye_color: u8,
    pub name: Option<String>,
    pub durability: u32,
    pub max_durability: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquipmentSlot {
    Weapon,
    Armor,
    Shield,
    Helmet,
    Earrings,
    Necklace,
    LeftRing,
    RightRing,
    LeftGauntlet,
    RightGauntlet,
    Belt,
    Greaves,
    Boots,
    Accessory1,
    Overcoat,
    OverHelm,
    Accessory2,
    Accessory3,
}

impl EquipmentSlot {
    #[must_use]
    pub const fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Weapon,
            2 => Self::Armor,
            3 => Self::Shield,
            4 => Self::Helmet,
            5 => Self::Earrings,
            6 => Self::Necklace,
            7 => Self::LeftRing,
            8 => Self::RightRing,
            9 => Self::LeftGauntlet,
            10 => Self::RightGauntlet,
            11 => Self::Belt,
            12 => Self::Greaves,
            13 => Self::Boots,
            14 => Self::Accessory1,
            15 => Self::Overcoat,
            16 => Self::OverHelm,
            17 => Self::Accessory2,
            18 => Self::Accessory3,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Weapon => 1,
            Self::Armor => 2,
            Self::Shield => 3,
            Self::Helmet => 4,
            Self::Earrings => 5,
            Self::Necklace => 6,
            Self::LeftRing => 7,
            Self::RightRing => 8,
            Self::LeftGauntlet => 9,
            Self::RightGauntlet => 10,
            Self::Belt => 11,
            Self::Greaves => 12,
            Self::Boots => 13,
            Self::Accessory1 => 14,
            Self::Overcoat => 15,
            Self::OverHelm => 16,
            Self::Accessory2 => 17,
            Self::Accessory3 => 18,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armor => "armor",
            Self::Shield => "shield",
            Self::Helmet => "helmet",
            Self::Earrings => "earrings",
            Self::Necklace => "necklace",
            Self::LeftRing => "left_ring",
            Self::RightRing => "right_ring",
            Self::LeftGauntlet => "left_gauntlet",
            Self::RightGauntlet => "right_gauntlet",
            Self::Belt => "belt",
            Self::Greaves => "greaves",
            Self::Boots => "boots",
            Self::Accessory1 => "accessory1",
            Self::Overcoat => "overcoat",
            Self::OverHelm => "over_helm",
            Self::Accessory2 => "accessory2",
            Self::Accessory3 => "accessory3",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpellTargetType {
    None,
    TextInput,
    Target,
    Unknown(u8),
}

impl SpellTargetType {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::TextInput,
            2 => Self::Target,
            5 => Self::None,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::TextInput => 1,
            Self::Target => 2,
            Self::None => 5,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spell {
    pub slot: u8,
    pub icon: u16,
    pub name: Option<String>,
    pub level: u8,
    pub max_level: u8,
    pub lines: u8,
    pub target_type: SpellTargetType,
    pub prompt: Option<String>,
    pub cooldown: CooldownStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    pub slot: u8,
    pub icon: u16,
    pub name: Option<String>,
    pub level: u8,
    pub max_level: u8,
    pub cooldown: CooldownStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectDuration {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    White,
}

impl EffectDuration {
    #[must_use]
    pub const fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Blue,
            2 => Self::Green,
            3 => Self::Yellow,
            4 => Self::Orange,
            5 => Self::Red,
            6 => Self::White,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Blue => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Orange => 4,
            Self::Red => 5,
            Self::White => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effect {
    pub icon: u16,
    /// Relative remaining-duration band. This is not an exact time value.
    pub duration: EffectDuration,
}

#[cfg(test)]
mod tests {
    use super::{EquipmentSlot, SpellTargetType};

    #[test]
    fn equipment_slots_round_trip() {
        for value in 1..=18 {
            let slot = EquipmentSlot::from_raw(value).unwrap();
            assert_eq!(slot.raw(), value);
            assert!(!slot.as_str().is_empty());
        }
        assert_eq!(EquipmentSlot::from_raw(0), None);
        assert_eq!(EquipmentSlot::from_raw(19), None);
    }

    #[test]
    fn spell_target_types_round_trip() {
        for value in 0..=u8::MAX {
            assert_eq!(SpellTargetType::from_raw(value).raw(), value);
        }
    }
}
