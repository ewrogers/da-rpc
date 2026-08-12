mod expand;

use super::*;

pub(super) use expand::expand;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ClientCommand {
    pub(super) observation: EventObservation,
    pub(super) command: String,
    pub(super) args: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ClientLifecycleChanged {
    pub(super) observation: EventObservation,
    pub(super) previous: crate::state::ClientLifecycle,
    pub(super) current: crate::state::ClientLifecycle,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct SoundPlayed {
    pub(super) observation: EventObservation,
    pub(super) effect: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MusicStarted {
    pub(super) observation: EventObservation,
    pub(super) track: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MusicStopped {
    pub(super) observation: EventObservation,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LegendMarkAdded {
    pub(super) observation: EventObservation,
    pub(super) mark: LegendMark,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LegendMarkChanged {
    pub(super) observation: EventObservation,
    pub(super) previous: LegendMark,
    pub(super) current: LegendMark,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LegendMarkRemoved {
    pub(super) observation: EventObservation,
    pub(super) mark: LegendMark,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A spell-effect icon became active.
pub(crate) struct EffectAdded {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    pub(super) icon: u16,
    /// Relative remaining-duration color band, not an exact time.
    pub(super) duration: EffectDuration,
}

impl EffectAdded {
    fn new(observation: EventObservation, effect: Effect) -> Self {
        Self {
            observation,
            icon: effect.icon,
            duration: effect.duration.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A spell-effect icon is no longer active.
pub(crate) struct EffectRemoved {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    pub(super) icon: u16,
}

impl EffectRemoved {
    const fn new(observation: EventObservation, icon: u16) -> Self {
        Self { observation, icon }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// The relative remaining-duration band changed for an active icon.
pub(crate) struct EffectChanged {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    pub(super) icon: u16,
    /// New relative remaining-duration color band, not an exact time.
    pub(super) duration: EffectDuration,
}

impl EffectChanged {
    fn new(observation: EventObservation, effect: Effect) -> Self {
        Self {
            observation,
            icon: effect.icon,
            duration: effect.duration.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One occupied inventory slot changed as part of an atomic collection batch.
pub(crate) struct InventorySlotChanged {
    pub(super) observation: EventObservation,
    /// Zero-based position within this collection batch.
    pub(super) batch_index: u16,
    /// Total number of slot changes in this collection batch.
    pub(super) batch_count: u16,
    pub(super) slot: u8,
    pub(super) before: Option<InventoryItem>,
    pub(super) after: Option<InventoryItem>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One occupied spellbook slot changed as part of an atomic collection batch.
pub(crate) struct SpellSlotChanged {
    pub(super) observation: EventObservation,
    /// Zero-based position within this collection batch.
    pub(super) batch_index: u16,
    /// Total number of slot changes in this collection batch.
    pub(super) batch_count: u16,
    pub(super) slot: u8,
    pub(super) before: Option<Spell>,
    pub(super) after: Option<Spell>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One occupied skillbook slot changed as part of an atomic collection batch.
pub(crate) struct SkillSlotChanged {
    pub(super) observation: EventObservation,
    /// Zero-based position within this collection batch.
    pub(super) batch_index: u16,
    /// Total number of slot changes in this collection batch.
    pub(super) batch_count: u16,
    pub(super) slot: u8,
    pub(super) before: Option<Skill>,
    pub(super) after: Option<Skill>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StatsChanged {
    pub(super) observation: EventObservation,
    pub(super) strength: u16,
    pub(super) intelligence: u16,
    pub(super) wisdom: u16,
    pub(super) constitution: u16,
    pub(super) dexterity: u16,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct VitalsChanged {
    pub(super) observation: EventObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) health: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_health: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) mana: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_mana: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ProgressionChanged {
    pub(super) observation: EventObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ability_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) experience: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ability_points: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) experience_to_next_level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ability_to_next_level: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GoldChanged {
    pub(super) observation: EventObservation,
    pub(super) gold: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WeightChanged {
    pub(super) observation: EventObservation,
    pub(super) weight: u32,
    pub(super) max_weight: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ModifiersChanged {
    pub(super) observation: EventObservation,
    pub(super) armor_class: i8,
    pub(super) damage: u8,
    pub(super) hit: u8,
    pub(super) magic_resistance: u16,
    pub(super) attack_element: Element,
    pub(super) defense_element: Element,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LocationChanged {
    pub(super) observation: EventObservation,
    pub(super) x: i32,
    pub(super) y: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) map: Option<MapChanged>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MapChanged {
    pub(super) id: u32,
    pub(super) name: Option<String>,
    pub(super) width: i32,
    pub(super) height: i32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BlindChanged {
    pub(super) observation: EventObservation,
    pub(super) is_blinded: bool,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct TilePosition {
    pub(super) x: i32,
    pub(super) y: i32,
}

impl From<ModelTilePosition> for TilePosition {
    fn from(value: ModelTilePosition) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WalkingStarted {
    pub(super) observation: EventObservation,
    pub(super) current: TilePosition,
    pub(super) destination: Option<TilePosition>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WalkingStopped {
    pub(super) observation: EventObservation,
    pub(super) current: TilePosition,
    pub(super) destination: Option<TilePosition>,
    pub(super) reached_destination: Option<bool>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WalkingRouteChanged {
    pub(super) observation: EventObservation,
    pub(super) generation: u32,
    pub(super) tiles: Vec<TilePosition>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ActionRestrictionChanged {
    pub(super) observation: EventObservation,
    pub(super) is_action_restricted: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct CharacterProfileChanged {
    pub(super) observation: EventObservation,
    previous: Option<crate::state::PlayerIdentity>,
    current: crate::state::PlayerIdentity,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ObjectChanged {
    pub(super) observation: EventObservation,
    pub(super) object: WorldObject,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ObjectsCleared {
    pub(super) observation: EventObservation,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StreamResyncRequired {
    pub(super) pid: u32,
    pub(super) instance_id: String,
    pub(super) last_event_sequence: u32,
    pub(super) dropped_events: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StreamClosed {
    pub(super) pid: u32,
    pub(super) instance_id: String,
    pub(super) last_event_sequence: u32,
    pub(super) reason: String,
}
