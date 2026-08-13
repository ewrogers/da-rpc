use super::*;

fn assert_routes_action(path: &str, body: &str, expected_kind: CommandKind) {
    assert_routes_action_request(
        axum::http::Method::POST,
        path,
        body,
        expected_kind,
        game_snapshot(),
    );
}

fn assert_routes_put_action(path: &str, body: &str, expected_kind: CommandKind) {
    assert_routes_action_request(
        axum::http::Method::PUT,
        path,
        body,
        expected_kind,
        game_snapshot(),
    );
}

fn assert_routes_delete_action(path: &str, expected_kind: CommandKind) {
    assert_routes_action_request(
        axum::http::Method::DELETE,
        path,
        "",
        expected_kind,
        game_snapshot(),
    );
}

fn assert_routes_action_with_snapshot(
    path: &str,
    body: &str,
    expected_kind: CommandKind,
    snapshot: ModelClientSnapshot,
) {
    assert_routes_action_request(
        axum::http::Method::POST,
        path,
        body,
        expected_kind,
        snapshot,
    );
}

fn assert_routes_action_request(
    method: axum::http::Method,
    path: &str,
    body: &str,
    expected_kind: CommandKind,
    snapshot: ModelClientSnapshot,
) {
    let mut registry = Registry::new();
    let hello = hello();
    let identity = RegistryClientIdentity::from_hello(hello);
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    registry.apply(&ConnectionEvent::Snapshot {
        pid: 42,
        identity,
        snapshot: Box::new(snapshot),
    });
    let (events, _event_receiver) = mpsc::channel();
    let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
        .with_command_sender(commands);
    let worker = std::thread::spawn(move || {
        let call = command_receiver.recv().unwrap();
        assert_eq!(call.pid, 42);
        assert_eq!(call.identity, identity);
        assert!(matches!(
            call.operation,
            CommandOperation::Submit {
                kind,
                timeout_ms: 1_000,
                wait_ms: 1_000,
            } if kind == expected_kind
        ));
        call.reply
            .send(CommandReply::Result(CommandResult::Status(CommandStatus {
                command_id: 9,
                kind: expected_kind,
                state: CommandState::Executed,
                enqueued_tick_ms: 100,
                deadline_tick_ms: 1_100,
                started_tick_ms: Some(104),
                completed_tick_ms: Some(104),
                execution_us: Some(2),
                main_thread_id: Some(77),
                failure: None,
            })))
            .unwrap();
    });

    let response = match method {
        axum::http::Method::POST if body.is_empty() => post_empty(state, path),
        axum::http::Method::POST => post_json(state, path, body),
        axum::http::Method::PUT => put_json(state, path, body),
        axum::http::Method::DELETE => delete_empty(state, path),
        _ => panic!("unsupported test request method"),
    };
    assert_eq!(response.status(), StatusCode::OK, "route failed: {path}");
    let response = response_json(response);
    assert_eq!(response["command_id"], 9);
    assert_eq!(response["state"], "executed");
    worker.join().unwrap();
}

#[test]
fn routes_a_diagnostic_through_the_bounded_command_path() {
    let mut registry = Registry::new();
    let hello = hello();
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    let (events, event_receiver) = mpsc::channel();
    let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
        .with_command_sender(commands);
    let worker = std::thread::spawn(move || {
        assert!(matches!(
            event_receiver.recv().unwrap(),
            DaemonEvent::CommandsReady
        ));
        let call = command_receiver.recv().unwrap();
        assert_eq!(call.pid, 42);
        assert_eq!(call.identity, RegistryClientIdentity::from_hello(hello));
        assert!(matches!(
            call.operation,
            CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: 1_000,
                wait_ms: 1_000,
            }
        ));
        call.reply
            .send(CommandReply::Result(CommandResult::Status(CommandStatus {
                command_id: 9,
                kind: CommandKind::Diagnostic,
                state: CommandState::Executed,
                enqueued_tick_ms: 100,
                deadline_tick_ms: 1_100,
                started_tick_ms: Some(104),
                completed_tick_ms: Some(104),
                execution_us: Some(2),
                main_thread_id: Some(77),
                failure: None,
            })))
            .unwrap();
    });

    let response = post_json(state, "/clients/42/commands/diagnostic", "{}");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response);
    assert_eq!(body["pid"], 42);
    assert_eq!(body["instance_id"], "abababababababababababababababab");
    assert_eq!(body["command_id"], 9);
    assert_eq!(body["state"], "executed");
    assert_eq!(body["queue_delay_ms"], 4);
    assert_eq!(body["execution_us"], 2);
    assert_eq!(body["main_thread_id"], 77);
    worker.join().unwrap();
}

