use super::*;

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
        profile: None,
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
        profile: None,
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
