use darpc_model::{
    AbilityUpdate, ActionUpdate, CharacterAppearance, CharacterClass, CharacterModifiers,
    CharacterProgression, CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle,
    ClientMessage, ClientSnapshot, CollectionChange, CooldownStatus, CoreStatus, CreatureKind,
    CurrentVitals, DialogChoice, DialogInteraction, DialogKind, DialogNavigation, DialogSpeaker,
    DialogSpriteType, DialogState, DialogTarget, DialogUpdate, Direction, Effect, EffectDuration,
    EffectUpdate, Element, EntityUpdate, EquipmentItem, EquipmentSlot, ExchangeItem, ExchangeOffer,
    ExchangeParty, ExchangeState, ExchangeUpdate, Gender, GroupInvitation, GroupMember, GroupState,
    GroupUpdate, InventoryItem, LegendIcon, LegendMark, LegendUpdate, LocationUpdate, MapChange,
    MapLocation, MessageKind, MovementUpdate, ObjectUpdate, ProgressionStatus, Skill, SlotUpdate,
    Spell, SpellCancellationSource, SpellCastArguments, SpellTargetType, StateEvent, StateUpdate,
    StatusUpdate, TilePosition, UserState, WhoList, WhoPlayer, WorldObject,
};
use darpc_protocol::{
    Architecture, ChantText, CommandFailure, CommandKind, CommandOperation, CommandRequest,
    CommandResponse, CommandResult, CommandState, CommandStatus, ComponentVersion, DecodeError,
    DialogAction, DialogCommand, DialogText, EchoRequest, EchoResponse, EncodeError,
    EventPollRequest, EventPollResponse, EventPollResult, ExchangeCommand, FRAME_HEADER_LEN,
    FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, GoldTransfer, GroupCommand,
    GroupInvitationAction, GroupText, Hello, HelloAck, ItemSlot, ItemTransfer,
    MAX_COMMAND_TIMEOUT_MS, MAX_COMMAND_WAIT_MS, MAX_ECHO_TEXT_LEN, MAX_PAYLOAD_LEN, Message,
    MessageType, PROTOCOL_VERSION_1_0, Ping, Pong, SkillSlot, SlotSwap, SnapshotRequest,
    SnapshotResponse, SnapshotResult, SnapshotUnavailableReason, SpellArguments, SpellCast,
    SpellInput, SpellSlot, SpellTarget, TickHealthRequest, TickHealthResponse,
    TilePosition as CommandTilePosition, TransferTarget, VersionRange, WalkTarget, decode_frame,
    decode_header, encode_frame, protocol_version, protocol_version_major, protocol_version_minor,
};

fn hello() -> Hello {
    Hello {
        protocol_versions: VersionRange {
            min: PROTOCOL_VERSION_1_0,
            max: PROTOCOL_VERSION_1_0,
        },
        dll_instance_id: [0x5a; 16],
        process_id: 42,
        process_creation_time: 0x1122_3344_5566_7788,
        architecture: Architecture::X86,
        dll_version: ComponentVersion {
            major: 1,
            minor: 2,
            patch: 3,
        },
        executable_fingerprint: [0xa5; 32],
        client_version: 741,
    }
}

