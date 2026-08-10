use super::*;
use darpc_model::{
    AbilityUpdate, ActionUpdate, CharacterStats, ClientMessage, CollectionChange, CooldownStatus,
    CoreStatus, CurrentVitals, Effect, EffectDuration, EffectUpdate, EntityUpdate,
    ExchangeItem as ModelExchangeItem, ExchangeOffer as ModelExchangeOffer,
    ExchangeParty as ModelExchangeParty, ExchangeState as ModelExchangeState, ExchangeUpdate,
    InventoryItem as ModelInventoryItem, LegendIcon as ModelLegendIcon,
    LegendMark as ModelLegendMark, LegendUpdate, LocationUpdate, MapChange, MessageKind,
    MovementUpdate, Skill as ModelSkill, SlotUpdate, Spell as ModelSpell,
    SpellCancellationSource as ModelSpellCancellationSource,
    SpellCastArguments as ModelSpellCastArguments, SpellTargetType, StateUpdate, StatusUpdate,
    TilePosition as ModelTilePosition,
};

fn observed_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_775_000_000, 0).unwrap()
}

#[test]
fn expands_legend_diffs_with_previous_and_current_marks() {
    let previous = ModelLegendMark {
        text: "Found the grove".into(),
        tag: "Quest".into(),
        color: 3,
        icon: ModelLegendIcon::Aisling,
    };
    let current = ModelLegendMark {
        text: "Found the hidden grove".into(),
        tag: "Quest".into(),
        color: 7,
        icon: ModelLegendIcon::Wizard,
    };
    let updates = [
        (
            LegendUpdate::MarkAdded {
                mark: current.clone(),
            },
            "legend.mark_added",
        ),
        (
            LegendUpdate::MarkChanged {
                previous: previous.clone(),
                current: current.clone(),
            },
            "legend.mark_changed",
        ),
        (
            LegendUpdate::MarkRemoved {
                mark: previous.clone(),
            },
            "legend.mark_removed",
        ),
    ];

    for (sequence, (update, expected_name)) in updates.into_iter().enumerate() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: sequence as u32 + 1,
                revision: sequence as u32 + 1,
                tick_ms: 500,
                update: StateUpdate::Legend(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        if expected_name == "legend.mark_changed" {
            assert_eq!(event["data"]["previous"]["text"], "Found the grove");
            assert_eq!(event["data"]["current"]["text"], "Found the hidden grove");
            assert_eq!(event["data"]["current"]["icon"], "wizard");
        }
    }
}

fn exchange_state() -> ModelExchangeState {
    ModelExchangeState {
        id: 77,
        partner: "ZiLo".into(),
        local: ModelExchangeOffer::default(),
        other: ModelExchangeOffer::default(),
    }
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

#[test]
fn exchange_updates_use_stable_public_event_names_and_context() {
    let item = ModelExchangeItem {
        index: 0,
        sprite: 123,
        dye_color: 4,
        quantity: Some(2),
        name: "Red Potion".into(),
    };
    let updates = [
        (ExchangeUpdate::Opened(exchange_state()), "exchange.opened"),
        (
            ExchangeUpdate::ItemAdded {
                state: exchange_state(),
                party: ModelExchangeParty::Other,
                item,
            },
            "exchange.item_added",
        ),
        (
            ExchangeUpdate::GoldChanged {
                state: exchange_state(),
                party: ModelExchangeParty::Local,
                gold: 100,
            },
            "exchange.gold_changed",
        ),
        (
            ExchangeUpdate::Accepted {
                state: exchange_state(),
                party: ModelExchangeParty::Other,
                message: "accepted".into(),
            },
            "exchange.accepted",
        ),
        (
            ExchangeUpdate::Completed {
                state: exchange_state(),
                message: "complete".into(),
            },
            "exchange.completed",
        ),
        (
            ExchangeUpdate::Cancelled {
                state: exchange_state(),
                message: "cancelled".into(),
            },
            "exchange.cancelled",
        ),
    ];

    for (sequence, (update, expected_name)) in updates.into_iter().enumerate() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: sequence as u32,
                revision: sequence as u32,
                tick_ms: sequence as u32,
                update: StateUpdate::Exchange(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["data"]["exchange"]["id"], 77);
        assert_eq!(event["data"]["exchange"]["partner"], "ZiLo");
    }
}
