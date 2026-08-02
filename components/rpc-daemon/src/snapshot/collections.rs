use super::ObservationMetadata;
use darpc_model::{
    ClientSnapshot as ModelClientSnapshot, CooldownStatus as ModelCooldownStatus,
    Effect as ModelEffect, EffectDuration as ModelEffectDuration,
    EquipmentItem as ModelEquipmentItem, EquipmentSlot as ModelEquipmentSlot,
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
    can_stack: bool,
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
            can_stack: value.can_stack,
            durability: value.durability,
            max_durability: value.max_durability,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct EquipmentItem {
    slot: EquipmentSlot,
    sprite: u16,
    dye_color: u8,
    name: Option<String>,
    durability: u32,
    max_durability: u32,
}

impl From<&ModelEquipmentItem> for EquipmentItem {
    fn from(value: &ModelEquipmentItem) -> Self {
        Self {
            slot: EquipmentSlot::from(value.slot),
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
    prompt: Option<String>,
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
            prompt: value.prompt.clone(),
            cooldown: CooldownStatus::from(value.cooldown),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EquipmentSlot {
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

impl From<ModelEquipmentSlot> for EquipmentSlot {
    fn from(value: ModelEquipmentSlot) -> Self {
        match value {
            ModelEquipmentSlot::Weapon => Self::Weapon,
            ModelEquipmentSlot::Armor => Self::Armor,
            ModelEquipmentSlot::Shield => Self::Shield,
            ModelEquipmentSlot::Helmet => Self::Helmet,
            ModelEquipmentSlot::Earrings => Self::Earrings,
            ModelEquipmentSlot::Necklace => Self::Necklace,
            ModelEquipmentSlot::LeftRing => Self::LeftRing,
            ModelEquipmentSlot::RightRing => Self::RightRing,
            ModelEquipmentSlot::LeftGauntlet => Self::LeftGauntlet,
            ModelEquipmentSlot::RightGauntlet => Self::RightGauntlet,
            ModelEquipmentSlot::Belt => Self::Belt,
            ModelEquipmentSlot::Greaves => Self::Greaves,
            ModelEquipmentSlot::Boots => Self::Boots,
            ModelEquipmentSlot::Accessory1 => Self::Accessory1,
            ModelEquipmentSlot::Overcoat => Self::Overcoat,
            ModelEquipmentSlot::OverHelm => Self::OverHelm,
            ModelEquipmentSlot::Accessory2 => Self::Accessory2,
            ModelEquipmentSlot::Accessory3 => Self::Accessory3,
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

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Effects {
    observation: ObservationMetadata,
    effects: Option<Vec<Effect>>,
}

impl Effects {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            effects: snapshot.character.as_ref().and_then(|character| {
                character
                    .effects
                    .as_ref()
                    .map(|effects| effects.iter().copied().map(Effect::from).collect())
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Effect {
    icon: u16,
    /// Relative remaining-duration band, not an exact time value.
    duration: EffectDuration,
}

impl From<ModelEffect> for Effect {
    fn from(value: ModelEffect) -> Self {
        Self {
            icon: value.icon,
            duration: value.duration.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EffectDuration {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    White,
}

impl From<ModelEffectDuration> for EffectDuration {
    fn from(value: ModelEffectDuration) -> Self {
        match value {
            ModelEffectDuration::Blue => Self::Blue,
            ModelEffectDuration::Green => Self::Green,
            ModelEffectDuration::Yellow => Self::Yellow,
            ModelEffectDuration::Orange => Self::Orange,
            ModelEffectDuration::Red => Self::Red,
            ModelEffectDuration::White => Self::White,
        }
    }
}
