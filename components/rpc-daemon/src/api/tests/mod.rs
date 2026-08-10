use super::{ApiState, ClientList, LaunchOptions, resolve_client, router};
use crate::{
    commands::{CommandReply, ROUTER_CAPACITY},
    event::DaemonEvent,
    lifecycle::{
        LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
        LifecycleOutcome, ManagementError,
    },
    registry::{ClientIdentity as RegistryClientIdentity, ConnectionEvent, Registry},
    stream::{PublishedEvent, SpellFeedback},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::DateTime;
use darpc_model::{
    AbilityUpdate, CharacterAppearance, CharacterClass, CharacterProgression,
    CharacterSnapshot as ModelCharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle,
    ClientMessage as ModelClientMessage, ClientSnapshot as ModelClientSnapshot,
    CooldownStatus as ModelCooldownStatus, CreatureKind, DialogChoice as ModelDialogChoice,
    DialogInteraction as ModelDialogInteraction, DialogKind as ModelDialogKind,
    DialogNavigation as ModelDialogNavigation, DialogSpeaker as ModelDialogSpeaker,
    DialogSpriteType as ModelDialogSpriteType, DialogState as ModelDialogState,
    DialogTarget as ModelDialogTarget, Direction as ModelDirection, Effect as ModelEffect,
    EffectDuration as ModelEffectDuration, EquipmentItem as ModelEquipmentItem,
    EquipmentSlot as ModelEquipmentSlot, ExchangeItem as ModelExchangeItem,
    ExchangeOffer as ModelExchangeOffer, ExchangeState as ModelExchangeState, Gender,
    InventoryItem as ModelInventoryItem, LegendIcon as ModelLegendIcon,
    LegendMark as ModelLegendMark, MapLocation, MessageKind as ModelMessageKind,
    Skill as ModelSkill, Spell as ModelSpell, SpellCastArguments as ModelSpellCastArguments,
    SpellTargetType as ModelSpellTargetType, StateEvent, StateUpdate,
    WorldObject as ModelWorldObject,
};
use darpc_protocol::{
    Architecture, ChantText, CommandKind, CommandOperation, CommandResult, CommandState,
    CommandStatus, ComponentVersion, DialogAction, DialogCommand, ExchangeCommand, GoldTransfer,
    Hello, ItemSlot, ItemTransfer, RawPacket, RawPacketDirection, SUPPORTED_VERSIONS, SkillSlot,
    SlotSwap, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, TilePosition,
    TransferTarget, WalkTarget,
};
use serde_json::Value;
use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    sync::{Arc, mpsc},
};
use tower::ServiceExt as _;

fn hello() -> Hello {
    Hello {
        protocol_versions: SUPPORTED_VERSIONS,
        dll_instance_id: [0xAB; 16],
        process_id: 42,
        process_creation_time: 134_299_999_186_432_946,
        architecture: Architecture::X86,
        dll_version: ComponentVersion {
            major: 0,
            minor: 1,
            patch: 0,
        },
        executable_fingerprint: [0xCD; 32],
        client_version: 741,
    }
}

