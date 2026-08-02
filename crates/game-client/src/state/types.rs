use std::{error::Error, fmt};

pub(super) const MAX_MAP_NAME_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawLifecycle {
    Unknown,
    Title,
    Transition,
    InGame,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawStateSnapshot {
    /// Internal root identity used to derive a non-address generation number.
    /// This value must never leave the injected process.
    pub world_token: u32,
    pub lifecycle: RawLifecycle,
    pub character_available: bool,
    pub character: RawCharacter,
}

impl RawStateSnapshot {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            world_token: 0,
            lifecycle: RawLifecycle::Unknown,
            character_available: false,
            character: RawCharacter::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawCharacter {
    pub id: Option<u32>,
    pub name: [u8; 16],
    pub name_len: u8,
    pub appearance: Option<RawAppearance>,
    pub class: u8,
    pub is_action_restricted: bool,
    pub is_blinded: bool,
    pub gold: u32,
    pub weight: u32,
    pub max_weight: u32,
    pub level: u8,
    pub ability_level: u8,
    pub experience: u32,
    pub pane_progression: Option<RawPaneProgression>,
    pub strength: u16,
    pub intelligence: u16,
    pub wisdom: u16,
    pub constitution: u16,
    pub dexterity: u16,
    pub health: u32,
    pub max_health: u32,
    pub mana: u32,
    pub max_mana: u32,
    pub modifiers: Option<RawModifiers>,
    pub location: Option<RawLocation>,
    pub inventory_available: bool,
    pub inventory: super::RawInventory,
    pub equipment_available: bool,
    pub equipment: super::RawEquipment,
    pub spellbook_available: bool,
    pub spellbook: super::RawSpellbook,
    pub skillbook_available: bool,
    pub skillbook: super::RawSkillbook,
    pub effects: Option<super::RawEffects>,
}

impl RawCharacter {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            id: None,
            name: [0; 16],
            name_len: 0,
            appearance: None,
            class: 0,
            is_action_restricted: false,
            is_blinded: false,
            gold: 0,
            weight: 0,
            max_weight: 0,
            level: 0,
            ability_level: 0,
            experience: 0,
            pane_progression: None,
            strength: 0,
            intelligence: 0,
            wisdom: 0,
            constitution: 0,
            dexterity: 0,
            health: 0,
            max_health: 0,
            mana: 0,
            max_mana: 0,
            modifiers: None,
            location: None,
            inventory_available: false,
            inventory: super::RawInventory::empty(),
            equipment_available: false,
            equipment: super::RawEquipment::empty(),
            spellbook_available: false,
            spellbook: super::RawSpellbook::empty(),
            skillbook_available: false,
            skillbook: super::RawSkillbook::empty(),
            effects: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawAppearance {
    pub gender: u8,
    pub hair_style: u16,
    pub hair_color: u8,
    pub body_sprite: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawPaneProgression {
    pub ability_points: u32,
    pub experience_to_next_level: u32,
    pub ability_to_next_level: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawModifiers {
    pub armor_class: i8,
    pub damage: u8,
    pub hit: u8,
    pub magic_resistance_units: u16,
    pub attack_element: u16,
    pub defense_element: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawLocation {
    pub map_id: u32,
    pub name: Option<RawMapName>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawMapName {
    pub bytes: [u8; MAX_MAP_NAME_BYTES],
    pub length: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawClientText<const N: usize> {
    pub bytes: [u8; N],
    pub length: u8,
}

pub trait MemoryReader {
    /// Copies exactly `output.len()` bytes from a client virtual address.
    fn read(&self, address: u32, output: &mut [u8]) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateReadError {
    AddressOverflow,
    UnreadableMemory { address: u32, length: usize },
    WrongThread { expected: u32, actual: u32 },
    PointersChanged,
    InvalidObjectTree,
    InvalidCollection,
    InvalidPaneList,
}

impl fmt::Display for StateReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressOverflow => formatter.write_str("client address arithmetic overflowed"),
            Self::UnreadableMemory { address, length } => write!(
                formatter,
                "client memory at 0x{address:08X} is not readable for {length} bytes"
            ),
            Self::WrongThread { expected, actual } => write!(
                formatter,
                "snapshot ran on thread {actual}, expected client main thread {expected}"
            ),
            Self::PointersChanged => {
                formatter.write_str("client state pointers changed during snapshot")
            }
            Self::InvalidObjectTree => formatter.write_str("client object tree is invalid"),
            Self::InvalidCollection => formatter.write_str("client collection state is invalid"),
            Self::InvalidPaneList => formatter.write_str("client event pane list is invalid"),
        }
    }
}

impl Error for StateReadError {}