#[test]
fn legend_route_returns_refreshed_marks_with_friendly_icons() {
    let mut registry = Registry::new();
    let hello = hello();
    let identity = RegistryClientIdentity::from_hello(hello);
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    registry.apply(&ConnectionEvent::Snapshot {
        pid: 42,
        identity,
        snapshot: Box::new(game_snapshot()),
    });
    let (events, _event_receiver) = mpsc::channel();
    let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
        .with_command_sender(commands);
    let worker = std::thread::spawn(move || {
        let call = command_receiver.recv().unwrap();
        assert!(matches!(
            call.operation,
            CommandOperation::Submit {
                kind: CommandKind::Legend,
                timeout_ms: 3_000,
                wait_ms: 1_000,
            }
        ));
        call.reply
            .send(CommandReply::Result(CommandResult::Legend {
                status: CommandStatus {
                    command_id: 9,
                    kind: CommandKind::Legend,
                    state: CommandState::Executed,
                    enqueued_tick_ms: 100,
                    deadline_tick_ms: 3_100,
                    started_tick_ms: Some(101),
                    completed_tick_ms: Some(120),
                    execution_us: Some(2),
                    main_thread_id: Some(77),
                    failure: None,
                },
                marks: vec![ModelLegendMark {
                    text: "Found the hidden grove".into(),
                    tag: "Quest".into(),
                    color: 7,
                    icon: ModelLegendIcon::Wizard,
                }],
            }))
            .unwrap();
    });

    let response = json_with_state(state, "/clients/42/legend");
    assert_eq!(response["pid"], 42);
    assert_eq!(response["received_tick_ms"], 120);
    assert_eq!(response["marks"][0]["text"], "Found the hidden grove");
    assert_eq!(response["marks"][0]["tag"], "Quest");
    assert_eq!(response["marks"][0]["color"], 7);
    assert_eq!(response["marks"][0]["icon"], "wizard");
    worker.join().unwrap();
}

#[test]
fn cached_player_route_resolves_visible_name_without_refreshing() {
    let mut snapshot = game_snapshot();
    let player = snapshot
        .objects
        .as_mut()
        .and_then(|objects| objects.first_mut())
        .expect("fixture has a visible player");
    let ModelWorldObject::Player { profile, .. } = player else {
        panic!("first fixture object is a player")
    };
    *profile = Some(Box::new(player_profile()));

    let body = json_with_state(state_with_snapshot(snapshot), "/clients/42/players/eIDoLoN");
    assert_eq!(body["kind"], "player");
    assert_eq!(body["name"], "Eidolon");
    assert_eq!(body["profile"]["identity"]["nation"], "mileth");
    assert_eq!(body["profile"]["identity"]["display_class"], "Summoner");
    assert_eq!(body["profile"]["is_group_open"], true);
    assert_eq!(body["profile"]["equipment"][0]["slot"], "necklace");
    assert_eq!(body["profile"]["inspected_tick_ms"], 120);
}

#[test]
fn cached_player_route_exposes_a_pending_null_profile() {
    let body = json("/clients/42/players/eIDoLoN");
    assert_eq!(body["name"], "Eidolon");
    assert!(body["profile"].is_null());
}

