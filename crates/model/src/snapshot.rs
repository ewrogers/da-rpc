#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientLifecycle {
    Unknown,
    Title,
    Transition,
    InGame,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gender {
    Male,
    Female,
    Unknown(u8),
}

impl Gender {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Male,
            1 => Self::Female,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Male => 0,
            Self::Female => 1,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterClass {
    Peasant,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Unknown(u8),
}

impl CharacterClass {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Peasant,
            1 => Self::Warrior,
            2 => Self::Rogue,
            3 => Self::Wizard,
            4 => Self::Priest,
            5 => Self::Monk,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Peasant => 0,
            Self::Warrior => 1,
            Self::Rogue => 2,
            Self::Wizard => 3,
            Self::Priest => 4,
            Self::Monk => 5,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Element {
    None,
    Fire,
    Water,
    Wind,
    Earth,
    Light,
    Dark,
    Wood,
    Metal,
    Undead,
    Unknown(u16),
}

impl Element {
    #[must_use]
    pub const fn from_raw(value: u16) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Fire,
            2 => Self::Water,
            3 => Self::Wind,
            4 => Self::Earth,
            5 => Self::Light,
            6 => Self::Dark,
            7 => Self::Wood,
            8 => Self::Metal,
            9 => Self::Undead,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Fire => 1,
            Self::Water => 2,
            Self::Wind => 3,
            Self::Earth => 4,
            Self::Light => 5,
            Self::Dark => 6,
            Self::Wood => 7,
            Self::Metal => 8,
            Self::Undead => 9,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientSnapshot {
    pub revision: u32,
    pub event_sequence: u32,
    pub captured_tick_ms: u32,
    pub updated_tick_ms: u32,
    pub capture_duration_us: u32,
    pub world_generation: u32,
    pub lifecycle: ClientLifecycle,
    pub character: Option<CharacterSnapshot>,
    pub objects: Option<Vec<crate::WorldObject>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterSnapshot {
    pub id: Option<u32>,
    pub name: Option<String>,
    pub appearance: Option<CharacterAppearance>,
    pub class: CharacterClass,
    pub is_action_restricted: bool,
    pub is_blinded: bool,
    pub gold: u32,
    pub weight: u32,
    pub max_weight: u32,
    pub progression: CharacterProgression,
    pub stats: CharacterStats,
    pub vitals: CharacterVitals,
    pub modifiers: Option<CharacterModifiers>,
    pub location: Option<MapLocation>,
    pub inventory: Option<Vec<InventoryItem>>,
    pub equipment: Option<Vec<EquipmentItem>>,
    pub spellbook: Option<Vec<Spell>>,
    pub skillbook: Option<Vec<Skill>>,
    pub effects: Option<Vec<Effect>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterAppearance {
    pub gender: Gender,
    pub hair_style: u16,
    pub hair_color: u8,
    pub body_sprite: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterProgression {
    pub level: u8,
    pub ability_level: u8,
    pub experience: u32,
    pub ability_points: Option<u32>,
    pub experience_to_next_level: Option<u32>,
    pub ability_to_next_level: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterStats {
    pub strength: u16,
    pub intelligence: u16,
    pub wisdom: u16,
    pub constitution: u16,
    pub dexterity: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterVitals {
    pub health: u32,
    pub max_health: u32,
    pub mana: u32,
    pub max_mana: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterModifiers {
    pub armor_class: i8,
    pub damage: u8,
    pub hit: u8,
    /// Display percentage. The client stores this value in ten-percent units.
    pub magic_resistance: u16,
    pub attack_element: Element,
    pub defense_element: Element,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapLocation {
    pub id: u32,
    pub name: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: i32,
    pub height: i32,
}

#[cfg(test)]
mod tests {
    use super::{CharacterClass, Element, Gender};

    #[test]
    fn known_and_unknown_domain_values_round_trip() {
        for value in 0..=u8::MAX {
            assert_eq!(CharacterClass::from_raw(value).raw(), value);
            assert_eq!(Gender::from_raw(value).raw(), value);
        }
        for value in 0..=u16::MAX {
            assert_eq!(Element::from_raw(value).raw(), value);
        }
    }
}
mod collections;

pub use collections::{
    CooldownStatus, Effect, EffectDuration, EquipmentItem, EquipmentSlot, InventoryItem, Skill,
    Spell, SpellTargetType,
};
