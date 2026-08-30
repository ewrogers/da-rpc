use super::*;

#[test]
fn snapshot_recapture_publishes_an_unchanged_stream_boundary() {
    let (state, mut registry) = state_and_registry();
    let mut receiver = state.published_events.subscribe();
    let mut snapshot = game_snapshot();
    snapshot.revision += 1;
    snapshot.event_sequence += 1;
    let identity = RegistryClientIdentity::from_hello(hello());

    commit_change(
        &state,
        &mut registry,
        ConnectionEvent::Snapshot {
            pid: 42,
            identity,
            snapshot: Box::new(snapshot),
        },
    );

    let published = receiver.try_recv().unwrap();
    let PublishedEvent::Snapshot {
        pid,
        identity: event_identity,
        current,
        ..
    } = published
    else {
        panic!("expected a committed snapshot publication");
    };
    assert_eq!(pid, 42);
    assert_eq!(event_identity, identity);
    let retained = state.snapshot().clients[0].game_snapshot.clone().unwrap();
    assert!(Arc::ptr_eq(&current, &retained));
}

#[test]
fn rejected_observations_make_rest_and_new_streams_unavailable() {
    let (state, mut registry) = state_and_registry();
    let identity = RegistryClientIdentity::from_hello(hello());
    let mut receiver = state.subscribe();
    let retained = state.snapshot().clients[0].game_snapshot.clone().unwrap();

    commit_change(
        &state,
        &mut registry,
        ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![StateEvent {
                sequence: retained.event_sequence.wrapping_add(2),
                revision: retained.revision.wrapping_add(1),
                tick_ms: 600,
                update: StateUpdate::Message(ModelClientMessage {
                    kind: ModelMessageKind::System,
                    sender: None,
                    recipient: None,
                    text: "rejected observation".into(),
                }),
            }],
        },
    );

    let unavailable = state.snapshot();
    assert!(Arc::ptr_eq(
        unavailable.clients[0].game_snapshot.as_ref().unwrap(),
        &retained
    ));
    assert!(unavailable.clients[0].snapshot_reason.is_some());
    assert!(matches!(
        receiver.try_recv(),
        Ok(PublishedEvent::ResyncRequired {
            pid: 42,
            identity: event_identity,
        }) if event_identity == identity
    ));
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let messages = json_with_state(state.clone(), "/clients/42/messages");
    assert_eq!(messages["messages"].as_array().unwrap().len(), 0);
    for path in [
        "/clients/42/status",
        "/clients/42/players/zilo",
        "/clients/42/events",
    ] {
        let response = response_with_state(state.clone(), path);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

#[test]
fn serves_health_and_client_resources() {
    assert_eq!(json("/health")["status"], "ok");

    let clients = json("/clients");
    assert_eq!(clients["clients"][0]["pid"], 42);
    assert_eq!(clients["clients"][0]["name"], "SiLo");
    assert_eq!(clients["clients"][0]["status"], "connected");
    assert_eq!(
        clients["clients"][0]["identity"]["created_time"],
        "134299999186432946"
    );
    assert_eq!(
        clients["clients"][0]["connection"]["protocol_version"],
        "1.9"
    );
    assert_eq!(
        clients["clients"][0]["connection"]["client_version"],
        "7.41"
    );
    assert!(
        clients["clients"][0]["connection"]
            .get("layout_id")
            .is_none()
    );

    let status = json("/clients/silo/status");
    assert_eq!(status["observation"]["pid"], 42);
    assert_eq!(status["observation"]["revision"], 3);
    assert_eq!(status["observation"]["event_sequence"], 2);
    assert_eq!(status["observation"]["updated_tick_ms"], 510);
    assert_eq!(status["lifecycle"], "in_game");
    assert_eq!(status["character"]["name"], "SiLo");
    assert_eq!(status["character"]["gender"], "male");
    assert_eq!(status["character"]["hair_style"], 17);
    assert_eq!(status["character"]["hair_color"], 6);
    assert_eq!(status["character"]["body_sprite"], 1);
    assert_eq!(status["character"]["is_hidden"], false);
    assert_eq!(status["character"]["is_action_restricted"], false);
    assert_eq!(status["character"]["is_blinded"], true);
    assert_eq!(status["character"]["is_walking"], false);
    assert!(status["character"]["movement_source"].is_null());
    assert_eq!(status["character"]["is_in_exchange"], false);
    assert!(status["character"].get("gender_id").is_none());
    assert!(status["character"].get("class_id").is_none());
    assert!(status["character"].get("inventory").is_none());
    assert!(status["character"].get("equipment").is_none());
    assert!(status["character"].get("spellbook").is_none());
    assert!(status["character"].get("skillbook").is_none());
    assert_eq!(status["character"]["progression"]["level"], 50);
    assert_eq!(status["character"]["stats"]["stat_points"], 3);
    assert_eq!(status["map"]["x"], 11);
    assert_eq!(status["planned_route"]["generation"], 17);
    assert_eq!(status["planned_route"]["source"]["kind"], "client");
    assert_eq!(status["planned_route"]["tiles"][1]["x"], 12);

    let inventory = json("/clients/silo/items");
    assert_eq!(inventory["observation"]["revision"], 3);
    assert_eq!(inventory["items"][0]["quantity"], 3);
    assert_eq!(inventory["items"][0]["can_stack"], true);
    assert_eq!(inventory["items"][0]["name"], "Dark Belt");
    assert_eq!(inventory["items"][0]["sprite"], 0x0123);

    let equipment = json("/clients/silo/equipment");
    assert_eq!(equipment["observation"]["revision"], 3);
    assert_eq!(equipment["items"][0]["slot"], "armor");

    let spellbook = json("/clients/silo/spells");
    assert_eq!(spellbook["observation"]["revision"], 3);
    assert_eq!(spellbook["spells"][0]["target_type"], "text_input");
    assert_eq!(spellbook["spells"][0]["prompt"], "Who?");
    assert!(spellbook["spells"][0].get("target_type_id").is_none());

    let skillbook = json("/clients/silo/skills");
    assert_eq!(skillbook["observation"]["revision"], 3);
    assert_eq!(skillbook["skills"][0]["max_level"], 100);

    let effects = json("/clients/silo/effects");
    assert_eq!(effects["observation"]["revision"], 3);
    assert_eq!(effects["effects"][0]["icon"], 300);
    assert_eq!(effects["effects"][0]["duration"], "white");

    let events = response("/clients/silo/events");
    assert_eq!(events.status(), StatusCode::OK);
    assert_eq!(
        events.headers()[axum::http::header::CONTENT_TYPE],
        "text/event-stream"
    );

    assert_eq!(
        response("/clients/silo/snapshot").status(),
        StatusCode::NOT_FOUND
    );

    let state = state();
    let mut registry = Registry::new();
    registry.apply(&ConnectionEvent::NotLoaded { pid: 7 });
    state.publish(registry.snapshot());
    assert_eq!(state.snapshot().clients[0].pid, 7);
}

#[test]
fn serves_normalized_message_history() {
    let (state, mut registry) = state_and_registry();
    let identity = RegistryClientIdentity::from_hello(hello());
    commit_change(
        &state,
        &mut registry,
        ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![StateEvent {
                sequence: 3,
                revision: 4,
                tick_ms: 520,
                update: StateUpdate::Message(ModelClientMessage {
                    kind: ModelMessageKind::Whisper,
                    sender: Some("Eidolon".into()),
                    recipient: Some("SiLo".into()),
                    text: "hello".into(),
                }),
            }],
        },
    );

    let response = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            router(state.clone())
                .oneshot(
                    Request::get("/clients/silo/messages")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response);
    assert!(body["messages"][0].get("observation").is_none());
    assert_eq!(body["messages"][0]["channel"], "whisper");
    assert_eq!(body["messages"][0]["tick_ms"], 520);
    assert!(
        DateTime::parse_from_rfc3339(body["messages"][0]["timestamp"].as_str().unwrap()).is_ok()
    );
    assert_eq!(body["messages"][0]["sender"], "Eidolon");
    assert_eq!(body["messages"][0]["recipient"], "SiLo");
    assert_eq!(body["messages"][0]["text"], "hello");

    let filtered = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            router(state.clone())
                .oneshot(
                    Request::get("/clients/silo/messages?channels=say,shout")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
    assert!(
        response_json(filtered)["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let invalid = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            router(state)
                .oneshot(
                    Request::get("/clients/silo/messages?count=0")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn correlates_spell_feedback_before_broadcasting_to_subscribers() {
    let (state, mut registry) = state_and_registry();
    let identity = RegistryClientIdentity::from_hello(hello());
    let mut receiver = state.subscribe();
    commit_change(
        &state,
        &mut registry,
        ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![
                StateEvent {
                    sequence: 3,
                    revision: 4,
                    tick_ms: 520,
                    update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                        slot: 1,
                        arguments: ModelSpellCastArguments::None,
                    }),
                },
                StateEvent {
                    sequence: 4,
                    revision: 5,
                    tick_ms: 545,
                    update: StateUpdate::Message(ModelClientMessage {
                        kind: ModelMessageKind::System,
                        sender: None,
                        recipient: None,
                        text: "You cast Mist.".into(),
                    }),
                },
            ],
        },
    );

    assert!(matches!(
        receiver.try_recv().unwrap(),
        PublishedEvent::State { feedback: None, .. }
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        PublishedEvent::State {
            feedback: Some(feedback),
            ..
        } if matches!(*feedback, SpellFeedback::Succeeded(_))
    ));
}

#[test]
fn filters_world_objects_by_type() {
    let all = json("/clients/silo/objects");
    assert_eq!(all["objects"].as_array().unwrap().len(), 4);
    assert_eq!(all["objects"][0]["is_solid"], true);
    assert_eq!(all["objects"][1]["is_solid"], false);
    assert_eq!(all["objects"][2]["is_solid"], true);
    assert_eq!(all["objects"][3]["is_solid"], false);
    assert_eq!(all["objects"][3]["dye_color"], 5);

    let filtered = json("/clients/silo/objects?types=npc,player");
    let kinds = filtered["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["player", "mundane"]);
    assert_eq!(filtered["objects"][0]["is_hidden"], false);
    assert_eq!(filtered["objects"][0]["visual"]["form"], "human");
    assert_eq!(filtered["objects"][0]["visual"]["head_sprite"], 101);
    assert_eq!(filtered["objects"][0]["visual"]["skin_color"], 4);
    assert_eq!(filtered["objects"][0]["visual"]["accessory3_sprite"], 111);

    let invalid = response("/clients/silo/objects?types=dragon");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(invalid)["error"]["code"],
        "invalid_object_query"
    );
}

#[test]
fn disconnected_snapshots_fall_back_to_the_process_id() {
    let mut registry = Registry::new();
    let hello = hello();
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    let mut snapshot = game_snapshot();
    snapshot.lifecycle = ClientLifecycle::Disconnected;
    registry.apply(&ConnectionEvent::Snapshot {
        pid: 42,
        identity: RegistryClientIdentity::from_hello(hello),
        snapshot: Box::new(snapshot),
    });

    let snapshot = registry.snapshot();
    let clients = serde_json::to_value(ClientList::from(&snapshot)).unwrap();
    assert_eq!(clients["clients"][0]["name"], "42");
    assert_eq!(resolve_client(&snapshot, "42").unwrap().pid, 42);
    assert_eq!(
        resolve_client(&snapshot, "silo").unwrap_err().status,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn duplicate_active_character_names_are_ambiguous() {
    let mut registry = Registry::new();
    for (pid, instance) in [(42, 0xAB), (43, 0xAC)] {
        let mut hello = hello();
        hello.process_id = pid;
        hello.process_creation_time += u64::from(pid);
        hello.dll_instance_id = [instance; 16];
        registry.apply(&ConnectionEvent::Connected {
            pid,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Snapshot {
            pid,
            identity: RegistryClientIdentity::from_hello(hello),
            snapshot: Box::new(game_snapshot()),
        });
    }

    assert_eq!(
        resolve_client(&registry.snapshot(), "SILO")
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
}

#[test]
fn serializes_every_client_status() {
    let mut registry = Registry::new();
    registry.apply(&ConnectionEvent::Connecting { pid: 1 });
    registry.apply(&ConnectionEvent::Initializing { pid: 2 });
    registry.apply(&ConnectionEvent::NotLoaded { pid: 3 });
    let mut connected = hello();
    connected.process_id = 4;
    registry.apply(&ConnectionEvent::Connected {
        pid: 4,
        hello: connected,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    registry.apply(&ConnectionEvent::Busy { pid: 5 });
    registry.apply(&ConnectionEvent::Disconnected {
        pid: 6,
        identity: None,
        reason: "closed".into(),
    });
    registry.apply(&ConnectionEvent::Incompatible {
        pid: 7,
        identity: None,
        reason: "unsupported".into(),
    });

    let value = serde_json::to_value(ClientList::from(&registry.snapshot())).unwrap();
    let statuses = value["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|client| client["status"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        [
            "connecting",
            "initializing",
            "not_loaded",
            "connected",
            "busy",
            "disconnected",
            "incompatible",
        ]
    );
}