fn game_snapshot() -> ModelClientSnapshot {
    ModelClientSnapshot {
        revision: 3,
        event_sequence: 2,
        captured_tick_ms: 500,
        updated_tick_ms: 510,
        capture_duration_us: 75,
        world_generation: 1,
        lifecycle: ClientLifecycle::InGame,
        character: Some(ModelCharacterSnapshot {
            id: Some(1234),
            name: Some("SiLo".into()),
            appearance: Some(CharacterAppearance {
                gender: Gender::Male,
                hair_style: 17,
                hair_color: 6,
                body_sprite: 1,
            }),
            class: CharacterClass::Wizard,
            is_action_restricted: false,
            is_blinded: true,
            is_walking: false,
            is_casting: false,
            gold: 99,
            weight: 25,
            max_weight: 60,
            progression: CharacterProgression {
                level: 50,
                ability_level: 2,
                experience: 1_000,
                ability_points: Some(10),
                experience_to_next_level: Some(20),
                ability_to_next_level: Some(30),
            },
            stats: CharacterStats {
                strength: 3,
                intelligence: 7,
                wisdom: 6,
                constitution: 5,
                dexterity: 4,
            },
            vitals: CharacterVitals {
                health: 80,
                max_health: 100,
                mana: 60,
                max_mana: 70,
            },
            modifiers: None,
            location: Some(MapLocation {
                id: 3001,
                name: None,
                x: Some(11),
                y: Some(22),
                width: 100,
                height: 80,
            }),
            inventory: Some(vec![ModelInventoryItem {
                slot: 1,
                sprite: 0x0123,
                dye_color: 7,
                name: Some("Dark Belt".into()),
                quantity: 3,
                can_stack: true,
                durability: 41,
                max_durability: 50,
            }]),
            equipment: Some(vec![ModelEquipmentItem {
                slot: ModelEquipmentSlot::Armor,
                sprite: 0x1234,
                dye_color: 2,
                name: Some("Hy-Brasyl Armor".into()),
                durability: 900,
                max_durability: 1_000,
            }]),
            spellbook: Some(vec![
                ModelSpell {
                    slot: 7,
                    icon: 0x0456,
                    name: Some("Fas Spiorad".into()),
                    level: 3,
                    max_level: 5,
                    lines: 4,
                    target_type: ModelSpellTargetType::TextInput,
                    prompt: Some("Who?".into()),
                    cooldown: ModelCooldownStatus {
                        active: true,
                        remaining_ms: None,
                    },
                },
                ModelSpell {
                    slot: 1,
                    icon: 0x0400,
                    name: Some("Mist".into()),
                    level: 1,
                    max_level: 100,
                    lines: 0,
                    target_type: ModelSpellTargetType::None,
                    prompt: None,
                    cooldown: ModelCooldownStatus {
                        active: false,
                        remaining_ms: None,
                    },
                },
                ModelSpell {
                    slot: 2,
                    icon: 0x0401,
                    name: Some("Ao Puinsein".into()),
                    level: 1,
                    max_level: 100,
                    lines: 0,
                    target_type: ModelSpellTargetType::Target,
                    prompt: None,
                    cooldown: ModelCooldownStatus {
                        active: false,
                        remaining_ms: None,
                    },
                },
            ]),
            skillbook: Some(vec![ModelSkill {
                slot: 4,
                icon: 0x0123,
                name: Some("Assail".into()),
                level: 10,
                max_level: 100,
                cooldown: ModelCooldownStatus {
                    active: true,
                    remaining_ms: Some(750),
                },
            }]),
            effects: Some(vec![ModelEffect {
                icon: 300,
                duration: ModelEffectDuration::White,
            }]),
        }),
        objects: Some(vec![
            ModelWorldObject::Player {
                id: 1,
                name: Some("Eidolon".into()),
                x: 10,
                y: 20,
                direction: ModelDirection::North,
            },
            ModelWorldObject::Creature {
                id: 2,
                kind: CreatureKind::Monster,
                sprite: Some(100),
                name: None,
                x: 11,
                y: 20,
                direction: ModelDirection::East,
            },
            ModelWorldObject::Creature {
                id: 3,
                kind: CreatureKind::Npc,
                sprite: Some(200),
                name: Some("Innkeeper".into()),
                x: 12,
                y: 20,
                direction: ModelDirection::South,
            },
            ModelWorldObject::Item {
                id: 4,
                sprite: 300,
                x: 13,
                y: 20,
                z_index: 0,
            },
        ]),
        dialog: Some(ModelDialogState {
            revision: 7,
            kind: ModelDialogKind::Pursuit,
            target: ModelDialogTarget { id: 3 },
            speaker: ModelDialogSpeaker {
                name: Some("Innkeeper".into()),
                sprite: 200,
                sprite_type: ModelDialogSpriteType::Creature,
                color: 0,
                show_graphic: true,
            },
            content: Some("How can I help?".into()),
            response_pending: false,
            navigation: ModelDialogNavigation {
                previous: false,
                next: true,
                close: true,
            },
            interaction: ModelDialogInteraction::Choices(vec![ModelDialogChoice {
                index: 0,
                text: "Tell me more".into(),
            }]),
        }),
        group: None,
        exchange: None,
        legend: None,
    }
}

