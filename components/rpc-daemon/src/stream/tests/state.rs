use super::*;

#[test]
fn snapshot_recapture_emits_appearance_and_hidden_changes() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    let previous = character_snapshot(
        Some(ModelCharacterAppearance {
            gender: Gender::Male,
            hair_style: 7,
            hair_color: 2,
            body_sprite: 1,
        }),
        false,
        8,
        9,
    );
    let current = character_snapshot(
        Some(ModelCharacterAppearance {
            gender: Gender::Female,
            hair_style: 12,
            hair_color: 4,
            body_sprite: 2,
        }),
        true,
        9,
        10,
    );

    let events = snapshot_character_changes(42, identity, &previous, &current);
    assert_eq!(
        events.iter().map(ClientEvent::name).collect::<Vec<_>>(),
        ["character.appearance_changed", "character.hidden_changed"]
    );
    let appearance = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(appearance["data"]["previous"]["hair_style"], 7);
    assert_eq!(appearance["data"]["current"]["hair_style"], 12);
    let hidden = serde_json::to_value(&events[1]).unwrap();
    assert_eq!(hidden["data"]["previous"], false);
    assert_eq!(hidden["data"]["current"], true);
}

fn character_snapshot(
    appearance: Option<ModelCharacterAppearance>,
    is_hidden: bool,
    revision: u32,
    event_sequence: u32,
) -> ClientSnapshot {
    ClientSnapshot {
        revision,
        event_sequence,
        captured_tick_ms: 1,
        updated_tick_ms: 2,
        capture_duration_us: 3,
        world_generation: 1,
        lifecycle: ClientLifecycle::InGame,
        character: Some(CharacterSnapshot {
            id: Some(7),
            name: Some("Monitor".into()),
            identity: None,
            appearance,
            class: CharacterClass::Rogue,
            is_hidden,
            is_action_restricted: false,
            is_blinded: false,
            is_casting: false,
            is_walking: false,
            gold: 0,
            weight: 0,
            max_weight: 0,
            progression: CharacterProgression {
                level: 99,
                ability_level: 0,
                experience: 0,
                ability_points: None,
                experience_to_next_level: None,
                ability_to_next_level: None,
            },
            stats: CharacterStats {
                stat_points: 3,
                strength: 3,
                intelligence: 3,
                wisdom: 3,
                constitution: 3,
                dexterity: 3,
            },
            vitals: CharacterVitals {
                health: 1,
                max_health: 1,
                mana: 1,
                max_mana: 1,
            },
            modifiers: None,
            location: None,
            inventory: None,
            equipment: None,
            spellbook: None,
            skillbook: None,
            effects: None,
        }),
        objects: None,
        dialog: None,
        active_field_map: None,
        message_dialogs: Default::default(),
        group: None,
        exchange: None,
        legend: None,
        planned_route: None,
    }
}

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
fn look_results_have_a_stable_correlated_public_event_shape() {
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
            update: StateUpdate::Look(ModelLookResult {
                command_id: 7,
                target: ModelLookTarget::Tile { x: 40, y: 19 },
                text: "Light Belt\rLight Belt\rfior sal".into(),
            }),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "look.result");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "look_result");
    assert_eq!(event["data"]["command_id"], 7);
    assert_eq!(event["data"]["target"]["kind"], "tile");
    assert_eq!(event["data"]["target"]["x"], 40);
    assert_eq!(event["data"]["target"]["y"], 19);
    assert_eq!(event["data"]["text"], "Light Belt\rLight Belt\rfior sal");
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
            update: StateUpdate::Action(ActionUpdate::Resync { resync_id: 17 }),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "client.resync");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "client_resync");
    assert_eq!(event["data"]["resync_id"], 17);
    assert_eq!(event["data"]["observation"]["event_sequence"], 10);
}

#[test]
fn completed_client_resync_has_a_stable_public_event_shape() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 11,
            revision: 12,
            tick_ms: 13,
            update: StateUpdate::Action(ActionUpdate::ResyncCompleted { resync_id: 17 }),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "client.resync_completed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "client_resync_completed");
    assert_eq!(event["data"]["resync_id"], 17);
    assert_eq!(event["data"]["observation"]["event_sequence"], 11);
}

#[test]
fn legacy_timed_out_client_resync_maps_to_completed() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 12,
            revision: 13,
            tick_ms: 1_013,
            update: StateUpdate::Action(ActionUpdate::ResyncTimedOut { resync_id: 17 }),
        },
        None,
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "client.resync_completed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["type"], "client_resync_completed");
    assert_eq!(event["data"]["resync_id"], 17);
    assert_eq!(event["data"]["observation"]["event_sequence"], 12);
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
                    stat_points: 3,
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
    let events = expand(
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
    );
    let stats = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(stats["data"]["stat_points"], 3);
    let names = events.iter().map(ClientEvent::name).collect::<Vec<_>>();
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
                reason: darpc_model::MovementStopReason::Completed,
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
    assert_eq!(stopped["data"]["reason"], "completed");
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