fn snapshot() -> ClientSnapshot {
    ClientSnapshot {
        revision: 9,
        event_sequence: 7,
        captured_tick_ms: u32::MAX,
        updated_tick_ms: 12,
        capture_duration_us: 321,
        world_generation: 4,
        lifecycle: ClientLifecycle::InGame,
        character: Some(CharacterSnapshot {
            id: Some(0x1122_3344),
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
            gold: 123_456,
            weight: 88,
            max_weight: 120,
            progression: CharacterProgression {
                level: 99,
                ability_level: 7,
                experience: 8_000_000,
                ability_points: Some(66_000),
                experience_to_next_level: Some(44_000),
                ability_to_next_level: Some(55_000),
            },
            stats: CharacterStats {
                strength: 30,
                intelligence: 34,
                wisdom: 32,
                constitution: 33,
                dexterity: 31,
            },
            vitals: CharacterVitals {
                health: 1_000,
                max_health: 1_100,
                mana: 900,
                max_mana: 950,
            },
            modifiers: Some(CharacterModifiers {
                armor_class: -7,
                damage: 8,
                hit: 9,
                magic_resistance: 30,
                attack_element: Element::Fire,
                defense_element: Element::Water,
            }),
            location: Some(MapLocation {
                id: 3001,
                name: None,
                x: Some(11),
                y: Some(22),
                width: 100,
                height: 80,
            }),
            inventory: Some(vec![InventoryItem {
                slot: 1,
                sprite: 0x0123,
                dye_color: 7,
                name: Some("Dark Belt".into()),
                quantity: 3,
                can_stack: true,
                durability: 41,
                max_durability: 50,
            }]),
            equipment: Some(vec![EquipmentItem {
                slot: EquipmentSlot::Armor,
                sprite: 0x1234,
                dye_color: 2,
                name: Some("Hy-Brasyl Armor".into()),
                durability: 900,
                max_durability: 1_000,
            }]),
            spellbook: Some(vec![Spell {
                slot: 7,
                icon: 0x0456,
                name: Some("Fas Spiorad".into()),
                level: 3,
                max_level: 5,
                lines: 4,
                target_type: SpellTargetType::TextInput,
                prompt: Some("Who?".into()),
                cooldown: CooldownStatus {
                    active: true,
                    remaining_ms: None,
                },
            }]),
            skillbook: Some(vec![Skill {
                slot: 4,
                icon: 0x0123,
                name: Some("Assail".into()),
                level: 10,
                max_level: 100,
                cooldown: CooldownStatus {
                    active: true,
                    remaining_ms: Some(750),
                },
            }]),
            effects: Some(vec![Effect {
                icon: 300,
                duration: EffectDuration::White,
            }]),
        }),
        objects: Some(vec![
            WorldObject::Player {
                id: 10,
                name: Some("Eidolon".into()),
                x: 40,
                y: 30,
                direction: Direction::East,
            },
            WorldObject::Creature {
                id: 11,
                kind: CreatureKind::Monster,
                sprite: Some(45),
                name: None,
                x: 41,
                y: 30,
                direction: Direction::West,
            },
            WorldObject::Item {
                id: 12,
                sprite: 7,
                x: 42,
                y: 30,
                z_index: 0,
            },
        ]),
        dialog: Some(dialog_state()),
        group: Some(group_state()),
        exchange: Some(exchange_state()),
        legend: Some(vec![LegendMark {
            text: "Found the hidden grove".into(),
            tag: "Quest".into(),
            color: 7,
            icon: LegendIcon::Wizard,
        }]),
    }
}

fn group_state() -> GroupState {
    GroupState {
        members: vec![
            GroupMember {
                name: "Eidolon".into(),
                is_leader: true,
            },
            GroupMember {
                name: "ZiLo".into(),
                is_leader: false,
            },
        ],
        invitations: vec![GroupInvitation {
            id: 3,
            inviter: "Intern".into(),
            received_tick_ms: Some(456),
        }],
        is_group_open: Some(true),
        auto_accept: Some(false),
    }
}

fn exchange_state() -> ExchangeState {
    ExchangeState {
        id: 0x1122_3344,
        partner: "ZiLo".into(),
        local: ExchangeOffer {
            items: vec![ExchangeItem {
                index: 0,
                sprite: 321,
                dye_color: 4,
                quantity: Some(3),
                name: "Wine".into(),
            }],
            gold: 1_000,
            accepted: false,
        },
        other: ExchangeOffer::default(),
    }
}

fn dialog_state() -> DialogState {
    DialogState {
        revision: 7,
        kind: DialogKind::Pursuit,
        target: DialogTarget { id: 0x1122_3344 },
        speaker: DialogSpeaker {
            name: Some("Beggar".into()),
            sprite: 42,
            sprite_type: DialogSpriteType::Creature,
            color: 3,
            show_graphic: true,
        },
        content: Some("What do you seek?".into()),
        response_pending: false,
        navigation: DialogNavigation {
            previous: false,
            next: true,
            close: true,
        },
        interaction: DialogInteraction::Choices(vec![DialogChoice {
            index: 0,
            text: "A small task".into(),
        }]),
    }
}

fn frame_for(message_type: MessageType, payload: &[u8]) -> Vec<u8> {
    let payload_len = u32::try_from(payload.len()).unwrap();
    let mut bytes = header_with_payload_len(message_type.wire_value(), payload_len);
    bytes.extend_from_slice(payload);
    bytes
}

fn header_with_payload_len(message_type: u16, payload_len: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_HEADER_LEN);
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.extend_from_slice(&FRAME_VERSION.to_le_bytes());
    bytes.extend_from_slice(&message_type.to_le_bytes());
    bytes.extend_from_slice(&7_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&123_u32.to_le_bytes());
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes
}

#[path = "codec/basic.rs"]
mod basic;
#[path = "codec/command.rs"]
mod command;
#[path = "codec/command_round_trip.rs"]
mod command_round_trip;
#[path = "codec/event.rs"]
mod event;
#[path = "codec/event_round_trip.rs"]
mod event_round_trip;
#[path = "codec/frame.rs"]
mod frame;
#[path = "codec/snapshot.rs"]
mod snapshot;
