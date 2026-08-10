use super::*;

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
pub(crate) struct ActionRestrictionChanged {
    pub(super) observation: EventObservation,
    pub(super) is_action_restricted: bool,
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

pub(super) fn expand(
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
        StateUpdate::Command(command) => {
            events.push(ClientEvent::ClientCommand(ClientCommand {
                observation,
                command: command.command,
                args: command.args,
            }));
            return events;
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
            return events;
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
            return events;
        }
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
            events.push(action::expand_action(observation, update));
            return events;
        }
        StateUpdate::Entity(update) => {
            if let Some(event) = entity::expand_entity(observation, update) {
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
        StateUpdate::Dialog(update) => {
            events.push(match update {
                darpc_model::DialogUpdate::Opened(dialog) => {
                    ClientEvent::DialogOpened(DialogOpened::new(observation, dialog))
                }
                darpc_model::DialogUpdate::Changed(dialog) => {
                    ClientEvent::DialogChanged(DialogChanged::new(observation, dialog))
                }
                darpc_model::DialogUpdate::Submitted {
                    state,
                    previous_revision,
                    submission,
                } => ClientEvent::DialogSubmitted(DialogSubmitted::new(
                    observation,
                    previous_revision,
                    state,
                    submission,
                )),
                darpc_model::DialogUpdate::Closed { previous, reason } => {
                    ClientEvent::DialogClosed(DialogClosed::new(observation, previous, reason))
                }
            });
            return events;
        }
        StateUpdate::Group(update) => {
            events.push(match update {
                darpc_model::GroupUpdate::SettingsChanged { state } => {
                    ClientEvent::GroupSettingsChanged(GroupSettingsChanged::new(observation, state))
                }
                darpc_model::GroupUpdate::InvitationSent { target } => {
                    ClientEvent::GroupInvitationSent(GroupInvitationSent::new(observation, target))
                }
                darpc_model::GroupUpdate::InvitationReceived { invitation, state } => {
                    ClientEvent::GroupInvitationReceived(GroupInvitationReceived::new(
                        observation,
                        invitation,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::InvitationClosed {
                    invitation,
                    reason,
                    state,
                } => ClientEvent::GroupInvitationClosed(GroupInvitationClosed::new(
                    observation,
                    invitation,
                    reason,
                    state,
                )),
                darpc_model::GroupUpdate::Joined { state } => {
                    ClientEvent::GroupJoined(GroupJoined::new(observation, state))
                }
                darpc_model::GroupUpdate::MemberJoined { member, state } => {
                    ClientEvent::GroupMemberJoined(GroupMemberChanged::new(
                        observation,
                        member,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::MemberLeft { member, state } => {
                    ClientEvent::GroupMemberLeft(GroupMemberChanged::new(
                        observation,
                        member,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::Disbanded { state } => {
                    ClientEvent::GroupDisbanded(GroupDisbanded::new(observation, state))
                }
            });
            return events;
        }
        StateUpdate::Exchange(update) => {
            events.push(match update {
                darpc_model::ExchangeUpdate::Opened(state) => {
                    ClientEvent::ExchangeOpened(ExchangeOpened::new(observation, state))
                }
                darpc_model::ExchangeUpdate::ItemAdded { state, party, item } => {
                    ClientEvent::ExchangeItemAdded(ExchangeItemAdded::new(
                        observation,
                        state,
                        party,
                        item,
                    ))
                }
                darpc_model::ExchangeUpdate::GoldChanged { state, party, gold } => {
                    ClientEvent::ExchangeGoldChanged(ExchangeGoldChanged::new(
                        observation,
                        state,
                        party,
                        gold,
                    ))
                }
                darpc_model::ExchangeUpdate::Accepted {
                    state,
                    party,
                    message,
                } => ClientEvent::ExchangeAccepted(ExchangeAccepted::new(
                    observation,
                    state,
                    party,
                    message,
                )),
                darpc_model::ExchangeUpdate::Completed { state, message } => {
                    ClientEvent::ExchangeCompleted(ExchangeCompleted::new(
                        observation,
                        state,
                        message,
                    ))
                }
                darpc_model::ExchangeUpdate::Cancelled { state, message } => {
                    ClientEvent::ExchangeCancelled(ExchangeCancelled::new(
                        observation,
                        state,
                        message,
                    ))
                }
            });
            return events;
        }
        StateUpdate::Legend(update) => {
            events.push(match update {
                darpc_model::LegendUpdate::MarkAdded { mark } => {
                    ClientEvent::LegendMarkAdded(LegendMarkAdded {
                        observation,
                        mark: mark.into(),
                    })
                }
                darpc_model::LegendUpdate::MarkChanged { previous, current } => {
                    ClientEvent::LegendMarkChanged(LegendMarkChanged {
                        observation,
                        previous: previous.into(),
                        current: current.into(),
                    })
                }
                darpc_model::LegendUpdate::MarkRemoved { mark } => {
                    ClientEvent::LegendMarkRemoved(LegendMarkRemoved {
                        observation,
                        mark: mark.into(),
                    })
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
