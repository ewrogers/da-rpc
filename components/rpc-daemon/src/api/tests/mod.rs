mod commands;
mod documentation;
mod lifecycle;
mod resources;

use super::{ApiState, ClientList, LaunchOptions, resolve_client, router, start};
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
    http::{Request, StatusCode, header},
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
    LegendMark as ModelLegendMark, MapExclusions as ModelMapExclusions, MapLocation,
    MessageKind as ModelMessageKind, Nation as ModelNation, PlannedRoute as ModelPlannedRoute,
    PlayerEquipmentItem as ModelPlayerEquipmentItem, PlayerIdentity as ModelPlayerIdentity,
    PlayerProfile as ModelPlayerProfile, Skill as ModelSkill, Spell as ModelSpell,
    SpellCastArguments as ModelSpellCastArguments, SpellTargetType as ModelSpellTargetType,
    StateEvent, StateUpdate, TilePosition as ModelTilePosition, WorldObject as ModelWorldObject,
};
use darpc_protocol::{
    Architecture, ChantText, CommandKind, CommandOperation, CommandResult, CommandState,
    CommandStatus, ComponentVersion, DialogAction, DialogCommand, ExchangeCommand, GoldTransfer,
    GroupCommand, GroupText, Hello, ItemSlot, ItemTransfer, MessageCommand, MessageContent,
    MessageRecipient, PathExclusions, RawPacket, RawPacketDirection, RouteTile, SUPPORTED_VERSIONS,
    SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget,
    TilePosition, TransferTarget, WalkRoute, WalkTarget,
};
use serde_json::Value;
use std::{
    fs,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
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
            identity: None,
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
                        cooldown_ms: None,
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
                        cooldown_ms: None,
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
                        cooldown_ms: None,
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
                    cooldown_ms: Some(1_000),
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
                profile: None,
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
        planned_route: Some(ModelPlannedRoute {
            generation: 17,
            tiles: vec![
                ModelTilePosition { x: 11, y: 22 },
                ModelTilePosition { x: 12, y: 22 },
            ],
        }),
        map_exclusions: vec![ModelMapExclusions {
            map_id: 3001,
            tiles: vec![ModelTilePosition { x: 40, y: 50 }],
        }],
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

fn player_profile() -> ModelPlayerProfile {
    ModelPlayerProfile {
        identity: ModelPlayerIdentity {
            nation: ModelNation::Mileth,
            title: "Mentor".into(),
            guild_rank: "Leader".into(),
            display_class: "Summoner".into(),
            guild: "Guild".into(),
        },
        user_state: darpc_model::UserState::Grouped,
        is_group_open: true,
        equipment: vec![ModelPlayerEquipmentItem {
            slot: ModelEquipmentSlot::Necklace,
            sprite: 0x4321,
            dye_color: 4,
        }],
        legend: vec![],
        inspected_tick_ms: 120,
    }
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

fn empty_state() -> ApiState {
    let (events, _receiver) = mpsc::channel();
    ApiState::new(Registry::new().snapshot(), Arc::new(FakeLifecycle), events)
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

static NEXT_MAP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn map_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "darpc-map-download-{}-{}",
        std::process::id(),
        NEXT_MAP_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn downloads_configured_map_bytes_and_reports_missing_maps() {
    let directory = map_directory();
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("lod3001.map"), [0x00, 0x7f, 0x80, 0xff]).unwrap();
    let state = state().with_maps_directory(Some(directory.clone()));
    assert!(!state.set_maps_directory_if_unset("ignored".into()));

    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let response = router(state.clone())
                .oneshot(
                    Request::get("/maps/3001/download")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "application/octet-stream"
            );
            assert_eq!(
                response.headers()[header::CONTENT_DISPOSITION],
                "attachment; filename=\"lod3001.map\""
            );
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                [0x00, 0x7f, 0x80, 0xff]
            );

            let missing = router(state)
                .oneshot(
                    Request::get("/maps/3002/download")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(missing.status(), StatusCode::NOT_FOUND);
            let missing: Value =
                serde_json::from_slice(&to_bytes(missing.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert_eq!(missing["error"]["code"], "map_not_found");
        });

    fs::remove_dir_all(directory).unwrap();
    assert_eq!(
        response("/maps/3001/download").status(),
        StatusCode::NOT_FOUND
    );
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

fn put_json(state: ApiState, path: &str, body: &str) -> axum::response::Response {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            router(state)
                .oneshot(
                    Request::put(path)
                        .header("content-type", "application/json")
                        .header("content-length", body.len())
                        .body(Body::from(body.to_owned()))
                        .unwrap(),
                )
                .await
                .unwrap()
        })
}

fn delete_empty(state: ApiState, path: &str) -> axum::response::Response {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            router(state)
                .oneshot(Request::delete(path).body(Body::empty()).unwrap())
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

fn response_json(response: axum::response::Response) -> Value {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
}
