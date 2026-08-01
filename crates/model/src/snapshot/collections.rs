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
    pub durability: u32,
    pub max_durability: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquipmentItem {
    pub slot: u8,
    pub sprite: u16,
    pub dye_color: u8,
    pub name: Option<String>,
    pub durability: u32,
    pub max_durability: u32,
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

#[cfg(test)]
mod tests {
    use super::SpellTargetType;

    #[test]
    fn spell_target_types_round_trip() {
        for value in 0..=u8::MAX {
            assert_eq!(SpellTargetType::from_raw(value).raw(), value);
        }
    }
}