#[test]
fn player_inspection_route_resolves_visible_name_and_returns_profile() {
    let mut registry = Registry::new();
    let hello = hello();
    let identity = RegistryClientIdentity::from_hello(hello);
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    registry.apply(&ConnectionEvent::Snapshot {
        pid: 42,
        identity,
        snapshot: Box::new(game_snapshot()),
    });
    let (events, _event_receiver) = mpsc::channel();
    let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
        .with_command_sender(commands);
    let profile = player_profile();
    let worker_player = ModelWorldObject::Player {
        id: 1,
        name: Some("Eidolon".into()),
        x: 10,
        y: 20,
        direction: ModelDirection::North,
        profile: Some(Box::new(profile.clone())),
    };
    let worker = std::thread::spawn(move || {
        let call = command_receiver.recv().unwrap();
        assert!(matches!(
            call.operation,
            CommandOperation::Submit {
                kind: CommandKind::InspectPlayer(id),
                timeout_ms: 3_000,
                wait_ms: 1_000,
            } if id.get() == 1
        ));
        call.reply
            .send(CommandReply::Result(CommandResult::Player {
                status: CommandStatus {
                    command_id: 9,
                    kind: CommandKind::InspectPlayer(std::num::NonZeroU32::new(1).unwrap()),
                    state: CommandState::Executed,
                    enqueued_tick_ms: 100,
                    deadline_tick_ms: 3_100,
                    started_tick_ms: Some(101),
                    completed_tick_ms: Some(120),
                    execution_us: Some(2),
                    main_thread_id: Some(77),
                    failure: None,
                },
                player: Box::new(worker_player),
            }))
            .unwrap();
    });

    let response = post_empty(state, "/clients/42/players/eIDoLoN/inspect");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response);
    assert_eq!(body["name"], "Eidolon");
    assert_eq!(body["profile"]["identity"]["nation"], "mileth");
    assert_eq!(body["profile"]["identity"]["display_class"], "Summoner");
    assert_eq!(body["profile"]["is_group_open"], true);
    assert_eq!(body["profile"]["equipment"][0]["slot"], "necklace");
    worker.join().unwrap();
}

