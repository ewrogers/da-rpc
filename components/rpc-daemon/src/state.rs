use darpc_model::{
    CharacterClass as ModelCharacterClass, CharacterSnapshot as ModelCharacterSnapshot,
    ClientLifecycle as ModelClientLifecycle, ClientSnapshot as ModelClientSnapshot,
    Element as ModelElement, Gender as ModelGender,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ObservationMetadata {
    /// Source game process identifier.
    pid: u32,
    /// Wrapping state revision assigned by the injected DLL.
    revision: u32,
    /// Wrapping sequence of the latest event incorporated into this state.
    event_sequence: u32,
    /// Wrapping Windows millisecond tick at capture completion.
    captured_tick_ms: u32,
    /// Wrapping Windows millisecond tick of the latest snapshot or event update.
    updated_tick_ms: u32,
    /// Time spent walking client state on the game thread.
    capture_duration_us: u32,
    /// Non-address generation incremented when the world root changes.
    world_generation: u32,
}

impl ObservationMetadata {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            pid,
            revision: snapshot.revision,
            event_sequence: snapshot.event_sequence,
            captured_tick_ms: snapshot.captured_tick_ms,
            updated_tick_ms: snapshot.updated_tick_ms,
            capture_duration_us: snapshot.capture_duration_us,
            world_generation: snapshot.world_generation,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct GameStatus {
    observation: ObservationMetadata,
    lifecycle: ClientLifecycle,
    character: Option<CharacterStatus>,
    map: Option<MapLocation>,
}

impl GameStatus {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            lifecycle: ClientLifecycle::from(snapshot.lifecycle),
            character: snapshot
                .character
                .as_ref()
                .map(|character| CharacterStatus::from_model(character, snapshot.group.as_ref())),
            map: snapshot.character.as_ref().and_then(|character| {
                character.location.as_ref().map(|location| MapLocation {
                    id: location.id,
                    name: location.name.clone(),
                    x: location.x,
                    y: location.y,
                    width: location.width,
                    height: location.height,
                })
            }),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Inventory {
    observation: ObservationMetadata,
    items: Option<Vec<InventoryItem>>,
}

impl Inventory {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            items: snapshot.character.as_ref().and_then(|character| {
                character
                    .inventory
                    .as_ref()
                    .map(|items| items.iter().map(InventoryItem::from).collect())
            }),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Equipment {
    observation: ObservationMetadata,
    items: Option<Vec<EquipmentItem>>,
}

impl Equipment {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            items: snapshot.character.as_ref().and_then(|character| {
                character
                    .equipment
                    .as_ref()
                    .map(|items| items.iter().map(EquipmentItem::from).collect())
            }),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Spellbook {
    observation: ObservationMetadata,
    spells: Option<Vec<Spell>>,
}

impl Spellbook {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            spells: snapshot.character.as_ref().and_then(|character| {
                character
                    .spellbook
                    .as_ref()
                    .map(|spells| spells.iter().map(Spell::from).collect())
            }),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Skillbook {
    observation: ObservationMetadata,
    skills: Option<Vec<Skill>>,
}

impl Skillbook {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            skills: snapshot.character.as_ref().and_then(|character| {
                character
                    .skillbook
                    .as_ref()
                    .map(|skills| skills.iter().map(Skill::from).collect())
            }),
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
pub(crate) struct CharacterStatus {
    id: Option<u32>,
    name: Option<String>,
    gender: Option<CharacterGender>,
    hair_style: Option<u16>,
    hair_color: Option<u8>,
    body_sprite: Option<u16>,
    class: CharacterClass,
    is_action_restricted: bool,
    is_blinded: bool,
    is_casting: bool,
    is_walking: bool,
    is_group_open: Option<bool>,
    group_members: Vec<crate::group::GroupMember>,
    gold: u32,
    weight: u32,
    max_weight: u32,
    progression: CharacterProgression,
    stats: CharacterStats,
    vitals: CharacterVitals,
    modifiers: Option<CharacterModifiers>,
}

impl CharacterStatus {
    fn from_model(value: &ModelCharacterSnapshot, group: Option<&darpc_model::GroupState>) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            gender: value
                .appearance
                .map(|appearance| CharacterGender::from(appearance.gender)),
            hair_style: value.appearance.map(|appearance| appearance.hair_style),
            hair_color: value.appearance.map(|appearance| appearance.hair_color),
            body_sprite: value.appearance.map(|appearance| appearance.body_sprite),
            class: CharacterClass::from(value.class),
            is_action_restricted: value.is_action_restricted,
            is_blinded: value.is_blinded,
            is_casting: value.is_casting,
            is_walking: value.is_walking,
            is_group_open: group.and_then(|group| group.is_group_open),
            group_members: group
                .map(|group| {
                    group
                        .members
                        .iter()
                        .map(crate::group::GroupMember::from)
                        .collect()
                })
                .unwrap_or_default(),
            gold: value.gold,
            weight: value.weight,
            max_weight: value.max_weight,
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
    defense_element: Element,
}

impl From<darpc_model::CharacterModifiers> for CharacterModifiers {
    fn from(value: darpc_model::CharacterModifiers) -> Self {
        Self {
            armor_class: value.armor_class,
            damage: value.damage,
            hit: value.hit,
            magic_resistance: value.magic_resistance,
            attack_element: Element::from(value.attack_element),
            defense_element: Element::from(value.defense_element),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
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
mod objects;

pub(crate) use collections::{
    CooldownStatus, Effect, EffectDuration, Effects, EquipmentItem, EquipmentSlot, InventoryItem,
    Skill, Spell, SpellTargetType,
};
pub(crate) use objects::{Direction, WorldObject, WorldObjectKind, WorldObjects};
