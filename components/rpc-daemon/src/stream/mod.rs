use crate::{
    commands::LegendMark,
    dialog::{DialogChanged, DialogClosed, DialogOpened, DialogSubmitted},
    exchange::{
        ExchangeAccepted, ExchangeCancelled, ExchangeCompleted, ExchangeGoldChanged,
        ExchangeItemAdded, ExchangeOpened,
    },
    group::{
        GroupDisbanded, GroupInvitationClosed, GroupInvitationReceived, GroupInvitationSent,
        GroupJoined, GroupMemberChanged, GroupSettingsChanged,
    },
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
    CollectionChange, CreatureKind, Effect, MovementUpdate, ObjectUpdate, SequenceNumber,
    StateEvent, StateUpdate, TilePosition as ModelTilePosition,
};
use serde::Serialize;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use utoipa::ToSchema;

mod action;
use action::*;
mod entity;
use entity::*;
mod ability;
mod feedback;
mod state;

use ability::*;
use feedback::*;
pub(crate) use feedback::{SpellFeedback, SpellFeedbackTrackers};
pub(crate) use state::*;

pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 4_096;

#[derive(Clone, Debug)]
pub(crate) enum PublishedEvent {
    Internal {
        recipients: Arc<[ClientIdentity]>,
        message: Message,
    },
    #[cfg_attr(not(windows), allow(dead_code))]
    State {
        pid: u32,
        identity: ClientIdentity,
        event: Box<StateEvent>,
        replaced_players: Vec<darpc_model::WorldObject>,
        ability_name: Option<String>,
        target_name: Option<String>,
        feedback: Option<Box<SpellFeedback>>,
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
    ClientCommand(ClientCommand),
    ClientResync(ClientResync),
    ClientLoggedIn(ClientLifecycleChanged),
    ClientDisconnected(ClientLifecycleChanged),
    SoundPlayed(SoundPlayed),
    MusicStarted(MusicStarted),
    MusicStopped(MusicStopped),
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
    WalkingObstructed(WalkingObstructed),
    WalkingRouteChanged(WalkingRouteChanged),
    MapExclusionsChanged(MapExclusionsChanged),
    ActionRestrictionChanged(ActionRestrictionChanged),
    CharacterProfileChanged(CharacterProfileChanged),
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
    SkillCooldown(CooldownStarted),
    SkillReady(AbilityReady),
    SpellCooldown(CooldownStarted),
    SpellReady(AbilityReady),
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
    PlayerReplaced(PlayerReplaced),
    PlayerInspected(PlayerInspected),
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
    DialogOpened(DialogOpened),
    DialogChanged(DialogChanged),
    DialogSubmitted(DialogSubmitted),
    DialogClosed(DialogClosed),
    GroupInvitationSent(GroupInvitationSent),
    GroupInvitationReceived(GroupInvitationReceived),
    GroupInvitationClosed(GroupInvitationClosed),
    GroupSettingsChanged(GroupSettingsChanged),
    GroupJoined(GroupJoined),
    GroupMemberJoined(GroupMemberChanged),
    GroupMemberLeft(GroupMemberChanged),
    GroupDisbanded(GroupDisbanded),
    ExchangeOpened(ExchangeOpened),
    ExchangeItemAdded(ExchangeItemAdded),
    ExchangeGoldChanged(ExchangeGoldChanged),
    ExchangeAccepted(ExchangeAccepted),
    ExchangeCompleted(ExchangeCompleted),
    ExchangeCancelled(ExchangeCancelled),
    LegendMarkAdded(LegendMarkAdded),
    LegendMarkChanged(LegendMarkChanged),
    LegendMarkRemoved(LegendMarkRemoved),
    StreamResyncRequired(StreamResyncRequired),
    StreamClosed(StreamClosed),
}

impl ClientEvent {
    const fn name(&self) -> &'static str {
        match self {
            Self::StreamReady(_) => "stream.ready",
            Self::ClientCommand(_) => "client.command",
            Self::ClientResync(_) => "client.resync",
            Self::ClientLoggedIn(_) => "client.logged_in",
            Self::ClientDisconnected(_) => "client.disconnected",
            Self::SoundPlayed(_) => "sound.played",
            Self::MusicStarted(_) => "music.started",
            Self::MusicStopped(_) => "music.stopped",
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
            Self::WalkingObstructed(_) => "walking.obstructed",
            Self::WalkingRouteChanged(_) => "walking.route_changed",
            Self::MapExclusionsChanged(_) => "map.exclusions_changed",
            Self::ActionRestrictionChanged(_) => "action_restriction.changed",
            Self::CharacterProfileChanged(_) => "character.profile_changed",
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
            Self::SkillCooldown(_) => "skill.cooldown",
            Self::SkillReady(_) => "skill.ready",
            Self::SpellCooldown(_) => "spell.cooldown",
            Self::SpellReady(_) => "spell.ready",
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
            Self::PlayerReplaced(_) => "player.replaced",
            Self::PlayerInspected(_) => "player.inspected",
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
            Self::DialogOpened(_) => "dialog.opened",
            Self::DialogChanged(_) => "dialog.changed",
            Self::DialogSubmitted(_) => "dialog.submitted",
            Self::DialogClosed(_) => "dialog.closed",
            Self::GroupInvitationSent(_) => "group.invitation_sent",
            Self::GroupInvitationReceived(_) => "group.invitation_received",
            Self::GroupInvitationClosed(_) => "group.invitation_closed",
            Self::GroupSettingsChanged(_) => "group.settings_changed",
            Self::GroupJoined(_) => "group.joined",
            Self::GroupMemberJoined(_) => "group.member_joined",
            Self::GroupMemberLeft(_) => "group.member_left",
            Self::GroupDisbanded(_) => "group.disbanded",
            Self::ExchangeOpened(_) => "exchange.opened",
            Self::ExchangeItemAdded(_) => "exchange.item_added",
            Self::ExchangeGoldChanged(_) => "exchange.gold_changed",
            Self::ExchangeAccepted(_) => "exchange.accepted",
            Self::ExchangeCompleted(_) => "exchange.completed",
            Self::ExchangeCancelled(_) => "exchange.cancelled",
            Self::LegendMarkAdded(_) => "legend.mark_added",
            Self::LegendMarkChanged(_) => "legend.mark_changed",
            Self::LegendMarkRemoved(_) => "legend.mark_removed",
            Self::StreamResyncRequired(_) => "stream.resync_required",
            Self::StreamClosed(_) => "stream.closed",
        }
    }