#[test]
fn routes_typed_actions() {
    assert_routes_action("/clients/42/assail", "", CommandKind::Assail);
    assert_routes_action("/clients/42/resync", "", CommandKind::Resync);
    assert_routes_action(
        "/clients/42/group/invite",
        r#"{"target":"OtherPlayer"}"#,
        CommandKind::Group(GroupCommand::Invite(GroupText::new("OtherPlayer").unwrap())),
    );
    assert_routes_action(
        "/clients/42/raw/send",
        r#"{"direction":"client","command":"7E","payload":"00 03 02"}"#,
        CommandKind::Raw(
            RawPacket::new(RawPacketDirection::Client, 0x7e, &[0x00, 0x03, 0x02]).unwrap(),
        ),
    );
    assert_routes_action(
        "/clients/42/raw/send",
        r#"{"direction":"server","command":"0x3A","payload":""}"#,
        CommandKind::Raw(RawPacket::new(RawPacketDirection::Server, 0x3a, &[]).unwrap()),
    );
    assert_routes_action(
        "/clients/42/turn",
        r#"{"direction":"north"}"#,
        CommandKind::Turn(ModelDirection::North),
    );
    assert_routes_action(
        "/clients/42/walk",
        r#"{"direction":"east"}"#,
        CommandKind::Walk(WalkTarget::Direction(ModelDirection::East)),
    );
    assert_routes_action(
        "/clients/42/walk",
        r#"{"destination":{"x":99,"y":79}}"#,
        CommandKind::Walk(WalkTarget::Destination { x: 99, y: 79 }),
    );
    assert_routes_action(
        "/clients/42/walk",
        r#"{"route":{"map_id":3001,"tiles":[{"x":11,"y":22},{"x":12,"y":22},{"x":12,"y":23}]}}"#,
        CommandKind::Walk(WalkTarget::Route(
            WalkRoute::new(
                3001,
                &[
                    RouteTile { x: 11, y: 22 },
                    RouteTile { x: 12, y: 22 },
                    RouteTile { x: 12, y: 23 },
                ],
            )
            .unwrap(),
        )),
    );
    assert_routes_put_action(
        "/clients/42/maps/3002/path-exclusions",
        r#"{"tiles":[{"x":41,"y":50},{"x":40,"y":50},{"x":40,"y":50}]}"#,
        CommandKind::SetPathExclusions(
            PathExclusions::new(
                3002,
                &[RouteTile { x: 40, y: 50 }, RouteTile { x: 41, y: 50 }],
            )
            .unwrap(),
        ),
    );
    assert_routes_delete_action(
        "/clients/42/maps/3001/path-exclusions",
        CommandKind::RemovePathExclusions { map_id: 3001 },
    );
    assert_routes_delete_action(
        "/clients/42/maps/path-exclusions",
        CommandKind::ClearPathExclusions,
    );
    let skill = CommandKind::UseSkill(SkillSlot::new(4).unwrap());
    assert_routes_action("/clients/42/skills/use", r#"{"slot":4}"#, skill);
    assert_routes_action("/clients/42/skills/use", r#"{"name":"aSsAiL"}"#, skill);
    assert_routes_action(
        "/clients/42/skills/swap",
        r#"{"source":{"name":"aSsAiL"},"destination":{"slot":8}}"#,
        CommandKind::SwapSlots(SlotSwap::Skillbook {
            source: SkillSlot::new(4).unwrap(),
            destination: SkillSlot::new(8).unwrap(),
        }),
    );

    assert_routes_action(
        "/clients/42/spells/cast",
        r#"{"name":"mIsT"}"#,
        CommandKind::CastSpell(SpellCast {
            slot: SpellSlot::new(1).unwrap(),
            arguments: SpellArguments::None,
        }),
    );
    assert_routes_action(
        "/clients/42/spells/swap",
        r#"{"source":{"slot":1},"destination":{"name":"AO PUINSEIN"}}"#,
        CommandKind::SwapSlots(SlotSwap::Spellbook {
            source: SpellSlot::new(1).unwrap(),
            destination: SpellSlot::new(2).unwrap(),
        }),
    );
    assert_routes_action(
        "/clients/42/spells/cast",
        r#"{"name":"AO PUINSEIN","target":"eidolon"}"#,
        CommandKind::CastSpell(SpellCast {
            slot: SpellSlot::new(2).unwrap(),
            arguments: SpellArguments::Target(SpellTarget::Object(
                std::num::NonZeroU32::new(1).unwrap(),
            )),
        }),
    );
    assert_routes_action(
        "/clients/42/spells/cast",
        r#"{"name":"AO PUINSEIN"}"#,
        CommandKind::CastSpell(SpellCast {
            slot: SpellSlot::new(2).unwrap(),
            arguments: SpellArguments::None,
        }),
    );
    assert_routes_action(
        "/clients/42/spells/cast",
        r#"{"slot":2,"target":{"x":12,"y":20}}"#,
        CommandKind::CastSpell(SpellCast {
            slot: SpellSlot::new(2).unwrap(),
            arguments: SpellArguments::Target(SpellTarget::Tile { x: 12, y: 20 }),
        }),
    );
    assert_routes_action(
        "/clients/42/spells/cast",
        r#"{"name":"fas spiorad","input":"nothing"}"#,
        CommandKind::CastSpell(SpellCast {
            slot: SpellSlot::new(7).unwrap(),
            arguments: SpellArguments::Input(SpellInput::new("nothing").unwrap()),
        }),
    );
    assert_routes_action(
        "/clients/42/items/use",
        r#"{"name":"dark belt"}"#,
        CommandKind::UseItem(ItemSlot::new(1).unwrap()),
    );
    assert_routes_action(
        "/clients/42/items/drop",
        r#"{"slot":1,"destination":{"x":11,"y":22}}"#,
        CommandKind::DropItem(ItemTransfer {
            slot: ItemSlot::new(1).unwrap(),
            quantity: 1,
            target: TransferTarget::Tile(TilePosition { x: 11, y: 22 }),
        }),
    );
    assert_routes_action(
        "/clients/42/items/drop",
        r#"{"name":"DARK BELT","destination":{"x":11,"y":22}}"#,
        CommandKind::DropItem(ItemTransfer {
            slot: ItemSlot::new(1).unwrap(),
            quantity: 1,
            target: TransferTarget::Tile(TilePosition { x: 11, y: 22 }),
        }),
    );
    assert_routes_action(
        "/clients/42/items/give",
        r#"{"slot":1,"quantity":2,"target":2}"#,
        CommandKind::GiveItem(ItemTransfer {
            slot: ItemSlot::new(1).unwrap(),
            quantity: 2,
            target: TransferTarget::Object(std::num::NonZeroU32::new(2).unwrap()),
        }),
    );
    assert_routes_action(
        "/clients/42/items/give",
        r#"{"slot":1,"target":"iNnKeEpEr"}"#,
        CommandKind::GiveItem(ItemTransfer {
            slot: ItemSlot::new(1).unwrap(),
            quantity: 1,
            target: TransferTarget::Object(std::num::NonZeroU32::new(3).unwrap()),
        }),
    );
    assert_routes_action(
        "/clients/42/items/swap",
        r#"{"source":{"name":"DARK BELT"},"destination":{"slot":9}}"#,
        CommandKind::SwapSlots(SlotSwap::Inventory {
            source: ItemSlot::new(1).unwrap(),
            destination: ItemSlot::new(9).unwrap(),
        }),
    );
    assert_routes_action(
        "/clients/42/gold/drop",
        r#"{"amount":50,"destination":{"x":11,"y":22}}"#,
        CommandKind::DropGold(GoldTransfer {
            amount: 50,
            target: TransferTarget::Tile(TilePosition { x: 11, y: 22 }),
        }),
    );
    assert_routes_action(
        "/clients/42/gold/give",
        r#"{"amount":50,"target":"iNnKeEpEr"}"#,
        CommandKind::GiveGold(GoldTransfer {
            amount: 50,
            target: TransferTarget::Object(std::num::NonZeroU32::new(3).unwrap()),
        }),
    );
    assert_routes_action(
        "/clients/42/items/pickup",
        r#"{"position":{"x":11,"y":22}}"#,
        CommandKind::PickupItem(TilePosition { x: 11, y: 22 }),
    );
    assert_routes_action(
        "/clients/42/equipment/unequip",
        r#"{"slot":"armor"}"#,
        CommandKind::Unequip(ModelEquipmentSlot::Armor),
    );
    assert_routes_action(
        "/clients/42/emote",
        r#"{"code":12}"#,
        CommandKind::Emote(12),
    );
    assert_routes_action(
        "/clients/42/messages/send",
        r#"{"channel":"say","content":"hello"}"#,
        CommandKind::Message(MessageCommand::Say(MessageContent::new("hello").unwrap())),
    );
    for (channel, message) in [
        (
            "shout",
            MessageCommand::Shout(MessageContent::new("hello").unwrap()),
        ),
        (
            "guild",
            MessageCommand::Guild(MessageContent::new("hello").unwrap()),
        ),
        (
            "group",
            MessageCommand::Group(MessageContent::new("hello").unwrap()),
        ),
    ] {
        assert_routes_action(
            "/clients/42/messages/send",
            &format!(r#"{{"channel":"{channel}","content":"hello"}}"#),
            CommandKind::Message(message),
        );
    }
    assert_routes_action(
        "/clients/42/messages/send",
        r#"{"channel":"whisper","recipient":"Eidolon","content":"hello"}"#,
        CommandKind::Message(MessageCommand::Whisper {
            recipient: MessageRecipient::new("Eidolon").unwrap(),
            content: MessageContent::new("hello").unwrap(),
        }),
    );
    assert_routes_action(
        "/clients/42/emote",
        r#"{"name":"WaVe"}"#,
        CommandKind::Emote(13),
    );
    let item = "Dark-Belt  (Fine)";
    for (path, body, text) in [
        (
            "/clients/42/chant",
            r#"{"text":"MiXeD, punctuation!  "}"#,
            ChantText::new("MiXeD, punctuation!  ").unwrap(),
        ),
        (
            "/clients/42/items/sell",
            r#"{"name":"Dark-Belt  (Fine)"}"#,
            ChantText::sell(item).unwrap(),
        ),
        (
            "/clients/42/items/sell-all",
            r#"{"name":"Dark-Belt  (Fine)"}"#,
            ChantText::sell_all(item).unwrap(),
        ),
        (
            "/clients/42/items/deposit",
            r#"{"name":"Dark-Belt  (Fine)"}"#,
            ChantText::deposit(item).unwrap(),
        ),
        (
            "/clients/42/items/withdraw",
            r#"{"name":"Dark-Belt  (Fine)"}"#,
            ChantText::withdraw(item).unwrap(),
        ),
        (
            "/clients/42/items/repair",
            r#"{"name":"Dark-Belt  (Fine)"}"#,
            ChantText::repair(item).unwrap(),
        ),
        ("/clients/42/items/repair-all", "", ChantText::repair_all()),
    ] {
        assert_routes_action(path, body, CommandKind::Chant(text));
    }
    assert_routes_action(
        "/clients/42/interact",
        r#"{"target":"iNnKeEpEr"}"#,
        CommandKind::Interact(std::num::NonZeroU32::new(3).unwrap()),
    );
    assert_routes_action(
        "/clients/42/dialog/select",
        r#"{"revision":7,"index":0}"#,
        CommandKind::Dialog(DialogCommand {
            revision: 7,
            action: DialogAction::Select {
                index: 0,
                quantity: 1,
            },
        }),
    );
    assert_routes_action(
        "/clients/42/dialog/next",
        r#"{"revision":7}"#,
        CommandKind::Dialog(DialogCommand {
            revision: 7,
            action: DialogAction::Next,
        }),
    );
    assert_routes_action(
        "/clients/42/dialog/close",
        r#"{"revision":7}"#,
        CommandKind::Dialog(DialogCommand {
            revision: 7,
            action: DialogAction::Close,
        }),
    );
}

#[test]
fn validates_outbound_message_fields_before_routing() {
    for body in [
        r#"{"channel":"whisper","content":"hello"}"#.to_owned(),
        r#"{"channel":"say","recipient":"Eidolon","content":"hello"}"#.to_owned(),
        r#"{"channel":"whisper","recipient":"!!","content":"hello"}"#.to_owned(),
        r##"{"channel":"whisper","recipient":"#","content":"hello"}"##.to_owned(),
        r#"{"channel":"group","content":""}"#.to_owned(),
        format!(r#"{{"channel":"guild","content":"{}"}}"#, "x".repeat(101)),
    ] {
        let response = post_json(state(), "/clients/42/messages/send", &body);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "body: {body}");
    }
}

#[test]
fn sends_internal_content_to_a_named_client_without_the_game_limit() {
    let state = state();
    let mut events = state.subscribe();
    let content = "x".repeat(101);
    let response = post_json(
        state.clone(),
        "/messages/send",
        &format!(r#"{{"channel":"internal","recipient":"silo","content":"{content}"}}"#),
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response)["delivered"], 1);

    let history = json_with_state(state, "/clients/42/messages?channels=internal");
    let message = &history["messages"][0];
    assert_eq!(message["channel"], "internal");
    assert_eq!(message["recipient"], "SiLo");
    assert_eq!(message["payload"]["content"], content);
    assert!(message.get("text").is_none());
    assert!(message.get("tick_ms").is_none());

    let PublishedEvent::Internal {
        recipients,
        message,
    } = events.try_recv().unwrap()
    else {
        panic!("expected an internal message event");
    };
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0].pid, 42);
    assert_eq!(message.recipient.as_deref(), Some("SiLo"));
}

#[test]
fn validates_internal_message_shape_and_recipient() {
    for body in [
        r#"{"channel":"internal"}"#,
        r#"{"channel":"internal","content":"","payload":{}}"#,
        r#"{"channel":"internal","content":""}"#,
        r#"{"channel":"internal","payload":[]}"#,
        r#"{"channel":"say","content":"hello"}"#,
    ] {
        let response = post_json(state(), "/messages/send", body);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "body: {body}");
    }

    let response = post_json(
        state(),
        "/messages/send",
        r#"{"channel":"internal","recipient":"Missing","payload":{"ready":true}}"#,
    );
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(response)["error"]["code"],
        "recipient_not_found"
    );
}

