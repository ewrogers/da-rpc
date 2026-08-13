use super::*;

#[test]
fn client_commands_have_a_stable_public_event_shape() {
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
            tick_ms: 10,
            update: StateUpdate::Command(ModelClientCommand {
                command: "walk".into(),
                args: vec!["x".into(), "y".into()],
            }),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "client.command");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "client_command");
    assert_eq!(event["data"]["command"], "walk");
    assert_eq!(event["data"]["args"], serde_json::json!(["x", "y"]));
}

#[test]
fn client_resync_has_a_stable_public_event_shape() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 10,
            revision: 11,
            tick_ms: 12,
            update: StateUpdate::Action(ActionUpdate::Resync),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "client.resync");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "client_resync");
    assert_eq!(event["data"]["observation"]["event_sequence"], 10);
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
fn obstruction_updates_include_the_attempted_edge_and_route_mode() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 12,
            revision: 15,
            tick_ms: 503,
            update: StateUpdate::Movement(MovementUpdate::Obstructed {
                map_id: 3001,
                current: ModelTilePosition { x: 11, y: 22 },
                attempted: ModelTilePosition { x: 12, y: 22 },
                direction: darpc_model::Direction::East,
                destination: Some(ModelTilePosition { x: 30, y: 40 }),
                mode: darpc_model::WalkMode::ExactRoute,
            }),
        },
        None,
        None,
        observed_at(),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "walking.obstructed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["map_id"], 3001);
    assert_eq!(event["data"]["attempted"]["x"], 12);
    assert_eq!(event["data"]["direction"], "east");
    assert_eq!(event["data"]["mode"], "exact_route");
}

#[test]
fn planned_route_updates_expose_generation_and_absolute_tiles() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 12,
            revision: 15,
            tick_ms: 503,
            update: StateUpdate::PlannedRoute(PlannedRoute {
                generation: 9,
                tiles: vec![
                    ModelTilePosition { x: 2, y: 8 },
                    ModelTilePosition { x: 3, y: 8 },
                    ModelTilePosition { x: 3, y: 9 },
                ],
            }),
        },
        None,
        None,
        observed_at(),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "walking.route_changed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["generation"], 9);
    assert_eq!(event["data"]["tiles"][0]["x"], 2);
    assert_eq!(event["data"]["tiles"][2]["y"], 9);
}

#[test]
fn map_exclusion_updates_expose_resource_metadata() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 13,
            revision: 16,
            tick_ms: 504,
            update: StateUpdate::MapExclusions(darpc_model::MapExclusionsUpdate::Replaced {
                exclusions: darpc_model::MapExclusions {
                    map_id: 3001,
                    tiles: vec![
                        ModelTilePosition { x: 40, y: 50 },
                        ModelTilePosition { x: 41, y: 50 },
                    ],
                },
                map_count: 3,
            }),
        },
        None,
        None,
        observed_at(),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "map.exclusions_changed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["observation"]["revision"], 16);
    assert_eq!(event["data"]["operation"], "replaced");
    assert_eq!(event["data"]["map_id"], 3001);
    assert_eq!(event["data"]["tile_count"], 2);
    assert_eq!(event["data"]["map_count"], 3);
}

#[test]
fn sequence_ordering_handles_nonzero_wrap() {
    assert_eq!(SequenceNumber::new(u32::MAX).next().get(), 1);
    assert!(SequenceNumber::new(1).is_after(SequenceNumber::new(u32::MAX)));
    assert!(!SequenceNumber::new(u32::MAX).is_after(SequenceNumber::new(1)));
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