fn exchange_snapshot() -> ModelClientSnapshot {
    let mut snapshot = game_snapshot();
    snapshot.exchange = Some(ModelExchangeState {
        id: 1234,
        partner: "ZiLo".into(),
        local: ModelExchangeOffer {
            items: vec![ModelExchangeItem {
                index: 0,
                sprite: 123,
                dye_color: 4,
                quantity: Some(2),
                name: "Wine".into(),
            }],
            ..ModelExchangeOffer::default()
        },
        other: ModelExchangeOffer::default(),
    });
    snapshot
}

fn state() -> ApiState {
    state_with_snapshot(game_snapshot())
}

fn state_with_snapshot(snapshot: ModelClientSnapshot) -> ApiState {
    let mut registry = Registry::new();
    let hello = hello();
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    registry.apply(&ConnectionEvent::Snapshot {
        pid: 42,
        identity: RegistryClientIdentity::from_hello(hello),
        snapshot: Box::new(snapshot),
    });
    let (events, _receiver) = mpsc::channel();
    ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
}

struct FakeLifecycle;

impl LifecycleControl for FakeLifecycle {
    fn load(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
        Ok(LifecycleOutcome {
            operation: LifecycleOperation::Load,
            pid,
            changed: true,
            darpc_loaded: true,
        })
    }

    fn unload(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
        Ok(LifecycleOutcome {
            operation: LifecycleOperation::Unload,
            pid,
            changed: true,
            darpc_loaded: false,
        })
    }

    fn launch(&self, _options: &ManagedLaunchOptions) -> Result<LifecycleOutcome, ManagementError> {
        Ok(LifecycleOutcome {
            operation: LifecycleOperation::Launch,
            pid: 77,
            changed: true,
            darpc_loaded: true,
        })
    }
}

fn response(path: &str) -> axum::response::Response {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            router(state())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
        })
}

fn json(path: &str) -> Value {
    json_with_state(state(), path)
}

fn json_with_state(state: ApiState, path: &str) -> Value {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let response = router(state)
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
}

fn text(path: &str) -> String {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let response = router(state())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        })
}

fn state_with_status(event: ConnectionEvent) -> (ApiState, mpsc::Receiver<DaemonEvent>) {
    let mut registry = Registry::new();
    registry.apply(&event);
    let (events, receiver) = mpsc::channel();
    (
        ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events),
        receiver,
    )
}

fn post_json(state: ApiState, path: &str, body: &str) -> axum::response::Response {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            router(state.clone())
                .oneshot(
                    Request::post(path)
                        .header("content-type", "application/json")
                        .header("content-length", body.len())
                        .body(Body::from(body.to_owned()))
                        .unwrap(),
                )
                .await
                .unwrap()
        })
}

fn post_empty(state: ApiState, path: &str) -> axum::response::Response {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            router(state)
                .oneshot(Request::post(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
        })
}

fn assert_routes_action(path: &str, body: &str, expected_kind: CommandKind) {
    assert_routes_action_with_snapshot(path, body, expected_kind, game_snapshot());
}