    fn sequence(&self) -> u32 {
        match self {
            Self::StreamReady(value) => value.event_sequence,
            Self::ClientCommand(value) => value.observation.event_sequence,
            Self::ClientResync(value) => value.observation.event_sequence,
            Self::ClientLoggedIn(value) | Self::ClientDisconnected(value) => {
                value.observation.event_sequence
            }
            Self::SoundPlayed(value) => value.observation.event_sequence,
            Self::MusicStarted(value) => value.observation.event_sequence,
            Self::MusicStopped(value) => value.observation.event_sequence,
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
            Self::WalkingObstructed(value) => value.observation.event_sequence,
            Self::WalkingRouteChanged(value) => value.observation.event_sequence,
            Self::MapExclusionsChanged(value) => value.observation.event_sequence,
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
            Self::SkillCooldown(value) | Self::SpellCooldown(value) => {
                value.observation.event_sequence
            }
            Self::SkillReady(value) | Self::SpellReady(value) => value.observation.event_sequence,
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
            Self::CharacterProfileChanged(value) => value.observation.event_sequence,
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
            Self::PlayerReplaced(value) => value.observation.event_sequence,
            Self::PlayerInspected(value) => value.observation.event_sequence,
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
            Self::DialogOpened(value) => value.observation.event_sequence,
            Self::DialogChanged(value) => value.observation.event_sequence,
            Self::DialogSubmitted(value) => value.observation.event_sequence,
            Self::DialogClosed(value) => value.observation.event_sequence,
            Self::GroupInvitationSent(value) => value.observation.event_sequence,
            Self::GroupInvitationReceived(value) => value.observation.event_sequence,
            Self::GroupInvitationClosed(value) => value.observation.event_sequence,
            Self::GroupSettingsChanged(value) => value.observation.event_sequence,
            Self::GroupJoined(value) => value.observation.event_sequence,
            Self::GroupMemberJoined(value) | Self::GroupMemberLeft(value) => {
                value.observation.event_sequence
            }
            Self::GroupDisbanded(value) => value.observation.event_sequence,
            Self::ExchangeOpened(value) => value.observation.event_sequence,
            Self::ExchangeItemAdded(value) => value.observation.event_sequence,
            Self::ExchangeGoldChanged(value) => value.observation.event_sequence,
            Self::ExchangeAccepted(value) => value.observation.event_sequence,
            Self::ExchangeCompleted(value) => value.observation.event_sequence,
            Self::ExchangeCancelled(value) => value.observation.event_sequence,
            Self::LegendMarkAdded(value) => value.observation.event_sequence,
            Self::LegendMarkChanged(value) => value.observation.event_sequence,
            Self::LegendMarkRemoved(value) => value.observation.event_sequence,
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

    fn into_internal_sse(self) -> Event {
        debug_assert!(matches!(&self, Self::Message(message) if message.is_internal()));
        let event_name = self.name();
        let event_id = format!("internal-{}", self.sequence());
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

fn replace_player_appearance(
    events: Vec<ClientEvent>,
    replaced_players: &[darpc_model::WorldObject],
) -> Vec<ClientEvent> {
    if replaced_players.is_empty() {
        return events;
    }
    events
        .into_iter()
        .map(|event| match event {
            ClientEvent::PlayerAppeared(changed) => ClientEvent::PlayerReplaced(PlayerReplaced {
                observation: changed.observation,
                previous: replaced_players.iter().map(WorldObject::from).collect(),
                current: changed.object,
            }),
            other => other,
        })
        .collect()
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
                Ok(PublishedEvent::Internal {
                    recipients,
                    message,
                }) if recipients.contains(&identity) => {
                    yield Ok(ClientEvent::Message(message).into_internal_sse());
                }
                Ok(PublishedEvent::State {
                    pid: event_pid,
                    identity: event_identity,
                    event,
                    replaced_players,
                    ability_name,
                    target_name,
                    feedback,
                    observed_at_utc,
                }) if event_pid == pid && event_identity == identity => {
                    if !SequenceNumber::new(event.sequence)
                        .is_after(SequenceNumber::new(last_sequence))
                    {
                        continue;
                    }
                    if event.sequence != SequenceNumber::new(last_sequence).next().get() {
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
                    api_events = replace_player_appearance(api_events, &replaced_players);
                    if let Some(feedback) = feedback {
                        api_events.push((*feedback).into_event(feedback_observation));
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

#[cfg(test)]
mod tests;
