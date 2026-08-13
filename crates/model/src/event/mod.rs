use crate::{
    CharacterModifiers, CharacterProfileUpdate, CharacterStats, ClientLifecycle, ClientMessage,
    ClientSnapshot, DialogUpdate, Direction, Effect, EntityUpdate, EquipmentSlot, ExchangeUpdate,
    GroupUpdate, InventoryItem, LegendUpdate, MapExclusions, MapLocation, ObjectUpdate,
    PlayerUpdate, SequenceNumber, Skill, Spell,
};
use std::{error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateEvent {
    pub sequence: u32,
    pub revision: u32,
    pub tick_ms: u32,
    pub update: StateUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateUpdate {
    Lifecycle(LifecycleUpdate),
    Audio(AudioUpdate),
    Command(ClientCommand),
    Status(StatusUpdate),
    Movement(MovementUpdate),
    PlannedRoute(PlannedRoute),
    MapExclusions(MapExclusionsUpdate),
    Location(LocationUpdate),
    Effect(EffectUpdate),
    Object(ObjectUpdate),
    Player(PlayerUpdate),
    CharacterProfile(CharacterProfileUpdate),
    Message(ClientMessage),
    Inventory(InventoryUpdate),
    Spellbook(SpellbookUpdate),
    Skillbook(SkillbookUpdate),
    Ability(AbilityUpdate),
    Action(ActionUpdate),
    Entity(EntityUpdate),
    Dialog(DialogUpdate),
    Group(GroupUpdate),
    Exchange(ExchangeUpdate),
    Legend(LegendUpdate),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MapExclusionsUpdate {
    Replaced {
        exclusions: MapExclusions,
        map_count: u16,
    },
    Removed {
        map_id: u32,
        map_count: u16,
    },
    Cleared {
        removed_map_count: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientCommand {
    pub command: String,
    pub args: Vec<String>,
}

impl ClientCommand {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (command, args) = value
            .split_once(char::is_whitespace)
            .map_or((value, ""), |(command, args)| (command, args.trim()));
        if command.is_empty() {
            return None;
        }
        Some(Self {
            command: command.to_owned(),
            args: args
                .split(',')
                .map(str::trim)
                .filter(|arg| !arg.is_empty())
                .map(str::to_owned)
                .collect(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleUpdate {
    pub previous: ClientLifecycle,
    pub current: ClientLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioUpdate {
    SoundPlayed { effect: u8 },
    MusicStarted { track: u8 },
    MusicStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionUpdate {
    ItemUsed {
        slot: u8,
    },
    ItemDropped {
        slot: u8,
        quantity: u32,
        position: TilePosition,
    },
    ItemGiven {
        slot: u8,
        quantity: u32,
        object_id: u32,
    },
    GoldDropped {
        amount: u32,
        position: TilePosition,
    },
    GoldGiven {
        amount: u32,
        object_id: u32,
    },
    ItemPickedUp {
        destination_slot: u8,
        position: TilePosition,
    },
    EquipmentUnequipped {
        slot: EquipmentSlot,
    },
    Emoted {
        code: u8,
    },
    Turned {
        direction: Direction,
    },
    Resync,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AbilityUpdate {
    SkillUsed {
        slot: u8,
    },
    SpellBegin {
        slot: u8,
        total_lines: u8,
    },
    SpellChant {
        slot: u8,
        line: u8,
        total_lines: u8,
    },
    SpellCast {
        slot: u8,
        arguments: SpellCastArguments,
    },
    SpellCancelled {
        slot: u8,
        source: SpellCancellationSource,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpellCastArguments {
    Unknown,
    None,
    Target { id: Option<u32>, x: i32, y: i32 },
    Input(String),
    Values(Vec<u16>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpellCancellationSource {
    Client,
    Server,
    Replaced,
}

pub type InventoryUpdate = SlotUpdate<InventoryItem>;
pub type SpellbookUpdate = SlotUpdate<Spell>;
pub type SkillbookUpdate = SlotUpdate<Skill>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotUpdate<T> {
    pub batch_index: u8,
    pub batch_count: u8,
    pub change: CollectionChange,
    pub slot: u8,
    pub before: Option<T>,
    pub after: Option<T>,
}

impl<T> SlotUpdate<T> {
    #[must_use]
    pub const fn batch(&self) -> CollectionBatch {
        CollectionBatch {
            index: self.batch_index,
            count: self.batch_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionBatch {
    pub index: u8,
    pub count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionChange {
    Added,
    Removed,
    Changed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    Inventory,
    Spellbook,
    Skillbook,
}

impl CollectionKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inventory => "inventory",
            Self::Spellbook => "spellbook",
            Self::Skillbook => "skillbook",
        }
    }
}

impl StateUpdate {
    #[must_use]
    pub const fn collection_batch(&self) -> Option<(CollectionKind, CollectionBatch)> {
        match self {
            Self::Inventory(update) => Some((CollectionKind::Inventory, update.batch())),
            Self::Spellbook(update) => Some((CollectionKind::Spellbook, update.batch())),
            Self::Skillbook(update) => Some((CollectionKind::Skillbook, update.batch())),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectUpdate {
    Added(Effect),
    Removed { icon: u16 },
    Changed(Effect),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocationUpdate {
    pub x: i32,
    pub y: i32,
    /// Present only when this position completes a staged map transition.
    pub map: Option<MapChange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovementUpdate {
    Started {
        current: TilePosition,
        destination: Option<TilePosition>,
    },
    Stopped {
        current: TilePosition,
        destination: Option<TilePosition>,
        reached_destination: Option<bool>,
    },
    Obstructed {
        map_id: u32,
        current: TilePosition,
        attempted: TilePosition,
        direction: Direction,
        destination: Option<TilePosition>,
        mode: WalkMode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkMode {
    Direct,
    NativeRoute,
    ExactRoute,
    Pursuit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedRoute {
    pub generation: u32,
    pub tiles: Vec<TilePosition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapChange {
    pub id: u32,
    pub name: Option<String>,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusUpdate {
    pub core: Option<CoreStatus>,
    pub vitals: Option<CurrentVitals>,
    pub progression: Option<ProgressionStatus>,
    pub gold: Option<u32>,
    pub modifiers: Option<CharacterModifiers>,
    pub is_blinded: Option<bool>,
    pub is_action_restricted: Option<bool>,
    pub is_casting: Option<bool>,
}

impl StatusUpdate {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.core.is_none()
            && self.vitals.is_none()
            && self.progression.is_none()
            && self.gold.is_none()
            && self.modifiers.is_none()
            && self.is_blinded.is_none()
            && self.is_action_restricted.is_none()
            && self.is_casting.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreStatus {
    pub level: u8,
    pub ability_level: u8,
    pub max_health: u32,
    pub max_mana: u32,
    pub weight: u32,
    pub max_weight: u32,
    pub stats: CharacterStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVitals {
    pub health: u32,
    pub mana: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgressionStatus {
    pub experience: u32,
    pub ability_points: u32,
    pub experience_to_next_level: u32,
    pub ability_to_next_level: u32,
}

impl ClientSnapshot {
    pub fn apply_event(&mut self, event: StateEvent) -> Result<(), ApplyEventError> {
        let expected_sequence = SequenceNumber::new(self.event_sequence).next().get();
        if event.sequence != expected_sequence {
            return Err(ApplyEventError::UnexpectedSequence {
                expected: expected_sequence,
                actual: event.sequence,
            });
        }
        let expected_revision = SequenceNumber::new(self.revision).next().get();
        if event.revision != expected_revision {
            return Err(ApplyEventError::UnexpectedRevision {
                expected: expected_revision,
                actual: event.revision,
            });
        }
        match event.update {
            StateUpdate::Lifecycle(update) => {
                if self.lifecycle != update.previous {
                    return Err(ApplyEventError::UnexpectedLifecycle {
                        expected: self.lifecycle,
                        actual: update.previous,
                    });
                }
                self.lifecycle = update.current;
            }
            StateUpdate::Audio(_) => {}
            StateUpdate::Status(update) => {
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                if let Some(core) = update.core {
                    character.progression.level = core.level;
                    character.progression.ability_level = core.ability_level;
                    character.vitals.max_health = core.max_health;
                    character.vitals.max_mana = core.max_mana;
                    character.weight = core.weight;
                    character.max_weight = core.max_weight;
                    character.stats = core.stats;
                }
                if let Some(vitals) = update.vitals {
                    character.vitals.health = vitals.health;
                    character.vitals.mana = vitals.mana;
                }
                if let Some(progression) = update.progression {
                    character.progression.experience = progression.experience;
                    character.progression.ability_points = Some(progression.ability_points);
                    character.progression.experience_to_next_level =
                        Some(progression.experience_to_next_level);
                    character.progression.ability_to_next_level =
                        Some(progression.ability_to_next_level);
                }
                if let Some(gold) = update.gold {
                    character.gold = gold;
                }
                if let Some(modifiers) = update.modifiers {
                    character.modifiers = Some(modifiers);
                }
                if let Some(is_blinded) = update.is_blinded {
                    character.is_blinded = is_blinded;
                }
                if let Some(is_action_restricted) = update.is_action_restricted {
                    character.is_action_restricted = is_action_restricted;
                }
                if let Some(is_casting) = update.is_casting {
                    character.is_casting = is_casting;
                }
            }
            StateUpdate::Movement(update) => {
                if matches!(update, MovementUpdate::Obstructed { .. }) {
                    return Ok(());
                }
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                let (current, is_walking) = match update {
                    MovementUpdate::Started { current, .. } => (current, true),
                    MovementUpdate::Stopped { current, .. } => (current, false),
                    MovementUpdate::Obstructed { .. } => unreachable!(),
                };
                character.is_walking = is_walking;
                let location = character
                    .location
                    .as_mut()
                    .ok_or(ApplyEventError::LocationUnavailable)?;
                location.x = Some(current.x);
                location.y = Some(current.y);
            }
            StateUpdate::PlannedRoute(route) => self.planned_route = Some(route),
            StateUpdate::MapExclusions(update) => match update {
                MapExclusionsUpdate::Replaced {
                    exclusions,
                    map_count,
                } => {
                    let result = self
                        .map_exclusions
                        .binary_search_by_key(&exclusions.map_id, |entry| entry.map_id);
                    let expected_count = self.map_exclusions.len() + usize::from(result.is_err());
                    if usize::from(map_count) != expected_count {
                        return Err(ApplyEventError::MapExclusionCountMismatch {
                            expected: expected_count,
                            actual: usize::from(map_count),
                        });
                    }
                    match result {
                        Ok(index) => self.map_exclusions[index] = exclusions,
                        Err(index) => self.map_exclusions.insert(index, exclusions),
                    }
                }
                MapExclusionsUpdate::Removed { map_id, map_count } => {
                    let index = self
                        .map_exclusions
                        .binary_search_by_key(&map_id, |entry| entry.map_id)
                        .map_err(|_| ApplyEventError::MapExclusionsNotFound { map_id })?;
                    let expected_count = self.map_exclusions.len() - 1;
                    if usize::from(map_count) != expected_count {
                        return Err(ApplyEventError::MapExclusionCountMismatch {
                            expected: expected_count,
                            actual: usize::from(map_count),
                        });
                    }
                    self.map_exclusions.remove(index);
                }
                MapExclusionsUpdate::Cleared { removed_map_count } => {
                    let expected_count = self.map_exclusions.len();
                    if usize::from(removed_map_count) != expected_count {
                        return Err(ApplyEventError::MapExclusionCountMismatch {
                            expected: expected_count,
                            actual: usize::from(removed_map_count),
                        });
                    }
                    self.map_exclusions.clear();
                }
            },
            StateUpdate::Location(update) => {
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                if let Some(map) = update.map {
                    character.is_walking = false;
                    if let Some(route) = &mut self.planned_route {
                        route.tiles.clear();
                    }
                    character.location = Some(MapLocation {
                        id: map.id,
                        name: map.name,
                        x: Some(update.x),
                        y: Some(update.y),
                        width: map.width,
                        height: map.height,
                    });
                    self.objects = Some(Vec::new());
                } else {
                    let location = character
                        .location
                        .as_mut()
                        .ok_or(ApplyEventError::LocationUnavailable)?;
                    location.x = Some(update.x);
                    location.y = Some(update.y);
                }
            }
            StateUpdate::Effect(update) => {
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                let effects = character
                    .effects
                    .as_mut()
                    .ok_or(ApplyEventError::EffectsUnavailable)?;
                match update {
                    EffectUpdate::Added(effect) => {
                        if effects.iter().any(|current| current.icon == effect.icon) {
                            return Err(ApplyEventError::EffectAlreadyExists { icon: effect.icon });
                        }
                        if effects.len() >= 10 {
                            return Err(ApplyEventError::EffectCapacityExceeded);
                        }
                        effects.push(effect);
                        effects.sort_unstable_by_key(|current| current.icon);
                    }
                    EffectUpdate::Removed { icon } => {
                        let index = effects
                            .iter()
                            .position(|effect| effect.icon == icon)
                            .ok_or(ApplyEventError::EffectNotFound { icon })?;
                        effects.remove(index);
                    }
                    EffectUpdate::Changed(effect) => {
                        let current = effects
                            .iter_mut()
                            .find(|current| current.icon == effect.icon)
                            .ok_or(ApplyEventError::EffectNotFound { icon: effect.icon })?;
                        *current = effect;
                    }
                }
            }
            StateUpdate::Object(update) => {
                let objects = self
                    .objects
                    .as_mut()
                    .ok_or(ApplyEventError::ObjectsUnavailable)?;
                match update {
                    ObjectUpdate::Appeared(object) => {
                        if let Some(current) = objects
                            .iter_mut()
                            .find(|current| current.id() == object.id())
                        {
                            *current = object;
                        } else {
                            objects.push(object);
                        }
                    }
                    ObjectUpdate::Disappeared(object) => {
                        let index = objects
                            .iter()
                            .position(|current| current.id() == object.id())
                            .ok_or(ApplyEventError::ObjectNotFound { id: object.id() })?;
                        objects.remove(index);
                    }
                    ObjectUpdate::Moved(object) | ObjectUpdate::DirectionChanged(object) => {
                        let id = object.id();
                        let current = objects
                            .iter_mut()
                            .find(|current| current.id() == id)
                            .ok_or(ApplyEventError::ObjectNotFound { id })?;
                        *current = object;
                    }
                    ObjectUpdate::Cleared => objects.clear(),
                }
                objects.sort_unstable_by_key(WorldObjectSortKey::of);
            }
            StateUpdate::Player(update) => {
                let objects = self
                    .objects
                    .as_mut()
                    .ok_or(ApplyEventError::ObjectsUnavailable)?;
                let id = update.player.id();
                let next_profile = match update.player {
                    crate::WorldObject::Player {
                        profile: Some(profile),
                        ..
                    } => *profile,
                    _ => return Err(ApplyEventError::ObjectNotFound { id }),
                };
                let player = objects
                    .iter_mut()
                    .find(|object| object.id() == id)
                    .ok_or(ApplyEventError::ObjectNotFound { id })?;
                match player {
                    crate::WorldObject::Player { profile, .. } => {
                        *profile = Some(Box::new(next_profile));
                    }
                    _ => return Err(ApplyEventError::ObjectNotFound { id }),
                }
            }
            StateUpdate::CharacterProfile(update) => {
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                character.identity = Some(update.current);
            }
            StateUpdate::Command(_) | StateUpdate::Message(_) => {}
            StateUpdate::Inventory(update) => {
                let inventory = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?
                    .inventory
                    .as_mut()
                    .ok_or(ApplyEventError::CollectionUnavailable {
                        collection: CollectionKind::Inventory,
                    })?;
                apply_slot_update(inventory, update, CollectionKind::Inventory)?;
            }
            StateUpdate::Spellbook(update) => {
                let spellbook = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?
                    .spellbook
                    .as_mut()
                    .ok_or(ApplyEventError::CollectionUnavailable {
                        collection: CollectionKind::Spellbook,
                    })?;
                apply_slot_update(spellbook, update, CollectionKind::Spellbook)?;
            }
            StateUpdate::Skillbook(update) => {
                let skillbook = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?
                    .skillbook
                    .as_mut()
                    .ok_or(ApplyEventError::CollectionUnavailable {
                        collection: CollectionKind::Skillbook,
                    })?;
                apply_slot_update(skillbook, update, CollectionKind::Skillbook)?;
            }
            StateUpdate::Ability(update) => {
                let character = self
                    .character
                    .as_mut()
                    .ok_or(ApplyEventError::CharacterUnavailable)?;
                match update {
                    AbilityUpdate::SpellBegin { .. } => character.is_casting = true,
                    AbilityUpdate::SpellCast { .. } | AbilityUpdate::SpellCancelled { .. } => {
                        character.is_casting = false;
                    }
                    AbilityUpdate::SkillUsed { .. } | AbilityUpdate::SpellChant { .. } => {}
                }
            }
            StateUpdate::Action(_) => {}
            StateUpdate::Entity(_) => {}
            StateUpdate::Dialog(update) => match update {
                DialogUpdate::Opened(state)
                | DialogUpdate::Changed(state)
                | DialogUpdate::Submitted { state, .. } => self.dialog = Some(state),
                DialogUpdate::Closed { .. } => self.dialog = None,
            },
            StateUpdate::Group(update) => {
                if let Some(state) = update.state() {
                    self.group = Some(state.clone());
                }
            }
            StateUpdate::Exchange(update) => match update {
                ExchangeUpdate::Completed { .. } | ExchangeUpdate::Cancelled { .. } => {
                    self.exchange = None;
                }
                update => self.exchange = Some(update.state().clone()),
            },
            StateUpdate::Legend(update) => {
                let legend = self.legend.get_or_insert_with(Vec::new);
                match update {
                    LegendUpdate::MarkAdded { mark } => legend.push(mark),
                    LegendUpdate::MarkChanged { previous, current } => {
                        let mark = legend
                            .iter_mut()
                            .find(|mark| **mark == previous)
                            .ok_or(ApplyEventError::LegendMarkNotFound)?;
                        *mark = current;
                    }
                    LegendUpdate::MarkRemoved { mark } => {
                        let index = legend
                            .iter()
                            .position(|current| *current == mark)
                            .ok_or(ApplyEventError::LegendMarkNotFound)?;
                        legend.remove(index);
                    }
                }
            }
        }
        self.revision = event.revision;
        self.event_sequence = event.sequence;
        self.updated_tick_ms = event.tick_ms;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyEventError {
    UnexpectedSequence {
        expected: u32,
        actual: u32,
    },
    UnexpectedRevision {
        expected: u32,
        actual: u32,
    },
    UnexpectedLifecycle {
        expected: ClientLifecycle,
        actual: ClientLifecycle,
    },
    CharacterUnavailable,
    LocationUnavailable,
    EffectsUnavailable,
    EffectAlreadyExists {
        icon: u16,
    },
    EffectNotFound {
        icon: u16,
    },
    EffectCapacityExceeded,
    ObjectsUnavailable,
    ObjectNotFound {
        id: u32,
    },
    InvalidCollectionBatch {
        collection: CollectionKind,
        index: u8,
        count: u8,
    },
    InvalidCollectionSlot {
        collection: CollectionKind,
        slot: u8,
    },
    CollectionUnavailable {
        collection: CollectionKind,
    },
    CollectionSlotMismatch {
        collection: CollectionKind,
        slot: u8,
    },
    MapExclusionsNotFound {
        map_id: u32,
    },
    MapExclusionCountMismatch {
        expected: usize,
        actual: usize,
    },
    LegendMarkNotFound,
}

impl fmt::Display for ApplyEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedSequence { expected, actual } => {
                write!(
                    formatter,
                    "expected event sequence {expected}, received {actual}"
                )
            }
            Self::UnexpectedRevision { expected, actual } => {
                write!(
                    formatter,
                    "expected state revision {expected}, received {actual}"
                )
            }
            Self::UnexpectedLifecycle { expected, actual } => write!(
                formatter,
                "expected lifecycle {expected:?}, received previous lifecycle {actual:?}"
            ),
            Self::CharacterUnavailable => {
                formatter.write_str("state event has no retained character state")
            }
            Self::LocationUnavailable => {
                formatter.write_str("location event has no retained map state")
            }
            Self::EffectsUnavailable => {
                formatter.write_str("effect event has no retained spell effect state")
            }
            Self::EffectAlreadyExists { icon } => {
                write!(formatter, "spell effect icon {icon} already exists")
            }
            Self::EffectNotFound { icon } => {
                write!(formatter, "spell effect icon {icon} does not exist")
            }
            Self::EffectCapacityExceeded => {
                formatter.write_str("spell effect capacity was exceeded")
            }
            Self::ObjectsUnavailable => {
                formatter.write_str("object event has no retained world object state")
            }
            Self::ObjectNotFound { id } => write!(formatter, "world object {id} does not exist"),
            Self::InvalidCollectionBatch {
                collection,
                index,
                count,
            } => write!(
                formatter,
                "invalid {} batch position {index} of {count}",
                collection.as_str()
            ),
            Self::InvalidCollectionSlot { collection, slot } => {
                write!(formatter, "invalid {} slot {slot}", collection.as_str())
            }
            Self::CollectionUnavailable { collection } => write!(
                formatter,
                "{} event has no retained collection state",
                collection.as_str()
            ),
            Self::CollectionSlotMismatch { collection, slot } => write!(
                formatter,
                "{} slot {slot} did not match the event baseline",
                collection.as_str()
            ),
            Self::MapExclusionsNotFound { map_id } => {
                write!(formatter, "path exclusions for map {map_id} do not exist")
            }
            Self::MapExclusionCountMismatch { expected, actual } => write!(
                formatter,
                "expected {expected} path-exclusion maps after update, received {actual}"
            ),
            Self::LegendMarkNotFound => {
                formatter.write_str("legend mark did not match the retained legend state")
            }
        }
    }
}

trait Slotted {
    fn slot(&self) -> u8;
}

impl Slotted for InventoryItem {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl Slotted for Spell {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl Slotted for Skill {
    fn slot(&self) -> u8 {
        self.slot
    }
}

fn apply_slot_update<T: Clone + Eq + Slotted>(
    collection: &mut Vec<T>,
    update: SlotUpdate<T>,
    kind: CollectionKind,
) -> Result<(), ApplyEventError> {
    if update.batch_count == 0 || update.batch_index >= update.batch_count {
        return Err(ApplyEventError::InvalidCollectionBatch {
            collection: kind,
            index: update.batch_index,
            count: update.batch_count,
        });
    }
    if update.slot == 0
        || update
            .before
            .as_ref()
            .is_some_and(|item| item.slot() != update.slot)
        || update
            .after
            .as_ref()
            .is_some_and(|item| item.slot() != update.slot)
    {
        return Err(ApplyEventError::InvalidCollectionSlot {
            collection: kind,
            slot: update.slot,
        });
    }
    let current = collection
        .iter()
        .find(|item| item.slot() == update.slot)
        .cloned();
    if current != update.before {
        return Err(ApplyEventError::CollectionSlotMismatch {
            collection: kind,
            slot: update.slot,
        });
    }
    collection.retain(|item| item.slot() != update.slot);
    if let Some(after) = update.after {
        collection.push(after);
        collection.sort_unstable_by_key(Slotted::slot);
    }
    Ok(())
}

struct WorldObjectSortKey;

impl WorldObjectSortKey {
    fn of(object: &crate::WorldObject) -> (i32, i32, u8, u16, u32) {
        match object {
            crate::WorldObject::Player { id, x, y, .. } => (*y, *x, 0, 0, *id),
            crate::WorldObject::Creature { id, x, y, .. } => (*y, *x, 1, 0, *id),
            crate::WorldObject::Item {
                id, x, y, z_index, ..
            } => (*y, *x, 2, *z_index, *id),
        }
    }
}

impl Error for ApplyEventError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_command_splits_trimmed_nonempty_arguments() {
        assert_eq!(
            ClientCommand::parse("walk  x, , y,  "),
            Some(ClientCommand {
                command: "walk".into(),
                args: vec!["x".into(), "y".into()],
            })
        );
        assert_eq!(
            ClientCommand::parse("refresh"),
            Some(ClientCommand {
                command: "refresh".into(),
                args: Vec::new(),
            })
        );
        assert_eq!(ClientCommand::parse("   "), None);
    }

    fn empty_snapshot(lifecycle: ClientLifecycle) -> ClientSnapshot {
        ClientSnapshot {
            revision: 1,
            event_sequence: 1,
            captured_tick_ms: 10,
            updated_tick_ms: 10,
            capture_duration_us: 0,
            world_generation: 1,
            lifecycle,
            character: None,
            objects: None,
            dialog: None,
            group: None,
            exchange: None,
            legend: None,
            planned_route: None,
            map_exclusions: Vec::new(),
        }
    }

    #[test]
    fn lifecycle_update_requires_and_replaces_the_previous_state() {
        let mut snapshot = empty_snapshot(ClientLifecycle::Title);
        snapshot
            .apply_event(StateEvent {
                sequence: 2,
                revision: 2,
                tick_ms: 20,
                update: StateUpdate::Lifecycle(LifecycleUpdate {
                    previous: ClientLifecycle::Title,
                    current: ClientLifecycle::InGame,
                }),
            })
            .unwrap();
        assert_eq!(snapshot.lifecycle, ClientLifecycle::InGame);

        let error = snapshot
            .apply_event(StateEvent {
                sequence: 3,
                revision: 3,
                tick_ms: 30,
                update: StateUpdate::Lifecycle(LifecycleUpdate {
                    previous: ClientLifecycle::Title,
                    current: ClientLifecycle::Disconnected,
                }),
            })
            .unwrap_err();
        assert_eq!(
            error,
            ApplyEventError::UnexpectedLifecycle {
                expected: ClientLifecycle::InGame,
                actual: ClientLifecycle::Title,
            }
        );
        assert_eq!(snapshot.event_sequence, 2);
        assert_eq!(snapshot.lifecycle, ClientLifecycle::InGame);
    }

    #[test]
    fn map_change_clears_walking_and_the_previous_maps_route() {
        let mut snapshot = empty_snapshot(ClientLifecycle::InGame);
        snapshot.character = Some(crate::CharacterSnapshot {
            id: Some(7),
            name: Some("Silo".into()),
            identity: None,
            appearance: None,
            class: crate::CharacterClass::Warrior,
            is_action_restricted: false,
            is_blinded: false,
            is_casting: false,
            is_walking: true,
            gold: 0,
            weight: 0,
            max_weight: 0,
            progression: crate::CharacterProgression {
                level: 1,
                ability_level: 0,
                experience: 0,
                ability_points: None,
                experience_to_next_level: None,
                ability_to_next_level: None,
            },
            stats: CharacterStats {
                strength: 3,
                intelligence: 3,
                wisdom: 3,
                constitution: 3,
                dexterity: 3,
            },
            vitals: crate::CharacterVitals {
                health: 50,
                max_health: 50,
                mana: 25,
                max_mana: 25,
            },
            modifiers: None,
            location: Some(MapLocation {
                id: 1,
                name: Some("Mileth".into()),
                x: Some(10),
                y: Some(20),
                width: 100,
                height: 100,
            }),
            inventory: None,
            equipment: None,
            spellbook: None,
            skillbook: None,
            effects: None,
        });
        snapshot.planned_route = Some(PlannedRoute {
            generation: 9,
            tiles: vec![TilePosition { x: 11, y: 20 }],
        });

        snapshot
            .apply_event(StateEvent {
                sequence: 2,
                revision: 2,
                tick_ms: 20,
                update: StateUpdate::Location(LocationUpdate {
                    x: 2,
                    y: 3,
                    map: Some(MapChange {
                        id: 2,
                        name: Some("Abel".into()),
                        width: 80,
                        height: 90,
                    }),
                }),
            })
            .unwrap();

        let character = snapshot.character.unwrap();
        assert!(!character.is_walking);
        assert_eq!(character.location.unwrap().id, 2);
        assert_eq!(
            snapshot.planned_route,
            Some(PlannedRoute {
                generation: 9,
                tiles: Vec::new(),
            })
        );
    }

    #[test]
    fn map_exclusion_updates_reduce_into_the_sorted_session_registry() {
        let mut snapshot = empty_snapshot(ClientLifecycle::InGame);
        for (sequence, exclusions) in [
            MapExclusions {
                map_id: 20,
                tiles: vec![TilePosition { x: 2, y: 3 }],
            },
            MapExclusions {
                map_id: 10,
                tiles: vec![TilePosition { x: 4, y: 5 }],
            },
        ]
        .into_iter()
        .enumerate()
        {
            snapshot
                .apply_event(StateEvent {
                    sequence: u32::try_from(sequence + 2).unwrap(),
                    revision: u32::try_from(sequence + 2).unwrap(),
                    tick_ms: 20,
                    update: StateUpdate::MapExclusions(MapExclusionsUpdate::Replaced {
                        exclusions,
                        map_count: u16::try_from(sequence + 1).unwrap(),
                    }),
                })
                .unwrap();
        }
        assert_eq!(
            snapshot
                .map_exclusions
                .iter()
                .map(|entry| entry.map_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );

        snapshot
            .apply_event(StateEvent {
                sequence: 4,
                revision: 4,
                tick_ms: 30,
                update: StateUpdate::MapExclusions(MapExclusionsUpdate::Removed {
                    map_id: 10,
                    map_count: 1,
                }),
            })
            .unwrap();
        assert_eq!(snapshot.map_exclusions[0].map_id, 20);

        snapshot
            .apply_event(StateEvent {
                sequence: 5,
                revision: 5,
                tick_ms: 40,
                update: StateUpdate::MapExclusions(MapExclusionsUpdate::Cleared {
                    removed_map_count: 1,
                }),
            })
            .unwrap();
        assert!(snapshot.map_exclusions.is_empty());

        let mut snapshot = empty_snapshot(ClientLifecycle::InGame);
        let error = snapshot
            .apply_event(StateEvent {
                sequence: 2,
                revision: 2,
                tick_ms: 50,
                update: StateUpdate::MapExclusions(MapExclusionsUpdate::Replaced {
                    exclusions: MapExclusions {
                        map_id: 10,
                        tiles: vec![TilePosition { x: 1, y: 2 }],
                    },
                    map_count: 2,
                }),
            })
            .unwrap_err();
        assert_eq!(
            error,
            ApplyEventError::MapExclusionCountMismatch {
                expected: 1,
                actual: 2,
            }
        );
        assert!(snapshot.map_exclusions.is_empty());
    }

    fn item(slot: u8, quantity: u32) -> InventoryItem {
        InventoryItem {
            slot,
            sprite: 21,
            dye_color: 2,
            name: Some("Hy-Brasyl Gauntlet".into()),
            quantity,
            can_stack: quantity > 1,
            durability: 900,
            max_durability: 1_000,
        }
    }

    #[test]
    fn collection_move_reduces_without_false_add_or_remove() {
        let mut inventory = vec![item(1, 1)];
        apply_slot_update(
            &mut inventory,
            SlotUpdate {
                batch_index: 0,
                batch_count: 2,
                change: CollectionChange::Changed,
                slot: 1,
                before: Some(item(1, 1)),
                after: None,
            },
            CollectionKind::Inventory,
        )
        .unwrap();
        apply_slot_update(
            &mut inventory,
            SlotUpdate {
                batch_index: 1,
                batch_count: 2,
                change: CollectionChange::Changed,
                slot: 2,
                before: None,
                after: Some(item(2, 1)),
            },
            CollectionKind::Inventory,
        )
        .unwrap();

        assert_eq!(inventory, vec![item(2, 1)]);
    }

    #[test]
    fn collection_update_rejects_a_stale_baseline() {
        let mut inventory = vec![item(1, 2)];
        let error = apply_slot_update(
            &mut inventory,
            SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Removed,
                slot: 1,
                before: Some(item(1, 1)),
                after: None,
            },
            CollectionKind::Inventory,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ApplyEventError::CollectionSlotMismatch {
                collection: CollectionKind::Inventory,
                slot: 1,
            }
        );
        assert_eq!(inventory, vec![item(1, 2)]);
    }
}