fn assert_routes_action_with_snapshot(
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

    let response = if body.is_empty() {
        post_empty(state, path)
    } else {
        post_json(state, path, body)
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
fn routes_typed_actions() {
    assert_routes_action("/clients/42/assail", "", CommandKind::Assail);
    assert_routes_action(
        "/clients/42/raw/send",
        r#"{"direction":"client","command":"0x7E","payload":"00 03 02"}"#,
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
        r#"{"direction":"client","command":"7E","payload":"00"}"#,
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

fn response_json(response: axum::response::Response) -> Value {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
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
        "1.0"
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
    assert_eq!(status["character"]["is_action_restricted"], false);
    assert_eq!(status["character"]["is_blinded"], true);
    assert_eq!(status["character"]["is_walking"], false);
    assert_eq!(status["character"]["is_in_exchange"], false);
    assert!(status["character"].get("gender_id").is_none());
    assert!(status["character"].get("class_id").is_none());
    assert!(status["character"].get("inventory").is_none());
    assert!(status["character"].get("equipment").is_none());
    assert!(status["character"].get("spellbook").is_none());
    assert!(status["character"].get("skillbook").is_none());
    assert_eq!(status["character"]["progression"]["level"], 50);
    assert_eq!(status["map"]["x"], 11);

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
    let state = state();
    let identity = RegistryClientIdentity::from_hello(hello());
    state.publish_connection_event(&ConnectionEvent::StateEvents {
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
    });

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
    let state = state();
    let identity = RegistryClientIdentity::from_hello(hello());
    let mut receiver = state.subscribe();
    state.publish_connection_event(&ConnectionEvent::StateEvents {
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
    });

    assert!(matches!(
        receiver.try_recv().unwrap(),
        PublishedEvent::State { feedback: None, .. }
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        PublishedEvent::State {
            feedback: Some(SpellFeedback::Succeeded(_)),
            ..
        }
    ));
}

#[test]
fn filters_world_objects_by_type() {
    let all = json("/clients/silo/objects");
    assert_eq!(all["objects"].as_array().unwrap().len(), 4);

    let filtered = json("/clients/silo/objects?types=npc,player");
    let kinds = filtered["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["player", "mundane"]);

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

#[test]
fn serves_the_openapi_contract_and_vendored_swagger_ui() {
    let openapi = json("/openapi.json");
    assert_eq!(openapi["openapi"], "3.1.0");
    assert_eq!(openapi["info"]["title"], "daRPC API");
    assert!(openapi["paths"]["/health"].is_object());
    assert!(openapi["paths"]["/clients"].is_object());
    for path in [
        "/clients/{client}/status",
        "/clients/{client}/items",
        "/clients/{client}/equipment",
        "/clients/{client}/spells",
        "/clients/{client}/skills",
        "/clients/{client}/effects",
        "/clients/{client}/objects",
        "/clients/{client}/messages",
        "/clients/{client}/events",
        "/clients/{client}/turn",
        "/clients/{client}/walk",
        "/clients/{client}/skills/use",
        "/clients/{client}/skills/swap",
        "/clients/{client}/spells/cast",
        "/clients/{client}/spells/swap",
        "/clients/{client}/items/use",
        "/clients/{client}/items/drop",
        "/clients/{client}/items/give",
        "/clients/{client}/items/swap",
        "/clients/{client}/items/pickup",
        "/clients/{client}/equipment/unequip",
        "/clients/{client}/gold/drop",
        "/clients/{client}/gold/give",
        "/clients/{client}/exchange",
        "/clients/{client}/exchange/items",
        "/clients/{client}/exchange/gold",
        "/clients/{client}/exchange/accept",
        "/clients/{client}/exchange/cancel",
        "/clients/{client}/emote",
        "/clients/{client}/raw/send",
        "/clients/{client}/assail",
        "/clients/{client}/commands/diagnostic",
        "/clients/{client}/commands/{command_id}",
    ] {
        assert!(openapi["paths"][path].is_object(), "OpenAPI omitted {path}");
    }
    assert!(
        openapi["paths"]["/clients/{client}/raw/send"]["post"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("crash"))
    );
    let object_parameters = openapi["paths"]["/clients/{client}/objects"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert!(object_parameters.iter().any(|parameter| {
        parameter["name"] == "types" && parameter["in"] == "query" && parameter["required"] != true
    }));
    assert!(openapi["paths"]["/clients/{client}/snapshot"].is_null());
    assert!(openapi["paths"]["/clients/launch"].is_object());
    assert!(openapi["paths"]["/clients/{client}/load"].is_object());
    assert!(openapi["paths"]["/clients/{client}/unload"].is_object());
    let schemas = openapi["components"]["schemas"].as_object().unwrap();
    for name in [
        "HealthState",
        "HealthStatus",
        "RawDirection",
        "RawSendOptions",
        "ClientList",
        "ClientState",
        "ClientStatus",
        "ClientIdentity",
        "ConnectionMetadata",
        "ObservationMetadata",
        "GameStatus",
        "ClientLifecycle",
        "CharacterStatus",
        "CharacterGender",
        "CharacterClass",
        "CharacterProgression",
        "CharacterStats",
        "CharacterVitals",
        "CharacterModifiers",
        "Element",
        "MapLocation",
        "Inventory",
        "InventoryItem",
        "Equipment",
        "EquipmentItem",
        "EquipmentSlot",
        "Spellbook",
        "Spell",
        "Skillbook",
        "Skill",
        "CooldownStatus",
        "SpellTargetType",
        "Effects",
        "Effect",
        "EffectDuration",
        "WorldObjects",
        "WorldObject",
        "Direction",
        "Messages",
        "Message",
        "MessageChannel",
        "LaunchOptions",
        "LoadResult",
        "LifecycleResult",
        "LifecycleAction",
        "UnloadResult",
        "ErrorState",
        "ErrorDetail",
        "ClientEvent",
        "ClientLifecycleChanged",
        "SoundPlayed",
        "MusicStarted",
        "MusicStopped",
        "StreamReady",
        "EventObservation",
        "EffectAdded",
        "EffectRemoved",
        "EffectChanged",
        "InventorySlotChanged",
        "SpellSlotChanged",
        "SkillSlotChanged",
        "StreamResyncRequired",
        "StreamClosed",
        "DiagnosticOptions",
        "SkillSlotOptions",
        "SkillNameOptions",
        "UseSkillOptions",
        "SlotSelector",
        "SwapSlotsOptions",
        "SpellTargetOptions",
        "CastSpellBySlot",
        "CastSpellByName",
        "CastSpellOptions",
        "GiveItemOptions",
        "GiveGoldOptions",
        "SkillUsed",
        "SpellBegin",
        "SpellChant",
        "SpellCast",
        "SpellCastArguments",
        "SpellCancelled",
        "SpellCancellationSource",
        "SpellSucceeded",
        "SpellFailed",
        "SpellFailureReason",
        "SpellReceived",
        "ReceivedSpellKind",
        "ExchangeSnapshot",
        "ExchangeState",
        "ExchangeOffer",
        "ExchangeItem",
        "ExchangeParty",
        "ExchangeOpened",
        "ExchangeItemAdded",
        "ExchangeGoldChanged",
        "ExchangeAccepted",
        "ExchangeCompleted",
        "ExchangeCancelled",
        "AddExchangeItemOptions",
        "SetExchangeGoldOptions",
        "CommandStatus",
        "CommandKind",
        "CommandState",
        "CommandFailure",
    ] {
        assert!(schemas.contains_key(name), "OpenAPI omitted {name}");
    }
    assert!(
        schemas["MessageChannel"]["enum"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == "chant"))
    );
    let message_parameters = openapi["paths"]["/clients/{client}/messages"]["get"]["parameters"]
        .as_array()
        .unwrap();
    let since = message_parameters
        .iter()
        .find(|parameter| parameter["name"] == "since")
        .expect("OpenAPI omitted the since parameter");
    assert_eq!(since["required"], false);
    assert_eq!(since["schema"]["format"], "date-time");
    let event_response = &openapi["paths"]["/clients/{client}/events"]["get"]["responses"]["200"];
    assert_eq!(
        event_response["content"]["text/event-stream"]["schema"]["$ref"],
        "#/components/schemas/ClientEvent"
    );
    let event_variants = schemas["ClientEvent"]["oneOf"].as_array().unwrap();
    for event_type in [
        "stream_ready",
        "client_logged_in",
        "client_disconnected",
        "sound_played",
        "music_started",
        "music_stopped",
        "effect_added",
        "effect_removed",
        "effect_changed",
        "message",
        "spell_succeeded",
        "spell_failed",
        "spell_received",
        "exchange_opened",
        "exchange_item_added",
        "exchange_gold_changed",
        "exchange_accepted",
        "exchange_completed",
        "exchange_cancelled",
    ] {
        assert!(event_variants.iter().any(|variant| {
            variant["properties"]["type"]["enum"]
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == event_type))
        }));
    }
    assert!(
        schemas["LaunchOptions"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "client_path")
    );
    assert!(schemas["LaunchOptions"]["properties"]["skip_exchange_alerts"].is_object());
    assert!(schemas["LoadResult"]["properties"]["was_loaded"].is_object());
    assert!(schemas["LoadResult"]["properties"]["changed"].is_null());
    assert!(schemas["UnloadResult"]["properties"]["was_unloaded"].is_object());
    assert!(schemas["UnloadResult"]["properties"]["changed"].is_null());
    assert_eq!(
        openapi["paths"]["/clients/{client}/skills/use"]["post"]["requestBody"]["content"]["application/json"]
            ["example"],
        serde_json::json!({"name": "Assail"})
    );
    assert!(
        schemas["CharacterStatus"]["properties"]
            .get("gender_id")
            .is_none()
    );
    assert!(
        schemas["CharacterStatus"]["properties"]
            .get("class_id")
            .is_none()
    );
    for collection in [
        "inventory",
        "equipment",
        "spellbook",
        "skillbook",
        "effects",
    ] {
        assert!(
            schemas["CharacterStatus"]["properties"]
                .get(collection)
                .is_none(),
            "CharacterStatus still exposes {collection}"
        );
    }
    assert!(
        schemas["CharacterModifiers"]["properties"]
            .get("attack_element_id")
            .is_none()
    );
    assert!(
        schemas["CharacterModifiers"]["properties"]
            .get("defense_element_id")
            .is_none()
    );
    assert!(
        schemas["Spell"]["properties"]
            .get("target_type_id")
            .is_none()
    );
    assert!(
        schemas["ClientLifecycle"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "disconnected")
    );

    let docs = response("/docs/");
    assert_eq!(docs.status(), StatusCode::OK);
    assert!(text("/docs/").contains("/docs/ayu.css"));
    let asset = response("/docs/assets/swagger-ui-bundle.js");
    assert_eq!(asset.status(), StatusCode::OK);
    let theme = text("/docs/ayu.css");
    assert!(theme.contains("--ayu-bg: #0b0e14"));
    assert!(theme.contains("--ayu-orange: #ffb454"));
    assert!(theme.contains(".swagger-ui .info .title small pre.version"));
    assert!(theme.contains(".swagger-ui button.model-box-control"));
    assert!(theme.contains(".swagger-ui .json-schema-2020-12-accordion"));
    assert!(theme.contains(".swagger-ui .opblock-summary-control:focus"));
    assert!(theme.contains(".swagger-ui .opblock .opblock-section-header h4"));
}

#[test]
fn delegates_typed_lifecycle_operations() {
    let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(state, "/clients/42/load", "");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let result = response_json(response);
    assert_eq!(result["was_loaded"], true);
    assert!(result.get("changed").is_none());
    assert!(matches!(
        receiver.recv().unwrap(),
        DaemonEvent::Status(ConnectionEvent::Initializing { pid: 42 })
    ));
    assert!(matches!(
        receiver.recv().unwrap(),
        DaemonEvent::Status(ConnectionEvent::Connecting { pid: 42 })
    ));

    let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(
        state,
        "/clients/launch",
        r#"{"client_path":"C:\\Darkages.exe","allow_multiple":true,"server":"127.0.0.1"}"#,
    );
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(matches!(receiver.recv().unwrap(), DaemonEvent::Track(77)));
}

#[test]
fn reports_no_transition_when_the_dll_is_already_in_the_requested_state() {
    let result = response_json(post_json(state(), "/clients/42/load", ""));
    assert_eq!(result["was_loaded"], false);
    assert!(result.get("changed").is_none());

    let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let result = response_json(post_json(state, "/clients/42/unload", ""));
    assert_eq!(result["was_unloaded"], false);
    assert!(result.get("changed").is_none());
}

#[test]
fn rejects_arbitrary_launch_arguments() {
    let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(
        state,
        "/clients/launch",
        r#"{"client_path":"C:\\Darkages.exe","arguments":["unsafe"]}"#,
    );
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn accepts_a_full_client_path_and_defaults_the_server_port() {
    let request: LaunchOptions = serde_json::from_str(
        r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"da0.kru.com","skip_exchange_alerts":true}"#,
    )
    .unwrap();
    let options = ManagedLaunchOptions::try_from(request).unwrap();
    assert_eq!(
        options.client_path,
        std::path::PathBuf::from(r"D:\Games\Dark Ages\Darkages.exe")
    );
    assert!(options.skip_exchange_alerts);
    let server = options.server.unwrap();
    assert_eq!(server.host, "da0.kru.com");
    assert_eq!(server.port, 2610);

    let request: LaunchOptions = serde_json::from_str(
        r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"127.0.0.1:3000"}"#,
    )
    .unwrap();
    let server = ManagedLaunchOptions::try_from(request)
        .unwrap()
        .server
        .unwrap();
    assert_eq!(server.host, "127.0.0.1");
    assert_eq!(server.port, 3000);

    assert!(serde_json::from_str::<LaunchOptions>("{}").is_err());
    assert!(
        serde_json::from_str::<LaunchOptions>(
            r#"{"client_path":"C:\\Darkages.exe","server":{"host":"da0.kru.com"}}"#
        )
        .is_err()
    );
    for field in ["loader_path", "dll_path"] {
        let body = format!(r#"{{"{field}":"unsafe"}}"#);
        assert!(serde_json::from_str::<LaunchOptions>(&body).is_err());
    }
}

#[test]
fn rejects_relative_client_paths() {
    let request: LaunchOptions = serde_json::from_str(r#"{"client_path":"Darkages.exe"}"#).unwrap();
    let error = ManagedLaunchOptions::try_from(request).unwrap_err();
    assert_eq!(error.body.error.code, "invalid_client_path");
}

#[test]
fn rejects_invalid_server_strings() {
    for server in ["", ":2610", "host:", "host:0", "host:nope", "::1"] {
        let request = LaunchOptions {
            client_path: r"C:\Darkages.exe".into(),
            allow_multiple: false,
            skip_exchange_alerts: false,
            skip_intro: false,
            skip_notice: false,
            server: Some(server.into()),
        };
        let error = ManagedLaunchOptions::try_from(request).unwrap_err();
        assert_eq!(error.body.error.code, "invalid_server");
    }
}

#[test]
fn rejects_request_bodies() {
    let response = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            router(state())
                .oneshot(
                    Request::get("/health")
                        .header("content-length", "1")
                        .body(Body::from("x"))
                        .unwrap(),
                )
                .await
                .unwrap()
        });
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn refuses_an_occupied_port() {
    let held = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let result = super::start(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port), state());
    assert!(result.is_err());
}
