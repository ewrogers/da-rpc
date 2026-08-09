use crate::{
    messages::Message,
    registry::{ClientIdentity, hex},
    state::{EffectDuration, Element, InventoryItem, Skill, Spell, WorldObject},
};
use async_stream::stream;
use axum::response::{
    IntoResponse,
    sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, Utc};
use darpc_model::{
    CollectionChange, CreatureKind, Effect, MovementUpdate, ObjectUpdate, StateEvent, StateUpdate,
    TilePosition as ModelTilePosition,
};
use serde::Serialize;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast;
use utoipa::ToSchema;

mod action;
use action::*;
mod entity;
use entity::*;
mod feedback;
use feedback::*;
pub(crate) use feedback::{SpellFeedback, SpellFeedbackTrackers};

pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 4_096;

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A spell-effect icon became active.
pub(crate) struct EffectAdded {
    observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
    /// Relative remaining-duration color band, not an exact time.
    duration: EffectDuration,
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
    observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
}

impl EffectRemoved {
    const fn new(observation: EventObservation, icon: u16) -> Self {
        Self { observation, icon }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// The relative remaining-duration band changed for an active icon.
pub(crate) struct EffectChanged {
    observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
    /// New relative remaining-duration color band, not an exact time.
    duration: EffectDuration,
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
    observation: EventObservation,
    /// Zero-based position within this collection batch.
    batch_index: u16,
    /// Total number of slot changes in this collection batch.
    batch_count: u16,
    slot: u8,
    before: Option<InventoryItem>,
    after: Option<InventoryItem>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One occupied spellbook slot changed as part of an atomic collection batch.
pub(crate) struct SpellSlotChanged {
    observation: EventObservation,
    /// Zero-based position within this collection batch.
    batch_index: u16,
    /// Total number of slot changes in this collection batch.
    batch_count: u16,
    slot: u8,
    before: Option<Spell>,
    after: Option<Spell>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One occupied skillbook slot changed as part of an atomic collection batch.
pub(crate) struct SkillSlotChanged {
    observation: EventObservation,
    /// Zero-based position within this collection batch.
    batch_index: u16,
    /// Total number of slot changes in this collection batch.
    batch_count: u16,
    slot: u8,
    before: Option<Skill>,
    after: Option<Skill>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A skill packet was submitted by the client.
pub(crate) struct SkillUsed {
    observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A positive-line spell entered the native delayed-cast sequence.
pub(crate) struct SpellBegin {
    observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    total_lines: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One visible chant line was submitted during a delayed spell cast.
pub(crate) struct SpellChant {
    observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    line: u8,
    total_lines: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// The final spell packet was submitted by the client.
pub(crate) struct SpellCast {
    observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<SpellCastArguments>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SpellCastArguments {
    Unknown,
    Target {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        x: i32,
        y: i32,
    },
    Input {
        value: String,
    },
    Values {
        values: Vec<u16>,
    },
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A positive-line cast ended before its final spell packet was submitted.
pub(crate) struct SpellCancelled {
    observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    source: SpellCancellationSource,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpellCancellationSource {
    Client,
    Server,
    Replaced,
}

#[derive(Clone, Debug)]
pub(crate) enum PublishedEvent {
    #[cfg_attr(not(windows), allow(dead_code))]
    State {
        pid: u32,
        identity: ClientIdentity,
        event: Box<StateEvent>,
        ability_name: Option<String>,
        target_name: Option<String>,
        feedback: Option<SpellFeedback>,
        observed_at_utc: DateTime<Utc>,
    },
    #[cfg_attr(not(windows), allow(dead_code))]
    Closed {
        pid: u32,
        identity: ClientIdentity,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
/// JSON envelope carried by the `data` field of each Server-Sent Events frame.
/// The transport-level `event` and `id` fields are emitted separately.
pub(crate) enum ClientEvent {
    StreamReady(StreamReady),
    StatsChanged(StatsChanged),
    VitalsChanged(VitalsChanged),
    ProgressionChanged(ProgressionChanged),
    GoldChanged(GoldChanged),
    WeightChanged(WeightChanged),
    ModifiersChanged(ModifiersChanged),
    LocationChanged(LocationChanged),
    BlindChanged(BlindChanged),
    WalkingStarted(WalkingStarted),
    WalkingStopped(WalkingStopped),
    ActionRestrictionChanged(ActionRestrictionChanged),
    EffectAdded(EffectAdded),
    EffectRemoved(EffectRemoved),
    EffectChanged(EffectChanged),
    ItemAdded(InventorySlotChanged),
    ItemRemoved(InventorySlotChanged),
    ItemChanged(InventorySlotChanged),
    SpellAdded(SpellSlotChanged),
    SpellRemoved(SpellSlotChanged),
    SpellChanged(SpellSlotChanged),
    SkillAdded(SkillSlotChanged),
    SkillRemoved(SkillSlotChanged),
    SkillChanged(SkillSlotChanged),
    SkillUsed(SkillUsed),
    SpellBegin(SpellBegin),
    SpellChant(SpellChant),
    SpellCast(SpellCast),
    SpellCancelled(SpellCancelled),
    SpellSucceeded(SpellSucceeded),
    SpellFailed(SpellFailed),
    SpellReceived(SpellReceived),
    ItemUsed(ItemUsed),
    ItemDropped(ItemDropped),
    ItemGiven(ItemGiven),
    GoldDropped(GoldDropped),
    GoldGiven(GoldGiven),
    ItemPickedUp(ItemPickedUp),
    EquipmentUnequipped(EquipmentUnequipped),
    Emoted(Emoted),
    Turned(Turned),
    PlayerAppeared(ObjectChanged),
    PlayerDisappeared(ObjectChanged),
    PlayerMoved(ObjectChanged),
    PlayerDirectionChanged(ObjectChanged),
    MonsterAppeared(ObjectChanged),
    MonsterDisappeared(ObjectChanged),
    MonsterMoved(ObjectChanged),
    MonsterDirectionChanged(ObjectChanged),
    MundaneAppeared(ObjectChanged),
    MundaneDisappeared(ObjectChanged),
    MundaneMoved(ObjectChanged),
    MundaneDirectionChanged(ObjectChanged),
    PlayerAnimated(EntityAnimated),
    MonsterAnimated(EntityAnimated),
    MundaneAnimated(EntityAnimated),
    PlayerEffect(EntityEffect),
    MonsterEffect(EntityEffect),
    MundaneEffect(EntityEffect),
    PlayerDamaged(EntityDamaged),
    MonsterDamaged(EntityDamaged),
    MundaneDamaged(EntityDamaged),
    ItemAppeared(ObjectChanged),
    ItemDisappeared(ObjectChanged),
    ItemMoved(ObjectChanged),
    ObjectsCleared(ObjectsCleared),
    Message(Message),
    StreamResyncRequired(StreamResyncRequired),
    StreamClosed(StreamClosed),
}

impl ClientEvent {
    const fn name(&self) -> &'static str {
        match self {
            Self::StreamReady(_) => "stream.ready",
            Self::StatsChanged(_) => "stats.changed",
            Self::VitalsChanged(_) => "vitals.changed",
            Self::ProgressionChanged(_) => "progression.changed",
            Self::GoldChanged(_) => "gold.changed",
            Self::WeightChanged(_) => "weight.changed",
            Self::ModifiersChanged(_) => "modifiers.changed",
            Self::LocationChanged(_) => "location.changed",
            Self::BlindChanged(_) => "blind.changed",
            Self::WalkingStarted(_) => "walking.started",
            Self::WalkingStopped(_) => "walking.stopped",
            Self::ActionRestrictionChanged(_) => "action_restriction.changed",
            Self::EffectAdded(_) => "effect.added",
            Self::EffectRemoved(_) => "effect.removed",
            Self::EffectChanged(_) => "effect.changed",
            Self::ItemAdded(_) => "item.added",
            Self::ItemRemoved(_) => "item.removed",
            Self::ItemChanged(_) => "item.changed",
            Self::SpellAdded(_) => "spell.added",
            Self::SpellRemoved(_) => "spell.removed",
            Self::SpellChanged(_) => "spell.changed",
            Self::SkillAdded(_) => "skill.added",
            Self::SkillRemoved(_) => "skill.removed",
            Self::SkillChanged(_) => "skill.changed",
            Self::SkillUsed(_) => "skill.used",
            Self::SpellBegin(_) => "spell.begin",
            Self::SpellChant(_) => "spell.chant",
            Self::SpellCast(_) => "spell.cast",
            Self::SpellCancelled(_) => "spell.cancelled",
            Self::SpellSucceeded(_) => "spell.succeeded",
            Self::SpellFailed(_) => "spell.failed",
            Self::SpellReceived(_) => "spell.received",
            Self::ItemUsed(_) => "item.used",
            Self::ItemDropped(_) => "item.dropped",
            Self::ItemGiven(_) => "item.given",
            Self::GoldDropped(_) => "gold.dropped",
            Self::GoldGiven(_) => "gold.given",
            Self::ItemPickedUp(_) => "item.picked_up",
            Self::EquipmentUnequipped(_) => "equipment.unequipped",
            Self::Emoted(_) => "character.emoted",
            Self::Turned(_) => "character.turned",
            Self::PlayerAppeared(_) => "player.appeared",
            Self::PlayerDisappeared(_) => "player.disappeared",
            Self::PlayerMoved(_) => "player.moved",
            Self::PlayerDirectionChanged(_) => "player.direction_changed",
            Self::MonsterAppeared(_) => "monster.appeared",
            Self::MonsterDisappeared(_) => "monster.disappeared",
            Self::MonsterMoved(_) => "monster.moved",
            Self::MonsterDirectionChanged(_) => "monster.direction_changed",
            Self::MundaneAppeared(_) => "mundane.appeared",
            Self::MundaneDisappeared(_) => "mundane.disappeared",
            Self::MundaneMoved(_) => "mundane.moved",
            Self::MundaneDirectionChanged(_) => "mundane.direction_changed",
            Self::PlayerAnimated(_) => "player.animated",
            Self::MonsterAnimated(_) => "monster.animated",
            Self::MundaneAnimated(_) => "mundane.animated",
            Self::PlayerEffect(_) => "player.effect",
            Self::MonsterEffect(_) => "monster.effect",
            Self::MundaneEffect(_) => "mundane.effect",
            Self::PlayerDamaged(_) => "player.damaged",
            Self::MonsterDamaged(_) => "monster.damaged",
            Self::MundaneDamaged(_) => "mundane.damaged",
            Self::ItemAppeared(_) => "item.appeared",
            Self::ItemDisappeared(_) => "item.disappeared",
            Self::ItemMoved(_) => "item.moved",
            Self::ObjectsCleared(_) => "objects.cleared",
            Self::Message(message) => message.event_name(),
            Self::StreamResyncRequired(_) => "stream.resync_required",
            Self::StreamClosed(_) => "stream.closed",
        }
    }

    fn sequence(&self) -> u32 {
        match self {
            Self::StreamReady(value) => value.event_sequence,
            Self::StatsChanged(value) => value.observation.event_sequence,
            Self::VitalsChanged(value) => value.observation.event_sequence,
            Self::ProgressionChanged(value) => value.observation.event_sequence,
            Self::GoldChanged(value) => value.observation.event_sequence,
            Self::WeightChanged(value) => value.observation.event_sequence,
            Self::ModifiersChanged(value) => value.observation.event_sequence,
            Self::LocationChanged(value) => value.observation.event_sequence,
            Self::BlindChanged(value) => value.observation.event_sequence,
            Self::WalkingStarted(value) => value.observation.event_sequence,
            Self::WalkingStopped(value) => value.observation.event_sequence,
            Self::ActionRestrictionChanged(value) => value.observation.event_sequence,
            Self::EffectAdded(value) => value.observation.event_sequence,
            Self::EffectRemoved(value) => value.observation.event_sequence,
            Self::EffectChanged(value) => value.observation.event_sequence,
            Self::ItemAdded(value) | Self::ItemRemoved(value) | Self::ItemChanged(value) => {
                value.observation.event_sequence
            }
            Self::SpellAdded(value) | Self::SpellRemoved(value) | Self::SpellChanged(value) => {
                value.observation.event_sequence
            }
            Self::SkillAdded(value) | Self::SkillRemoved(value) | Self::SkillChanged(value) => {
                value.observation.event_sequence
            }
            Self::SkillUsed(value) => value.observation.event_sequence,
            Self::SpellBegin(value) => value.observation.event_sequence,
            Self::SpellChant(value) => value.observation.event_sequence,
            Self::SpellCast(value) => value.observation.event_sequence,
            Self::SpellCancelled(value) => value.observation.event_sequence,
            Self::SpellSucceeded(value) => value.observation.event_sequence,
            Self::SpellFailed(value) => value.observation.event_sequence,
            Self::SpellReceived(value) => value.observation.event_sequence,
            Self::ItemUsed(value) => value.observation.event_sequence,
            Self::ItemDropped(value) => value.observation.event_sequence,
            Self::ItemGiven(value) => value.observation.event_sequence,
            Self::GoldDropped(value) => value.observation.event_sequence,
            Self::GoldGiven(value) => value.observation.event_sequence,
            Self::ItemPickedUp(value) => value.observation.event_sequence,
            Self::EquipmentUnequipped(value) => value.observation.event_sequence,
            Self::Emoted(value) => value.observation.event_sequence,
            Self::Turned(value) => value.observation.event_sequence,
            Self::PlayerAppeared(value)
            | Self::PlayerDisappeared(value)
            | Self::PlayerMoved(value)
            | Self::PlayerDirectionChanged(value)
            | Self::MonsterAppeared(value)
            | Self::MonsterDisappeared(value)
            | Self::MonsterMoved(value)
            | Self::MonsterDirectionChanged(value)
            | Self::MundaneAppeared(value)
            | Self::MundaneDisappeared(value)
            | Self::MundaneMoved(value)
            | Self::MundaneDirectionChanged(value)
            | Self::ItemAppeared(value)
            | Self::ItemDisappeared(value)
            | Self::ItemMoved(value) => value.observation.event_sequence,
            Self::PlayerAnimated(value)
            | Self::MonsterAnimated(value)
            | Self::MundaneAnimated(value) => value.observation.event_sequence,
            Self::PlayerEffect(value) | Self::MonsterEffect(value) | Self::MundaneEffect(value) => {
                value.observation.event_sequence
            }
            Self::PlayerDamaged(value)
            | Self::MonsterDamaged(value)
            | Self::MundaneDamaged(value) => value.observation.event_sequence,
            Self::ObjectsCleared(value) => value.observation.event_sequence,
            Self::Message(message) => message.sequence(),
            Self::StreamResyncRequired(value) => value.last_event_sequence,
            Self::StreamClosed(value) => value.last_event_sequence,
        }
    }

    fn into_sse(self) -> Event {
        let event_name = self.name();
        let event_id = self.sequence().to_string();
        Event::default()
            .event(event_name)
            .id(event_id)
            .json_data(self)
            .expect("client event serialization is infallible")
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// Snapshot boundary sent as the first event on every subscription.
pub(crate) struct StreamReady {
    pid: u32,
    instance_id: String,
    revision: u32,
    event_sequence: u32,
}

impl StreamReady {
    pub(crate) fn new(
        pid: u32,
        identity: ClientIdentity,
        revision: u32,
        event_sequence: u32,
    ) -> Self {
        Self {
            pid,
            instance_id: hex(&identity.dll_instance_id),
            revision,
            event_sequence,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// Source identity and ordering metadata shared by state-change payloads.
pub(crate) struct EventObservation {
    pid: u32,
    instance_id: String,
    revision: u32,
    event_sequence: u32,
    tick_ms: u32,
}

impl EventObservation {
    pub(crate) fn new(pid: u32, identity: ClientIdentity, event: &StateEvent) -> Self {
        Self {
            pid,
            instance_id: hex(&identity.dll_instance_id),
            revision: event.revision,
            event_sequence: event.sequence,
            tick_ms: event.tick_ms,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StatsChanged {
    observation: EventObservation,
    strength: u16,
    intelligence: u16,
    wisdom: u16,
    constitution: u16,
    dexterity: u16,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct VitalsChanged {
    observation: EventObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    health: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_health: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mana: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_mana: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ProgressionChanged {
    observation: EventObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ability_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    experience: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ability_points: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    experience_to_next_level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ability_to_next_level: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GoldChanged {
    observation: EventObservation,
    gold: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WeightChanged {
    observation: EventObservation,
    weight: u32,
    max_weight: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ModifiersChanged {
    observation: EventObservation,
    armor_class: i8,
    damage: u8,
    hit: u8,
    magic_resistance: u16,
    attack_element: Element,
    defense_element: Element,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LocationChanged {
    observation: EventObservation,
    x: i32,
    y: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    map: Option<MapChanged>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MapChanged {
    id: u32,
    name: Option<String>,
    width: i32,
    height: i32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BlindChanged {
    observation: EventObservation,
    is_blinded: bool,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct TilePosition {
    x: i32,
    y: i32,
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
    observation: EventObservation,
    current: TilePosition,
    destination: Option<TilePosition>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct WalkingStopped {
    observation: EventObservation,
    current: TilePosition,
    destination: Option<TilePosition>,
    reached_destination: Option<bool>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ActionRestrictionChanged {
    observation: EventObservation,
    is_action_restricted: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ObjectChanged {
    observation: EventObservation,
    object: WorldObject,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ObjectsCleared {
    observation: EventObservation,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StreamResyncRequired {
    pid: u32,
    instance_id: String,
    last_event_sequence: u32,
    dropped_events: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct StreamClosed {
    pid: u32,
    instance_id: String,
    last_event_sequence: u32,
    reason: String,
}

pub(crate) fn response(
    pid: u32,
    identity: ClientIdentity,
    revision: u32,
    event_sequence: u32,
    mut receiver: broadcast::Receiver<PublishedEvent>,
) -> impl IntoResponse {
    let events = stream! {
        let ready = ClientEvent::StreamReady(StreamReady::new(
            pid,
            identity,
            revision,
            event_sequence,
        ));
        yield Ok::<Event, Infallible>(ready.into_sse());

        let mut last_sequence = event_sequence;
        loop {
            match receiver.recv().await {
                Ok(PublishedEvent::State {
                    pid: event_pid,
                    identity: event_identity,
                    event,
                    ability_name,
                    target_name,
                    feedback,
                    observed_at_utc,
                }) if event_pid == pid && event_identity == identity => {
                    if !sequence_after(event.sequence, last_sequence) {
                        continue;
                    }
                    if event.sequence != next_nonzero(last_sequence) {
                        let resync = ClientEvent::StreamResyncRequired(StreamResyncRequired {
                            pid,
                            instance_id: hex(&identity.dll_instance_id),
                            last_event_sequence: last_sequence,
                            dropped_events: 0,
                        });
                        yield Ok(resync.into_sse());
                        break;
                    }
                    last_sequence = event.sequence;
                    let feedback_observation = EventObservation::new(pid, identity, &event);
                    let mut api_events = expand(
                        pid,
                        identity,
                        *event,
                        ability_name,
                        target_name,
                        observed_at_utc,
                    );
                    if let Some(feedback) = feedback {
                        api_events.push(feedback.into_event(feedback_observation));
                    }
                    for api_event in api_events {
                        yield Ok(api_event.into_sse());
                    }
                }
                Ok(PublishedEvent::Closed {
                    pid: event_pid,
                    identity: event_identity,
                    reason,
                }) if event_pid == pid && event_identity == identity => {
                    let closed = ClientEvent::StreamClosed(StreamClosed {
                        pid,
                        instance_id: hex(&identity.dll_instance_id),
                        last_event_sequence: last_sequence,
                        reason,
                    });
                    yield Ok(closed.into_sse());
                    break;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(dropped_events)) => {
                    let resync = ClientEvent::StreamResyncRequired(StreamResyncRequired {
                        pid,
                        instance_id: hex(&identity.dll_instance_id),
                        last_event_sequence: last_sequence,
                        dropped_events,
                    });
                    yield Ok(resync.into_sse());
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

fn expand(
    pid: u32,
    identity: ClientIdentity,
    event: StateEvent,
    ability_name: Option<String>,
    target_name: Option<String>,
    observed_at_utc: DateTime<Utc>,
) -> Vec<ClientEvent> {
    let observation = EventObservation::new(pid, identity, &event);
    let mut events = Vec::with_capacity(9);
    let update = match event.update {
        StateUpdate::Status(update) => update,
        StateUpdate::Location(update) => {
            events.push(ClientEvent::LocationChanged(LocationChanged {
                observation,
                x: update.x,
                y: update.y,
                map: update.map.map(|map| MapChanged {
                    id: map.id,
                    name: map.name,
                    width: map.width,
                    height: map.height,
                }),
            }));
            return events;
        }
        StateUpdate::Movement(update) => {
            events.push(match update {
                MovementUpdate::Started {
                    current,
                    destination,
                } => ClientEvent::WalkingStarted(WalkingStarted {
                    observation,
                    current: current.into(),
                    destination: destination.map(Into::into),
                }),
                MovementUpdate::Stopped {
                    current,
                    destination,
                    reached_destination,
                } => ClientEvent::WalkingStopped(WalkingStopped {
                    observation,
                    current: current.into(),
                    destination: destination.map(Into::into),
                    reached_destination,
                }),
            });
            return events;
        }
        StateUpdate::Effect(update) => {
            events.push(match update {
                darpc_model::EffectUpdate::Added(effect) => {
                    ClientEvent::EffectAdded(EffectAdded::new(observation, effect))
                }
                darpc_model::EffectUpdate::Removed { icon } => {
                    ClientEvent::EffectRemoved(EffectRemoved::new(observation, icon))
                }
                darpc_model::EffectUpdate::Changed(effect) => {
                    ClientEvent::EffectChanged(EffectChanged::new(observation, effect))
                }
            });
            return events;
        }
        StateUpdate::Inventory(update) => {
            let change = update.change;
            let payload = InventorySlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(InventoryItem::from),
                after: update.after.as_ref().map(InventoryItem::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::ItemAdded(payload),
                CollectionChange::Removed => ClientEvent::ItemRemoved(payload),
                CollectionChange::Changed => ClientEvent::ItemChanged(payload),
            });
            return events;
        }
        StateUpdate::Spellbook(update) => {
            let change = update.change;
            let payload = SpellSlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Spell::from),
                after: update.after.as_ref().map(Spell::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::SpellAdded(payload),
                CollectionChange::Removed => ClientEvent::SpellRemoved(payload),
                CollectionChange::Changed => ClientEvent::SpellChanged(payload),
            });
            return events;
        }
        StateUpdate::Skillbook(update) => {
            let change = update.change;
            let payload = SkillSlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Skill::from),
                after: update.after.as_ref().map(Skill::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::SkillAdded(payload),
                CollectionChange::Removed => ClientEvent::SkillRemoved(payload),
                CollectionChange::Changed => ClientEvent::SkillChanged(payload),
            });
            return events;
        }
        StateUpdate::Ability(update) => {
            events.push(match update {
                darpc_model::AbilityUpdate::SkillUsed { slot } => {
                    ClientEvent::SkillUsed(SkillUsed {
                        observation,
                        slot,
                        name: ability_name,
                    })
                }
                darpc_model::AbilityUpdate::SpellBegin { slot, total_lines } => {
                    ClientEvent::SpellBegin(SpellBegin {
                        observation,
                        slot,
                        name: ability_name,
                        total_lines,
                    })
                }
                darpc_model::AbilityUpdate::SpellChant {
                    slot,
                    line,
                    total_lines,
                } => ClientEvent::SpellChant(SpellChant {
                    observation,
                    slot,
                    name: ability_name,
                    line,
                    total_lines,
                }),
                darpc_model::AbilityUpdate::SpellCast { slot, arguments } => {
                    ClientEvent::SpellCast(SpellCast {
                        observation,
                        slot,
                        name: ability_name,
                        arguments: spell_arguments(arguments, target_name),
                    })
                }
                darpc_model::AbilityUpdate::SpellCancelled { slot, source } => {
                    ClientEvent::SpellCancelled(SpellCancelled {
                        observation,
                        slot,
                        name: ability_name,
                        source: match source {
                            darpc_model::SpellCancellationSource::Client => {
                                SpellCancellationSource::Client
                            }
                            darpc_model::SpellCancellationSource::Server => {
                                SpellCancellationSource::Server
                            }
                            darpc_model::SpellCancellationSource::Replaced => {
                                SpellCancellationSource::Replaced
                            }
                        },
                    })
                }
            });
            return events;
        }
        StateUpdate::Action(update) => {
            events.push(action::expand(observation, update));
            return events;
        }
        StateUpdate::Entity(update) => {
            if let Some(event) = entity::expand(observation, update) {
                events.push(event);
            }
            return events;
        }
        StateUpdate::Object(update) => {
            if let Some(event) = object_event(observation, update) {
                events.push(event);
            }
            return events;
        }
        StateUpdate::Message(message) => {
            if message.text.trim().is_empty() {
                return events;
            }
            events.push(ClientEvent::Message(Message::new(
                event.sequence,
                event.tick_ms,
                observed_at_utc,
                message,
            )));
            return events;
        }
    };
    if let Some(core) = update.core {
        events.push(ClientEvent::StatsChanged(StatsChanged {
            observation: observation.clone(),
            strength: core.stats.strength,
            intelligence: core.stats.intelligence,
            wisdom: core.stats.wisdom,
            constitution: core.stats.constitution,
            dexterity: core.stats.dexterity,
        }));
        events.push(ClientEvent::WeightChanged(WeightChanged {
            observation: observation.clone(),
            weight: core.weight,
            max_weight: core.max_weight,
        }));
    }
    if update.core.is_some() || update.vitals.is_some() {
        events.push(ClientEvent::VitalsChanged(VitalsChanged {
            observation: observation.clone(),
            health: update.vitals.map(|value| value.health),
            max_health: update.core.map(|value| value.max_health),
            mana: update.vitals.map(|value| value.mana),
            max_mana: update.core.map(|value| value.max_mana),
        }));
    }
    if update.core.is_some() || update.progression.is_some() {
        events.push(ClientEvent::ProgressionChanged(ProgressionChanged {
            observation: observation.clone(),
            level: update.core.map(|value| value.level),
            ability_level: update.core.map(|value| value.ability_level),
            experience: update.progression.map(|value| value.experience),
            ability_points: update.progression.map(|value| value.ability_points),
            experience_to_next_level: update
                .progression
                .map(|value| value.experience_to_next_level),
            ability_to_next_level: update.progression.map(|value| value.ability_to_next_level),
        }));
    }
    if let Some(gold) = update.gold {
        events.push(ClientEvent::GoldChanged(GoldChanged {
            observation: observation.clone(),
            gold,
        }));
    }
    if let Some(modifiers) = update.modifiers {
        events.push(ClientEvent::ModifiersChanged(ModifiersChanged {
            observation: observation.clone(),
            armor_class: modifiers.armor_class,
            damage: modifiers.damage,
            hit: modifiers.hit,
            magic_resistance: modifiers.magic_resistance,
            attack_element: Element::from(modifiers.attack_element),
            defense_element: Element::from(modifiers.defense_element),
        }));
    }
    if let Some(is_blinded) = update.is_blinded {
        events.push(ClientEvent::BlindChanged(BlindChanged {
            observation: observation.clone(),
            is_blinded,
        }));
    }
    if let Some(is_action_restricted) = update.is_action_restricted {
        events.push(ClientEvent::ActionRestrictionChanged(
            ActionRestrictionChanged {
                observation,
                is_action_restricted,
            },
        ));
    }
    events
}

fn spell_arguments(
    arguments: darpc_model::SpellCastArguments,
    target_name: Option<String>,
) -> Option<SpellCastArguments> {
    match arguments {
        darpc_model::SpellCastArguments::Unknown => Some(SpellCastArguments::Unknown),
        darpc_model::SpellCastArguments::None => None,
        darpc_model::SpellCastArguments::Target { id, x, y } => Some(SpellCastArguments::Target {
            id,
            name: target_name,
            x,
            y,
        }),
        darpc_model::SpellCastArguments::Input(value) => Some(SpellCastArguments::Input { value }),
        darpc_model::SpellCastArguments::Values(values) => {
            Some(SpellCastArguments::Values { values })
        }
    }
}

#[derive(Clone, Copy)]
enum ObjectChangeKind {
    Appeared,
    Disappeared,
    Moved,
    DirectionChanged,
}

#[derive(Clone, Copy)]
enum ObjectCategory {
    Player,
    Monster,
    Npc,
    Item,
}

fn object_event(observation: EventObservation, update: ObjectUpdate) -> Option<ClientEvent> {
    let (kind, object) = match update {
        ObjectUpdate::Appeared(object) => (ObjectChangeKind::Appeared, object),
        ObjectUpdate::Disappeared(object) => (ObjectChangeKind::Disappeared, object),
        ObjectUpdate::Moved(object) => (ObjectChangeKind::Moved, object),
        ObjectUpdate::DirectionChanged(object) => (ObjectChangeKind::DirectionChanged, object),
        ObjectUpdate::Cleared => {
            return Some(ClientEvent::ObjectsCleared(ObjectsCleared { observation }));
        }
    };
    let category = match &object {
        darpc_model::WorldObject::Player { .. } => ObjectCategory::Player,
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Monster,
            ..
        } => ObjectCategory::Monster,
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Npc,
            ..
        } => ObjectCategory::Npc,
        darpc_model::WorldObject::Item { .. } => ObjectCategory::Item,
    };
    let changed = ObjectChanged {
        observation,
        object: WorldObject::from(&object),
    };
    Some(match (category, kind) {
        (ObjectCategory::Player, ObjectChangeKind::Appeared) => {
            ClientEvent::PlayerAppeared(changed)
        }
        (ObjectCategory::Player, ObjectChangeKind::Disappeared) => {
            ClientEvent::PlayerDisappeared(changed)
        }
        (ObjectCategory::Player, ObjectChangeKind::Moved) => ClientEvent::PlayerMoved(changed),
        (ObjectCategory::Player, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::PlayerDirectionChanged(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Appeared) => {
            ClientEvent::MonsterAppeared(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Disappeared) => {
            ClientEvent::MonsterDisappeared(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Moved) => ClientEvent::MonsterMoved(changed),
        (ObjectCategory::Monster, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::MonsterDirectionChanged(changed)
        }
        (ObjectCategory::Npc, ObjectChangeKind::Appeared) => ClientEvent::MundaneAppeared(changed),
        (ObjectCategory::Npc, ObjectChangeKind::Disappeared) => {
            ClientEvent::MundaneDisappeared(changed)
        }
        (ObjectCategory::Npc, ObjectChangeKind::Moved) => ClientEvent::MundaneMoved(changed),
        (ObjectCategory::Npc, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::MundaneDirectionChanged(changed)
        }
        (ObjectCategory::Item, ObjectChangeKind::Appeared) => ClientEvent::ItemAppeared(changed),
        (ObjectCategory::Item, ObjectChangeKind::Disappeared) => {
            ClientEvent::ItemDisappeared(changed)
        }
        (ObjectCategory::Item, ObjectChangeKind::Moved) => ClientEvent::ItemMoved(changed),
        (ObjectCategory::Item, ObjectChangeKind::DirectionChanged) => return None,
    })
}

const fn next_nonzero(value: u32) -> u32 {
    let next = value.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

fn sequence_after(candidate: u32, baseline: u32) -> bool {
    let distance = candidate.wrapping_sub(baseline);
    distance != 0 && distance < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::{
        AbilityUpdate, ActionUpdate, CharacterStats, ClientMessage, CollectionChange,
        CooldownStatus, CoreStatus, CurrentVitals, Effect, EffectDuration, EffectUpdate,
        EntityUpdate, InventoryItem as ModelInventoryItem, LocationUpdate, MapChange, MessageKind,
        MovementUpdate, Skill as ModelSkill, SlotUpdate, Spell as ModelSpell,
        SpellCancellationSource as ModelSpellCancellationSource,
        SpellCastArguments as ModelSpellCastArguments, SpellTargetType, StateUpdate, StatusUpdate,
        TilePosition as ModelTilePosition,
    };

    fn observed_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_775_000_000, 0).unwrap()
    }

    #[test]
    fn expands_one_atomic_status_update_into_semantic_events() {
        let event = StateEvent {
            sequence: 9,
            revision: 12,
            tick_ms: 500,
            update: StateUpdate::Status(StatusUpdate {
                core: Some(CoreStatus {
                    level: 50,
                    ability_level: 4,
                    max_health: 1_000,
                    max_mana: 800,
                    weight: 25,
                    max_weight: 60,
                    stats: CharacterStats {
                        strength: 10,
                        intelligence: 11,
                        wisdom: 12,
                        constitution: 13,
                        dexterity: 14,
                    },
                }),
                vitals: Some(CurrentVitals {
                    health: 900,
                    mana: 700,
                }),
                gold: Some(123),
                is_blinded: Some(true),
                is_action_restricted: Some(true),
                ..StatusUpdate::default()
            }),
        };
        let names = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            event,
            None,
            None,
            observed_at(),
        )
        .iter()
        .map(ClientEvent::name)
        .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "stats.changed",
                "weight.changed",
                "vitals.changed",
                "progression.changed",
                "gold.changed",
                "blind.changed",
                "action_restriction.changed",
            ]
        );
    }

    #[test]
    fn movement_updates_expose_route_lifecycle_context() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        let destination = ModelTilePosition { x: 6, y: 5 };
        let started = expand(
            42,
            identity,
            StateEvent {
                sequence: 10,
                revision: 13,
                tick_ms: 501,
                update: StateUpdate::Movement(MovementUpdate::Started {
                    current: ModelTilePosition { x: 2, y: 8 },
                    destination: Some(destination),
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].name(), "walking.started");
        let started = serde_json::to_value(&started[0]).unwrap();
        assert_eq!(started["data"]["current"]["x"], 2);
        assert_eq!(started["data"]["destination"]["y"], 5);

        let stopped = expand(
            42,
            identity,
            StateEvent {
                sequence: 11,
                revision: 14,
                tick_ms: 502,
                update: StateUpdate::Movement(MovementUpdate::Stopped {
                    current: destination,
                    destination: Some(destination),
                    reached_destination: Some(true),
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].name(), "walking.stopped");
        let stopped = serde_json::to_value(&stopped[0]).unwrap();
        assert_eq!(stopped["data"]["reached_destination"], true);
    }

    #[test]
    fn sequence_ordering_handles_nonzero_wrap() {
        assert_eq!(next_nonzero(u32::MAX), 1);
        assert!(sequence_after(1, u32::MAX));
        assert!(!sequence_after(u32::MAX, 1));
    }

    #[test]
    fn action_updates_expose_drop_payloads() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 1,
            dll_instance_id: [1; 16],
        };
        let item = expand(
            42,
            identity,
            StateEvent {
                sequence: 1,
                revision: 2,
                tick_ms: 3,
                update: StateUpdate::Action(ActionUpdate::ItemDropped {
                    slot: 4,
                    quantity: 2,
                    position: ModelTilePosition { x: 11, y: 22 },
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(item[0].name(), "item.dropped");
        let json = serde_json::to_value(&item[0]).unwrap();
        assert_eq!(json["data"]["slot"], 4);
        assert_eq!(json["data"]["quantity"], 2);
        assert_eq!(json["data"]["destination"]["x"], 11);

        let gold = expand(
            42,
            identity,
            StateEvent {
                sequence: 2,
                revision: 3,
                tick_ms: 4,
                update: StateUpdate::Action(ActionUpdate::GoldDropped {
                    amount: 500,
                    position: ModelTilePosition { x: 12, y: 23 },
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(gold[0].name(), "gold.dropped");
        let json = serde_json::to_value(&gold[0]).unwrap();
        assert_eq!(json["data"]["amount"], 500);
        assert_eq!(json["data"]["destination"]["y"], 23);
    }

    #[test]
    fn collection_updates_keep_the_requested_public_event_names() {
        let updates = [
            StateUpdate::Inventory(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Added,
                slot: 1,
                before: None,
                after: Some(ModelInventoryItem {
                    slot: 1,
                    sprite: 21,
                    dye_color: 2,
                    name: Some("Hy-Brasyl Gauntlet".into()),
                    quantity: 1,
                    can_stack: false,
                    durability: 900,
                    max_durability: 1_000,
                }),
            }),
            StateUpdate::Spellbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Removed,
                slot: 2,
                before: Some(ModelSpell {
                    slot: 2,
                    icon: 82,
                    name: Some("beag srad".into()),
                    level: 10,
                    max_level: 100,
                    lines: 2,
                    target_type: SpellTargetType::Target,
                    prompt: None,
                    cooldown: CooldownStatus {
                        active: false,
                        remaining_ms: None,
                    },
                }),
                after: None,
            }),
            StateUpdate::Skillbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: 3,
                before: Some(ModelSkill {
                    slot: 3,
                    icon: 91,
                    name: Some("Assail".into()),
                    level: 99,
                    max_level: 100,
                    cooldown: CooldownStatus {
                        active: false,
                        remaining_ms: None,
                    },
                }),
                after: Some(ModelSkill {
                    slot: 3,
                    icon: 91,
                    name: Some("Assail".into()),
                    level: 100,
                    max_level: 100,
                    cooldown: CooldownStatus {
                        active: false,
                        remaining_ms: None,
                    },
                }),
            }),
        ];
        let names = updates
            .into_iter()
            .enumerate()
            .map(|(index, update)| {
                let mut events = expand(
                    42,
                    ClientIdentity {
                        pid: 42,
                        process_creation_time: 100,
                        dll_instance_id: [1; 16],
                    },
                    StateEvent {
                        sequence: u32::try_from(index + 1).unwrap(),
                        revision: u32::try_from(index + 1).unwrap(),
                        tick_ms: 500,
                        update,
                    },
                    None,
                    None,
                    observed_at(),
                );
                assert_eq!(events.len(), 1);
                events.remove(0).name()
            })
            .collect::<Vec<_>>();

        assert_eq!(names, ["item.added", "spell.removed", "skill.changed"]);
    }

    #[test]
    fn map_transition_expands_as_one_location_event() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: 10,
                revision: 13,
                tick_ms: 501,
                update: StateUpdate::Location(LocationUpdate {
                    x: 43,
                    y: 40,
                    map: Some(MapChange {
                        id: 3001,
                        name: Some("Mileth".into()),
                        width: 100,
                        height: 80,
                    }),
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), "location.changed");
        let ClientEvent::LocationChanged(location) = &events[0] else {
            panic!("expected location event");
        };
        assert_eq!((location.x, location.y), (43, 40));
        assert_eq!(location.map.as_ref().unwrap().id, 3001);
    }

    #[test]
    fn effect_updates_use_noun_action_event_names() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        let effect = Effect {
            icon: 300,
            duration: EffectDuration::White,
        };
        for (sequence, update, expected) in [
            (1, EffectUpdate::Added(effect), "effect.added"),
            (
                2,
                EffectUpdate::Changed(Effect {
                    duration: EffectDuration::Red,
                    ..effect
                }),
                "effect.changed",
            ),
            (3, EffectUpdate::Removed { icon: 300 }, "effect.removed"),
        ] {
            let events = expand(
                42,
                identity,
                StateEvent {
                    sequence,
                    revision: sequence,
                    tick_ms: sequence,
                    update: StateUpdate::Effect(update),
                },
                None,
                None,
                observed_at(),
            );
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), expected);
        }
    }

    #[test]
    fn object_updates_use_noun_action_event_names() {
        use darpc_model::{Direction, WorldObject as ModelWorldObject};

        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        let player = ModelWorldObject::Player {
            id: 1,
            name: Some("Monitor".into()),
            x: 10,
            y: 20,
            direction: Direction::East,
        };
        let monster = ModelWorldObject::Creature {
            id: 2,
            kind: CreatureKind::Monster,
            sprite: Some(7),
            name: None,
            x: 11,
            y: 20,
            direction: Direction::South,
        };
        let npc = ModelWorldObject::Creature {
            id: 3,
            kind: CreatureKind::Npc,
            sprite: Some(8),
            name: Some("Maria".into()),
            x: 12,
            y: 20,
            direction: Direction::North,
        };
        let item = ModelWorldObject::Item {
            id: 4,
            sprite: 327,
            x: 13,
            y: 20,
            z_index: 0,
        };
        let cases = vec![
            (ObjectUpdate::Appeared(player.clone()), "player.appeared"),
            (
                ObjectUpdate::Disappeared(player.clone()),
                "player.disappeared",
            ),
            (ObjectUpdate::Moved(player.clone()), "player.moved"),
            (
                ObjectUpdate::DirectionChanged(player),
                "player.direction_changed",
            ),
            (ObjectUpdate::Appeared(monster.clone()), "monster.appeared"),
            (
                ObjectUpdate::Disappeared(monster.clone()),
                "monster.disappeared",
            ),
            (ObjectUpdate::Moved(monster.clone()), "monster.moved"),
            (
                ObjectUpdate::DirectionChanged(monster),
                "monster.direction_changed",
            ),
            (ObjectUpdate::Appeared(npc.clone()), "mundane.appeared"),
            (
                ObjectUpdate::Disappeared(npc.clone()),
                "mundane.disappeared",
            ),
            (ObjectUpdate::Moved(npc.clone()), "mundane.moved"),
            (
                ObjectUpdate::DirectionChanged(npc),
                "mundane.direction_changed",
            ),
            (ObjectUpdate::Appeared(item.clone()), "item.appeared"),
            (ObjectUpdate::Disappeared(item.clone()), "item.disappeared"),
            (ObjectUpdate::Moved(item), "item.moved"),
            (ObjectUpdate::Cleared, "objects.cleared"),
        ];

        for (index, (update, expected)) in cases.into_iter().enumerate() {
            let sequence = u32::try_from(index + 1).unwrap();
            let events = expand(
                42,
                identity,
                StateEvent {
                    sequence,
                    revision: sequence,
                    tick_ms: sequence,
                    update: StateUpdate::Object(update),
                },
                None,
                None,
                observed_at(),
            );
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), expected);
        }
    }

    #[test]
    fn entity_visual_updates_expose_packet_values() {
        use darpc_model::{Direction, WorldObject as ModelWorldObject};

        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        let player = ModelWorldObject::Player {
            id: 1,
            name: Some("ZiLo".into()),
            x: 10,
            y: 20,
            direction: Direction::East,
        };
        let mundane = ModelWorldObject::Creature {
            id: 2,
            kind: CreatureKind::Npc,
            sprite: Some(7),
            name: Some("Beggar".into()),
            x: 11,
            y: 20,
            direction: Direction::South,
        };
        let cases = [
            (
                StateUpdate::Entity(EntityUpdate::Animated {
                    entity: player.clone(),
                    animation: 9,
                    duration_10ms: 25,
                }),
                "player.animated",
                "\"animation\":9",
            ),
            (
                StateUpdate::Entity(EntityUpdate::Effect {
                    entity: mundane.clone(),
                    effect: 123,
                    source: Some(player),
                    frame_interval_ms: Some(50),
                }),
                "mundane.effect",
                "\"effect\":123",
            ),
            (
                StateUpdate::Entity(EntityUpdate::Damaged {
                    entity: mundane,
                    health_percent: 73,
                }),
                "mundane.damaged",
                "\"health_percent\":73",
            ),
        ];

        for (index, (update, expected_name, expected_json)) in cases.into_iter().enumerate() {
            let events = expand(
                42,
                identity,
                StateEvent {
                    sequence: u32::try_from(index + 1).unwrap(),
                    revision: 1,
                    tick_ms: 100,
                    update,
                },
                None,
                None,
                observed_at(),
            );
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), expected_name);
            assert!(
                serde_json::to_string(&events[0])
                    .unwrap()
                    .contains(expected_json)
            );
        }
    }

    #[test]
    fn message_types_have_distinct_public_event_names() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        for (sequence, kind, expected) in [
            (1, MessageKind::Say, "message.say"),
            (2, MessageKind::Shout, "message.shout"),
            (3, MessageKind::Whisper, "message.whisper"),
            (4, MessageKind::Guild, "message.guild"),
            (5, MessageKind::Group, "message.group"),
            (6, MessageKind::System, "message.system"),
            (7, MessageKind::World, "message.world"),
        ] {
            let events = expand(
                42,
                identity,
                StateEvent {
                    sequence,
                    revision: sequence,
                    tick_ms: sequence,
                    update: StateUpdate::Message(ClientMessage {
                        kind,
                        sender: None,
                        recipient: None,
                        text: "hello".into(),
                    }),
                },
                None,
                None,
                observed_at(),
            );
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), expected);
        }
    }

    #[test]
    fn empty_messages_do_not_become_public_events() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        for text in ["", "   "] {
            let events = expand(
                42,
                identity,
                StateEvent {
                    sequence: 1,
                    revision: 1,
                    tick_ms: 1,
                    update: StateUpdate::Message(ClientMessage {
                        kind: MessageKind::System,
                        sender: None,
                        recipient: None,
                        text: text.into(),
                    }),
                },
                None,
                None,
                observed_at(),
            );
            assert!(events.is_empty());
        }
    }

    #[test]
    fn spell_cast_retains_resolved_name_and_target_context() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: 8,
                revision: 9,
                tick_ms: 500,
                update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                    slot: 4,
                    arguments: ModelSpellCastArguments::Target {
                        id: Some(77),
                        x: 10,
                        y: 12,
                    },
                }),
            },
            Some("Ao Puinsein".into()),
            Some("Eidolon".into()),
            observed_at(),
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), "spell.cast");
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["data"]["name"], "Ao Puinsein");
        assert_eq!(event["data"]["arguments"]["type"], "target");
        assert_eq!(event["data"]["arguments"]["id"], 77);
        assert_eq!(event["data"]["arguments"]["name"], "Eidolon");
        assert_eq!(event["data"]["arguments"]["x"], 10);
        assert_eq!(event["data"]["arguments"]["y"], 12);
    }

    #[test]
    fn interrupted_spell_reports_replacement_as_the_cancellation_source() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: 8,
                revision: 9,
                tick_ms: 500,
                update: StateUpdate::Ability(AbilityUpdate::SpellCancelled {
                    slot: 4,
                    source: ModelSpellCancellationSource::Replaced,
                }),
            },
            Some("Inner Fire".into()),
            None,
            observed_at(),
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), "spell.cancelled");
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["data"]["name"], "Inner Fire");
        assert_eq!(event["data"]["source"], "replaced");
    }
}
