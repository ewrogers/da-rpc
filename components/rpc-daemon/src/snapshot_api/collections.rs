use darpc_model::{
    CooldownStatus as ModelCooldownStatus, EquipmentItem as ModelEquipmentItem,
    InventoryItem as ModelInventoryItem, Skill as ModelSkill, Spell as ModelSpell,
    SpellTargetType as ModelSpellTargetType,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CooldownStatus {
    active: bool,
    /// Milliseconds remaining when the client retains an expiry tick.
    remaining_ms: Option<u32>,
}

impl From<ModelCooldownStatus> for CooldownStatus {
    fn from(value: ModelCooldownStatus) -> Self {
        Self {
            active: value.active,
            remaining_ms: value.remaining_ms,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct InventoryItem {
    slot: u8,
    sprite: u16,
    dye_color: u8,
    name: Option<String>,
    quantity: u32,
    durability: u32,
    max_durability: u32,
}

impl From<&ModelInventoryItem> for InventoryItem {
    fn from(value: &ModelInventoryItem) -> Self {
        Self {
            slot: value.slot,
            sprite: value.sprite,
            dye_color: value.dye_color,
            name: value.name.clone(),
            quantity: value.quantity,
            durability: value.durability,
            max_durability: value.max_durability,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct EquipmentItem {
    slot: u8,
    sprite: u16,
    dye_color: u8,
    name: Option<String>,
    durability: u32,
    max_durability: u32,
}

impl From<&ModelEquipmentItem> for EquipmentItem {
    fn from(value: &ModelEquipmentItem) -> Self {
        Self {
            slot: value.slot,
            sprite: value.sprite,
            dye_color: value.dye_color,
            name: value.name.clone(),
            durability: value.durability,
            max_durability: value.max_durability,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Spell {
    slot: u8,
    icon: u16,
    name: Option<String>,
    level: u8,
    max_level: u8,
    lines: u8,
    target_type: SpellTargetType,
    target_type_id: u8,
    cooldown: CooldownStatus,
}

impl From<&ModelSpell> for Spell {
    fn from(value: &ModelSpell) -> Self {
        Self {
            slot: value.slot,
            icon: value.icon,
            name: value.name.clone(),
            level: value.level,
            max_level: value.max_level,
            lines: value.lines,
            target_type: SpellTargetType::from(value.target_type),
            target_type_id: value.target_type.raw(),
            cooldown: CooldownStatus::from(value.cooldown),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Skill {
    slot: u8,
    icon: u16,
    name: Option<String>,
    level: u8,
    max_level: u8,
    cooldown: CooldownStatus,
}

impl From<&ModelSkill> for Skill {
    fn from(value: &ModelSkill) -> Self {
        Self {
            slot: value.slot,
            icon: value.icon,
            name: value.name.clone(),
            level: value.level,
            max_level: value.max_level,
            cooldown: CooldownStatus::from(value.cooldown),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpellTargetType {
    None,
    TextInput,
    Target,
    Unknown,
}

impl From<ModelSpellTargetType> for SpellTargetType {
    fn from(value: ModelSpellTargetType) -> Self {
        match value {
            ModelSpellTargetType::None => Self::None,
            ModelSpellTargetType::TextInput => Self::TextInput,
            ModelSpellTargetType::Target => Self::Target,
            ModelSpellTargetType::Unknown(_) => Self::Unknown,
        }
    }
}