#[test]
fn internal_broadcast_with_no_clients_succeeds() {
    let response = post_json(
        empty_state(),
        "/messages/send",
        r#"{"channel":"internal","payload":{"ready":true}}"#,
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response)["delivered"], 0);
}

#[test]
fn internal_broadcast_accepts_an_object_payload() {
    let state = state();
    let response = post_json(
        state.clone(),
        "/messages/send",
        r#"{"channel":"internal","payload":{"ready":true,"count":2}}"#,
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response)["delivered"], 1);

    let history = json_with_state(state, "/clients/42/messages?channels=internal");
    assert_eq!(history["messages"][0]["payload"]["ready"], true);
    assert_eq!(history["messages"][0]["payload"]["count"], 2);
    assert!(history["messages"][0].get("recipient").is_none());
}

#[test]
fn internal_messages_are_emitted_over_sse() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let state = state();
            let events = router(state.clone())
                .oneshot(
                    Request::get("/clients/42/events")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(events.status(), StatusCode::OK);

            let body = r#"{"channel":"internal","payload":{"ready":true}}"#;
            let response = router(state.clone())
                .oneshot(
                    Request::post("/messages/send")
                        .header("content-type", "application/json")
                        .header("content-length", body.len())
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);

            drop(state);
            let bytes = to_bytes(events.into_body(), 64 * 1024).await.unwrap();
            let stream = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(stream.contains("event: message.internal\n"));
            assert!(stream.contains("id: internal-1\n"));
            assert!(stream.contains(r#""channel":"internal""#));
            assert!(stream.contains(r#""payload":{"ready":true}"#));
        });
}

#[test]
fn serves_dialog_state_and_rejects_stale_actions() {
    let body = json("/clients/42/dialog");
    assert_eq!(body["dialog"]["revision"], 7);
    assert_eq!(body["dialog"]["speaker"]["name"], "Innkeeper");
    assert_eq!(body["dialog"]["interaction"]["type"], "choices");

    let response = post_json(
        state(),
        "/clients/42/dialog/select",
        r#"{"revision":6,"index":0}"#,
    );
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(response)["error"]["code"], "stale_dialog");
}

#[test]
fn serves_exchange_state_and_routes_exchange_actions() {
    let snapshot = exchange_snapshot();
    let exchange = json_with_state(
        state_with_snapshot(snapshot.clone()),
        "/clients/42/exchange",
    );
    assert_eq!(exchange["exchange"]["partner"], "ZiLo");
    assert_eq!(exchange["exchange"]["local"]["gold"], 0);
    assert_eq!(exchange["exchange"]["local"]["items"][0]["quantity"], 2);
    let status = json_with_state(state_with_snapshot(snapshot.clone()), "/clients/42/status");
    assert_eq!(status["character"]["is_in_exchange"], true);

    assert_routes_action_with_snapshot(
        "/clients/42/exchange/items",
        r#"{"name":"dark belt","quantity":2}"#,
        CommandKind::Exchange(ExchangeCommand::AddItem {
            slot: ItemSlot::new(1).unwrap(),
            quantity: 2,
        }),
        snapshot.clone(),
    );
    assert_routes_action_with_snapshot(
        "/clients/42/exchange/gold",
        r#"{"amount":50}"#,
        CommandKind::Exchange(ExchangeCommand::SetGold(50)),
        snapshot.clone(),
    );
    assert_routes_action_with_snapshot(
        "/clients/42/exchange/accept",
        "",
        CommandKind::Exchange(ExchangeCommand::Accept),
        snapshot.clone(),
    );
    assert_routes_action_with_snapshot(
        "/clients/42/exchange/cancel",
        "",
        CommandKind::Exchange(ExchangeCommand::Cancel),
        snapshot,
    );
}

#[test]
fn exchange_actions_enforce_offer_rules() {
    assert_eq!(
        post_json(state(), "/clients/42/exchange/accept", "").status(),
        StatusCode::CONFLICT
    );
    for (path, body) in [
        ("/clients/42/exchange/items", r#"{"slot":1,"quantity":0}"#),
        ("/clients/42/exchange/items", r#"{"slot":1,"quantity":4}"#),
        ("/clients/42/exchange/gold", r#"{"amount":0}"#),
        ("/clients/42/exchange/gold", r#"{"amount":100}"#),
    ] {
        assert_eq!(
            post_json(state_with_snapshot(exchange_snapshot()), path, body).status(),
            StatusCode::BAD_REQUEST,
            "request unexpectedly succeeded: {path} {body}"
        );
    }

    let mut gold_set = exchange_snapshot();
    gold_set.exchange.as_mut().unwrap().local.gold = 1;
    assert_eq!(
        post_json(
            state_with_snapshot(gold_set),
            "/clients/42/exchange/gold",
            r#"{"amount":1}"#,
        )
        .status(),
        StatusCode::CONFLICT
    );

    let mut accepted = exchange_snapshot();
    accepted.exchange.as_mut().unwrap().local.accepted = true;
    assert_eq!(
        post_json(
            state_with_snapshot(accepted.clone()),
            "/clients/42/exchange/items",
            r#"{"slot":1}"#,
        )
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        post_json(
            state_with_snapshot(accepted),
            "/clients/42/exchange/gold",
            r#"{"amount":1}"#,
        )
        .status(),
        StatusCode::CONFLICT
    );
}

#[test]
fn rejects_invalid_movement_requests() {
    for (path, body) in [
        ("/clients/42/turn", r#"{"direction":"up"}"#),
        ("/clients/42/walk", r#"{}"#),
        (
            "/clients/42/walk",
            r#"{"direction":"north","destination":{"x":1,"y":1}}"#,
        ),
        ("/clients/42/walk", r#"{"destination":{"x":-1,"y":0}}"#),
        ("/clients/42/walk", r#"{"destination":{"x":100,"y":79}}"#),
        ("/clients/42/walk", r#"{"destination":{"x":99,"y":80}}"#),
    ] {
        assert_eq!(
            post_json(state(), path, body).status(),
            StatusCode::BAD_REQUEST
        );
    }

    for (path, body) in [
        ("/clients/42/maps/3001/path-exclusions", r#"{"tiles":[]}"#),
        (
            "/clients/42/maps/3001/path-exclusions",
            r#"{"tiles":[{"x":400,"y":0}]}"#,
        ),
        (
            "/clients/42/maps/65536/path-exclusions",
            r#"{"tiles":[{"x":0,"y":0}]}"#,
        ),
    ] {
        assert_eq!(
            put_json(state(), path, body).status(),
            StatusCode::BAD_REQUEST
        );
    }
}

#[test]
fn rejects_invalid_or_unknown_skill_selectors() {
    for body in [
        r#"{}"#,
        r#"{"slot":4,"name":"Assail"}"#,
        r#"{"slot":0}"#,
        r#"{"slot":91}"#,
        r#"{"name":""}"#,
    ] {
        assert_eq!(
            post_json(state(), "/clients/42/skills/use", body).status(),
            StatusCode::BAD_REQUEST
        );
    }
    for body in [r#"{"slot":5}"#, r#"{"name":"Kick"}"#] {
        assert_eq!(
            post_json(state(), "/clients/42/skills/use", body).status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[test]
fn keeps_drop_give_and_swap_payloads_distinct() {
    for (path, body) in [
        ("/clients/42/items/drop", r#"{"slot":1,"target":2}"#),
        (
            "/clients/42/items/give",
            r#"{"slot":1,"destination":{"x":11,"y":22}}"#,
        ),
        ("/clients/42/gold/drop", r#"{"amount":1,"target":2}"#),
        (
            "/clients/42/gold/give",
            r#"{"amount":1,"destination":{"x":11,"y":22}}"#,
        ),
        (
            "/clients/42/items/swap",
            r#"{"source":{"slot":1,"name":"Dark Belt"},"destination":{"slot":2}}"#,
        ),
        (
            "/clients/42/spells/swap",
            r#"{"source":{"slot":1},"destination":{"name":"Mist"}}"#,
        ),
    ] {
        assert_eq!(
            post_json(state(), path, body).status(),
            StatusCode::BAD_REQUEST,
            "request unexpectedly succeeded: {path} {body}"
        );
    }
    assert_eq!(
        post_json(
            state(),
            "/clients/42/skills/swap",
            r#"{"source":{"slot":8},"destination":{"slot":9}}"#,
        )
        .status(),
        StatusCode::NOT_FOUND
    );
    for body in [
        r#"{"direction":"outbound","command":"0x7E","payload":"00"}"#,
        r#"{"direction":"client","command":"7","payload":"00"}"#,
        r#"{"direction":"client","command":"0x7E","payload":"0002"}"#,
    ] {
        assert_eq!(
            post_json(state(), "/clients/42/raw/send", body).status(),
            StatusCode::BAD_REQUEST
        );
    }
}

#[test]
fn rejects_movement_without_ready_in_game_state() {
    let hello = hello();
    let (state, _receiver) = state_with_status(ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    assert_eq!(
        post_json(state, "/clients/42/turn", r#"{"direction":"north"}"#).status(),
        StatusCode::CONFLICT
    );
}
