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
                    cooldown_ms: None,
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
                    cooldown_ms: None,
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
                    cooldown_ms: None,
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
fn moves_into_empty_slots_emit_changed_events_for_every_slotted_collection() {
    let item = ModelInventoryItem {
        slot: 1,
        sprite: 21,
        dye_color: 2,
        name: Some("Hy-Brasyl Gauntlet".into()),
        quantity: 1,
        can_stack: false,
        durability: 900,
        max_durability: 1_000,
    };
    let spell = ModelSpell {
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
            cooldown_ms: None,
            remaining_ms: None,
        },
    };
    let skill = ModelSkill {
        slot: 3,
        icon: 91,
        name: Some("Assail".into()),
        level: 99,
        max_level: 100,
        cooldown: CooldownStatus {
            active: false,
            cooldown_ms: None,
            remaining_ms: None,
        },
    };
    let updates = [
        StateUpdate::Inventory(SlotUpdate {
            batch_index: 0,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 1,
            before: Some(item.clone()),
            after: None,
        }),
        StateUpdate::Inventory(SlotUpdate {
            batch_index: 1,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 4,
            before: None,
            after: Some(ModelInventoryItem { slot: 4, ..item }),
        }),
        StateUpdate::Spellbook(SlotUpdate {
            batch_index: 0,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 2,
            before: Some(spell.clone()),
            after: None,
        }),
        StateUpdate::Spellbook(SlotUpdate {
            batch_index: 1,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 5,
            before: None,
            after: Some(ModelSpell { slot: 5, ..spell }),
        }),
        StateUpdate::Skillbook(SlotUpdate {
            batch_index: 0,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 3,
            before: Some(skill.clone()),
            after: None,
        }),
        StateUpdate::Skillbook(SlotUpdate {
            batch_index: 1,
            batch_count: 2,
            change: CollectionChange::Changed,
            slot: 6,
            before: None,
            after: Some(ModelSkill { slot: 6, ..skill }),
        }),
    ];

    let names = updates
        .into_iter()
        .enumerate()
        .flat_map(|(index, update)| {
            expand_collection_update(u32::try_from(index + 1).unwrap(), update)
        })
        .map(|event| event.name())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "item.changed",
            "item.changed",
            "spell.changed",
            "spell.changed",
            "skill.changed",
            "skill.changed",
        ]
    );
}

#[test]
fn cooldown_only_updates_emit_specialized_events_instead_of_changed() {
    let skill = ModelSkill {
        slot: 3,
        icon: 91,
        name: Some("Assail".into()),
        level: 99,
        max_level: 100,
        cooldown: CooldownStatus {
            active: false,
            cooldown_ms: None,
            remaining_ms: None,
        },
    };
    let spell = ModelSpell {
        slot: 4,
        icon: 82,
        name: Some("beag srad".into()),
        level: 10,
        max_level: 100,
        lines: 0,
        target_type: SpellTargetType::Target,
        prompt: None,
        cooldown: CooldownStatus {
            active: false,
            cooldown_ms: None,
            remaining_ms: None,
        },
    };

    let cases = [
        (
            StateUpdate::Skillbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: skill.slot,
                before: Some(skill.clone()),
                after: Some(ModelSkill {
                    cooldown: CooldownStatus {
                        active: true,
                        cooldown_ms: Some(1_000),
                        remaining_ms: Some(750),
                    },
                    ..skill
                }),
            }),
            "skill.cooldown",
            Some(1_000),
            Some(750),
        ),
        (
            StateUpdate::Spellbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: spell.slot,
                before: Some(spell.clone()),
                after: Some(ModelSpell {
                    cooldown: CooldownStatus {
                        active: true,
                        cooldown_ms: None,
                        remaining_ms: None,
                    },
                    ..spell
                }),
            }),
            "spell.cooldown",
            None,
            None,
        ),
    ];

    for (sequence, (update, expected_name, cooldown_ms, remaining_ms)) in cases
        .into_iter()
        .enumerate()
        .map(|(index, case)| (u32::try_from(index + 1).unwrap(), case))
    {
        let events = expand_collection_update(sequence, update);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let json = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(
            json["data"]
                .get("cooldown_ms")
                .and_then(|value| value.as_u64()),
            cooldown_ms
        );
        assert_eq!(
            json["data"]
                .get("remaining_ms")
                .and_then(|value| value.as_u64()),
            remaining_ms
        );
    }
}

