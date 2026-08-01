use darpc_model::{
    CharacterClass as ModelCharacterClass, CharacterSnapshot as ModelCharacterSnapshot,
    ClientLifecycle as ModelClientLifecycle, ClientSnapshot as ModelClientSnapshot,
    Element as ModelElement, Gender as ModelGender,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ClientSnapshot {
    /// Source game process identifier.
    pid: u32,
    /// Wrapping state revision assigned by the injected DLL.
    revision: u32,
    /// Wrapping Windows millisecond tick at capture completion.
    captured_tick_ms: u32,
    /// Time spent walking client state on the game thread.
    capture_duration_us: u32,
    /// Non-address generation incremented when the world root changes.
    world_generation: u32,
    lifecycle: ClientLifecycle,
    character: Option<CharacterSnapshot>,
}

impl ClientSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            pid,
            revision: snapshot.revision,
            captured_tick_ms: snapshot.captured_tick_ms,
            capture_duration_us: snapshot.capture_duration_us,
            world_generation: snapshot.world_generation,
            lifecycle: ClientLifecycle::from(snapshot.lifecycle),
            character: snapshot.character.as_ref().map(CharacterSnapshot::from),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientLifecycle {
    Unknown,
    Title,
    Transition,
    InGame,
    Disconnected,
}

impl From<ModelClientLifecycle> for ClientLifecycle {
    fn from(value: ModelClientLifecycle) -> Self {
        match value {
            ModelClientLifecycle::Unknown => Self::Unknown,
            ModelClientLifecycle::Title => Self::Title,
            ModelClientLifecycle::Transition => Self::Transition,
            ModelClientLifecycle::InGame => Self::InGame,
            ModelClientLifecycle::Disconnected => Self::Disconnected,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CharacterSnapshot {
    id: Option<u32>,
    name: Option<String>,
    gender: Option<CharacterGender>,
    gender_id: Option<u8>,
    class: CharacterClass,
    class_id: u8,
    gold: u32,
    progression: CharacterProgression,
    stats: CharacterStats,
    vitals: CharacterVitals,
    modifiers: Option<CharacterModifiers>,
    location: Option<MapLocation>,
    inventory: Option<Vec<InventoryItem>>,
    equipment: Option<Vec<EquipmentItem>>,
    spellbook: Option<Vec<Spell>>,
    skillbook: Option<Vec<Skill>>,
}

impl From<&ModelCharacterSnapshot> for CharacterSnapshot {
    fn from(value: &ModelCharacterSnapshot) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            gender: value.gender.map(CharacterGender::from),
            gender_id: value.gender.map(ModelGender::raw),
            class: CharacterClass::from(value.class),
            class_id: value.class.raw(),
            gold: value.gold,
            progression: CharacterProgression {
                level: value.progression.level,
                ability_level: value.progression.ability_level,
                experience: value.progression.experience,
                ability_points: value.progression.ability_points,
                experience_to_next_level: value.progression.experience_to_next_level,
                ability_to_next_level: value.progression.ability_to_next_level,
            },
            stats: CharacterStats {
                strength: value.stats.strength,
                intelligence: value.stats.intelligence,
                wisdom: value.stats.wisdom,
                constitution: value.stats.constitution,
                dexterity: value.stats.dexterity,
            },
            vitals: CharacterVitals {
                health: value.vitals.health,
                max_health: value.vitals.max_health,
                mana: value.vitals.mana,
                max_mana: value.vitals.max_mana,
            },
            modifiers: value.modifiers.map(CharacterModifiers::from),
            location: value.location.as_ref().map(|location| MapLocation {
                id: location.id,
                name: location.name.clone(),
                x: location.x,
                y: location.y,
                width: location.width,
                height: location.height,
            }),
            inventory: value
                .inventory
                .as_ref()
                .map(|items| items.iter().map(InventoryItem::from).collect()),
            equipment: value
                .equipment
                .as_ref()
                .map(|items| items.iter().map(EquipmentItem::from).collect()),
            spellbook: value
                .spellbook
                .as_ref()
                .map(|spells| spells.iter().map(Spell::from).collect()),
            skillbook: value
                .skillbook
                .as_ref()
                .map(|skills| skills.iter().map(Skill::from).collect()),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CharacterGender {
    Male,
    Female,
    Unknown,
}

impl From<ModelGender> for CharacterGender {
    fn from(value: ModelGender) -> Self {
        match value {
            ModelGender::Male => Self::Male,
            ModelGender::Female => Self::Female,
            ModelGender::Unknown(_) => Self::Unknown,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CharacterClass {
    Peasant,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Unknown,
}

impl From<ModelCharacterClass> for CharacterClass {
    fn from(value: ModelCharacterClass) -> Self {
        match value {
            ModelCharacterClass::Peasant => Self::Peasant,
            ModelCharacterClass::Warrior => Self::Warrior,
            ModelCharacterClass::Rogue => Self::Rogue,
            ModelCharacterClass::Wizard => Self::Wizard,
            ModelCharacterClass::Priest => Self::Priest,
            ModelCharacterClass::Monk => Self::Monk,
            ModelCharacterClass::Unknown(_) => Self::Unknown,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CharacterProgression {
    level: u8,
    ability_level: u8,
    experience: u32,
    ability_points: Option<u32>,
    experience_to_next_level: Option<u32>,
    ability_to_next_level: Option<u32>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CharacterStats {
    strength: u16,
    intelligence: u16,
    wisdom: u16,
    constitution: u16,
    dexterity: u16,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CharacterVitals {
    health: u32,
    max_health: u32,
    mana: u32,
    max_mana: u32,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CharacterModifiers {
    armor_class: i8,
    damage: u8,
    hit: u8,
    /// Display percentage, not the client's internal ten-percent unit count.
    magic_resistance: u16,
    attack_element: Element,
    attack_element_id: u16,
    defense_element: Element,
    defense_element_id: u16,
}

impl From<darpc_model::CharacterModifiers> for CharacterModifiers {
    fn from(value: darpc_model::CharacterModifiers) -> Self {
        Self {
            armor_class: value.armor_class,
            damage: value.damage,
            hit: value.hit,
            magic_resistance: value.magic_resistance,
            attack_element: Element::from(value.attack_element),
            attack_element_id: value.attack_element.raw(),
            defense_element: Element::from(value.defense_element),
            defense_element_id: value.defense_element.raw(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Element {
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
    Unknown,
}

impl From<ModelElement> for Element {
    fn from(value: ModelElement) -> Self {
        match value {
            ModelElement::None => Self::None,
            ModelElement::Fire => Self::Fire,
            ModelElement::Water => Self::Water,
            ModelElement::Wind => Self::Wind,
            ModelElement::Earth => Self::Earth,
            ModelElement::Light => Self::Light,
            ModelElement::Dark => Self::Dark,
            ModelElement::Wood => Self::Wood,
            ModelElement::Metal => Self::Metal,
            ModelElement::Undead => Self::Undead,
            ModelElement::Unknown(_) => Self::Unknown,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct MapLocation {
    id: u32,
    /// Read from the map pane or the latest accepted map-size event.
    name: Option<String>,
    x: Option<i32>,
    y: Option<i32>,
    width: i32,
    height: i32,
}
mod collections;

pub(crate) use collections::{
    CooldownStatus, EquipmentItem, InventoryItem, Skill, Spell, SpellTargetType,
};
