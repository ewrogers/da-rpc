mod collection;
mod interaction;
mod object;
mod status;

use super::*;

pub(crate) fn expand(
    pid: u32,
    identity: ClientIdentity,
    event: StateEvent,
    ability_name: Option<String>,
    target_name: Option<String>,
    observed_at_utc: DateTime<Utc>,
) -> Vec<ClientEvent> {
    let observation = EventObservation::new(pid, identity, &event);
    let mut events = Vec::with_capacity(9);
    match event.update {
        StateUpdate::CharacterProfile(update) => {
            events.push(ClientEvent::CharacterProfileChanged(
                CharacterProfileChanged {
                    observation,
                    previous: update
                        .previous
                        .as_ref()
                        .map(crate::state::PlayerIdentity::from),
                    current: crate::state::PlayerIdentity::from(&update.current),
                },
            ));
            events
        }
        StateUpdate::Command(command) => {
            events.push(ClientEvent::ClientCommand(ClientCommand {
                observation,
                command: command.command,
                args: command.args,
            }));
            events
        }
        StateUpdate::Lifecycle(update) => {
            let payload = ClientLifecycleChanged {
                observation,
                previous: update.previous.into(),
                current: update.current.into(),
            };
            match update.current {
                darpc_model::ClientLifecycle::InGame => {
                    events.push(ClientEvent::ClientLoggedIn(payload));
                }
                darpc_model::ClientLifecycle::Disconnected => {
                    events.push(ClientEvent::ClientDisconnected(payload));
                }
                _ => {}
            }
            events
        }
        StateUpdate::Audio(update) => {
            events.push(match update {
                darpc_model::AudioUpdate::SoundPlayed { effect } => {
                    ClientEvent::SoundPlayed(SoundPlayed {
                        observation,
                        effect,
                    })
                }
                darpc_model::AudioUpdate::MusicStarted { track } => {
                    ClientEvent::MusicStarted(MusicStarted { observation, track })
                }
                darpc_model::AudioUpdate::MusicStopped => {
                    ClientEvent::MusicStopped(MusicStopped { observation })
                }
            });
            events
        }
        StateUpdate::Player(update) => {
            events.push(ClientEvent::PlayerInspected(PlayerInspected::from_model(
                observation,
                update,
            )));
            events
        }
        StateUpdate::Status(update) => status::expand(observation, update),
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
            events
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
                    reason,
                } => ClientEvent::WalkingStopped(WalkingStopped {
                    observation,
                    current: current.into(),
                    destination: destination.map(Into::into),
                    reached_destination,
                    reason: reason.into(),
                }),
                MovementUpdate::Obstructed {
                    map_id,
                    current,
                    attempted,
                    direction,
                    destination,
                    mode,
                } => ClientEvent::WalkingObstructed(WalkingObstructed {
                    observation,
                    map_id,
                    current: current.into(),
                    attempted: attempted.into(),
                    direction: direction.into(),
                    destination: destination.map(Into::into),
                    mode: match mode {
                        darpc_model::WalkMode::Direct => WalkingMode::Direct,
                        darpc_model::WalkMode::NativeRoute => WalkingMode::NativeRoute,
                        darpc_model::WalkMode::ExactRoute => WalkingMode::ExactRoute,
                        darpc_model::WalkMode::Pursuit => WalkingMode::Pursuit,
                    },
                }),
            });
            events
        }
        StateUpdate::PlannedRoute(route) => {
            events.push(ClientEvent::WalkingRouteChanged(WalkingRouteChanged {
                observation,
                generation: route.generation,
                tiles: route.tiles.into_iter().map(Into::into).collect(),
            }));
            events
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
            events
        }
        update @ (StateUpdate::Inventory(_)
        | StateUpdate::Spellbook(_)
        | StateUpdate::Skillbook(_)) => collection::expand(observation, update),
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
            events
        }
        StateUpdate::Action(update) => {
            events.push(action::expand_action(observation, update));
            events
        }
        StateUpdate::Entity(update) => {
            if let Some(event) = entity::expand_entity(observation, update) {
                events.push(event);
            }
            events
        }
        StateUpdate::Object(update) => {
            if let Some(event) = object::expand(observation, update) {
                events.push(event);
            }
            events
        }
        StateUpdate::Message(message) => {
            if message.text.trim().is_empty() {
                events
            } else {
                events.push(ClientEvent::Message(Message::new(
                    event.sequence,
                    event.tick_ms,
                    observed_at_utc,
                    message,
                )));
                events
            }
        }
        update @ (StateUpdate::Dialog(_)
        | StateUpdate::FieldMap(_)
        | StateUpdate::MessageDialogs(_)
        | StateUpdate::Group(_)
        | StateUpdate::Exchange(_)
        | StateUpdate::Legend(_)) => interaction::expand(observation, update),
    }
}