#[test]
fn cooldown_completion_emits_ready_instead_of_changed() {
    let skill = ModelSkill {
        slot: 3,
        icon: 91,
        name: Some("Assail".into()),
        level: 99,
        max_level: 100,
        cooldown: CooldownStatus {
            active: true,
            cooldown_ms: Some(1_000),
            remaining_ms: Some(1),
        },
    };
    let spell = ModelSpell {
        slot: 4,
        icon: 82,
        name: Some("beag srad".into()),
        level: 10,
        max_level: 100,
        lines: 0,
        target_type: SpellTargetType::Target,
        prompt: None,
        cooldown: CooldownStatus {
            active: true,
            cooldown_ms: None,
            remaining_ms: None,
        },
    };
    let ready = CooldownStatus {
        active: false,
        cooldown_ms: None,
        remaining_ms: None,
    };
    let updates = [
        (
            StateUpdate::Skillbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: skill.slot,
                before: Some(skill.clone()),
                after: Some(ModelSkill {
                    cooldown: ready,
                    ..skill
                }),
            }),
            "skill.ready",
        ),
        (
            StateUpdate::Spellbook(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: spell.slot,
                before: Some(spell.clone()),
                after: Some(ModelSpell {
                    cooldown: ready,
                    ..spell
                }),
            }),
            "spell.ready",
        ),
    ];

    for (sequence, (update, expected_name)) in updates
        .into_iter()
        .enumerate()
        .map(|(index, case)| (u32::try_from(index + 1).unwrap(), case))
    {
        let events = expand_collection_update(sequence, update);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
    }
}

#[test]
fn metadata_and_cooldown_changes_emit_both_semantic_events() {
    let before = ModelSkill {
        slot: 3,
        icon: 91,
        name: Some("Assail".into()),
        level: 99,
        max_level: 100,
        cooldown: CooldownStatus {
            active: false,
            cooldown_ms: None,
            remaining_ms: None,
        },
    };
    let events = expand_collection_update(
        1,
        StateUpdate::Skillbook(SlotUpdate {
            batch_index: 0,
            batch_count: 1,
            change: CollectionChange::Changed,
            slot: before.slot,
            before: Some(before.clone()),
            after: Some(ModelSkill {
                level: 100,
                cooldown: CooldownStatus {
                    active: true,
                    cooldown_ms: Some(1_000),
                    remaining_ms: Some(750),
                },
                ..before
            }),
        }),
    );
    assert_eq!(
        events.iter().map(ClientEvent::name).collect::<Vec<_>>(),
        ["skill.changed", "skill.cooldown"]
    );
}

fn expand_collection_update(sequence: u32, update: StateUpdate) -> Vec<ClientEvent> {
    expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms: 500,
            update,
        },
        None,
        None,
        observed_at(),
    )
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
        if expected.starts_with("item.") {
            let event = serde_json::to_value(&events[0]).unwrap();
            assert_eq!(event["data"]["object"]["dye_color"], 5);
        }
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
        is_hidden: false,
        visual: None,
        profile: None,
    };
    let monster = ModelWorldObject::Creature {
        id: 2,
        kind: CreatureKind::Monster,
        is_solid: false,
        sprite: Some(7),
        name: None,
        x: 11,
        y: 20,
        direction: Direction::South,
    };
    let npc = ModelWorldObject::Creature {
        id: 3,
        kind: CreatureKind::Npc,
        is_solid: true,
        sprite: Some(8),
        name: Some("Maria".into()),
        x: 12,
        y: 20,
        direction: Direction::North,
    };
    let item = ModelWorldObject::Item {
        id: 4,
        sprite: 327,
        dye_color: 5,
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
fn same_name_player_relog_uses_a_replacement_event() {
    use darpc_model::{Direction, WorldObject as ModelWorldObject};

    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    let player = |id, x| ModelWorldObject::Player {
        id,
        name: Some("Monitor".into()),
        x,
        y: 20,
        direction: Direction::East,
        is_hidden: false,
        visual: None,
        profile: None,
    };
    let current = player(3, 12);
    let events = expand(
        42,
        identity,
        StateEvent {
            sequence: 1,
            revision: 1,
            tick_ms: 1,
            update: StateUpdate::Object(ObjectUpdate::Appeared(current)),
        },
        None,
        None,
        observed_at(),
    );
    let events = replace_player_appearance(events, &[player(1, 10), player(2, 11)]);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "player.replaced");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "player_replaced");
    assert_eq!(event["data"]["previous"][0]["id"], 1);
    assert_eq!(event["data"]["previous"][1]["id"], 2);
    assert_eq!(event["data"]["current"]["id"], 3);
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
        is_hidden: false,
        visual: None,
        profile: None,
    };
    let mundane = ModelWorldObject::Creature {
        id: 2,
        kind: CreatureKind::Npc,
        is_solid: true,
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
