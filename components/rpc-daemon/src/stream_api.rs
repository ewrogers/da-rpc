use crate::{
    registry::{ClientIdentity, hex},
    snapshot_api::Element,
};
use async_stream::stream;
use axum::response::{
    IntoResponse,
    sse::{Event, KeepAlive, Sse},
};
use darpc_model::{StateEvent, StateUpdate};
use serde::Serialize;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast;
use utoipa::ToSchema;

mod effects;

pub(crate) use effects::{EffectAdded, EffectChanged, EffectRemoved};

pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 4_096;

#[derive(Clone, Debug)]
pub(crate) enum PublishedEvent {
    #[cfg_attr(not(windows), allow(dead_code))]
    State {
        pid: u32,
        identity: ClientIdentity,
        event: StateEvent,
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
    ActionRestrictionChanged(ActionRestrictionChanged),
    EffectAdded(EffectAdded),
    EffectRemoved(EffectRemoved),
    EffectChanged(EffectChanged),
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
            Self::ActionRestrictionChanged(_) => "action_restriction.changed",
            Self::EffectAdded(_) => "effect_added",
            Self::EffectRemoved(_) => "effect_removed",
            Self::EffectChanged(_) => "effect_changed",
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
            Self::ActionRestrictionChanged(value) => value.observation.event_sequence,
            Self::EffectAdded(value) => value.observation.event_sequence,
            Self::EffectRemoved(value) => value.observation.event_sequence,
            Self::EffectChanged(value) => value.observation.event_sequence,
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
    fn new(pid: u32, identity: ClientIdentity, event: &StateEvent) -> Self {
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

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ActionRestrictionChanged {
    observation: EventObservation,
    is_action_restricted: bool,
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
                    for api_event in expand(pid, identity, event) {
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

fn expand(pid: u32, identity: ClientIdentity, event: StateEvent) -> Vec<ClientEvent> {
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
        CharacterStats, CoreStatus, CurrentVitals, Effect, EffectDuration, EffectUpdate,
        LocationUpdate, MapChange, StateUpdate, StatusUpdate,
    };

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
    fn sequence_ordering_handles_nonzero_wrap() {
        assert_eq!(next_nonzero(u32::MAX), 1);
        assert!(sequence_after(1, u32::MAX));
        assert!(!sequence_after(u32::MAX, 1));
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
    fn effect_updates_keep_the_requested_public_event_names() {
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
            (1, EffectUpdate::Added(effect), "effect_added"),
            (
                2,
                EffectUpdate::Changed(Effect {
                    duration: EffectDuration::Red,
                    ..effect
                }),
                "effect_changed",
            ),
            (3, EffectUpdate::Removed { icon: 300 }, "effect_removed"),
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
            );
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), expected);
        }
    }
}
