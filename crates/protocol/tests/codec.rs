use darpc_model::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, CoreStatus, CreatureKind, CurrentVitals, Direction, Effect, EffectDuration,
    EffectUpdate, Element, EquipmentItem, EquipmentSlot, Gender, InventoryItem, LocationUpdate,
    MapChange, MapLocation, ObjectUpdate, ProgressionStatus, Skill, Spell, SpellTargetType,
    StateEvent, StateUpdate, StatusUpdate, WorldObject,
};
use darpc_protocol::{
    Architecture, CommandFailure, CommandKind, CommandOperation, CommandRequest, CommandResponse,
    CommandResult, CommandState, CommandStatus, ComponentVersion, DecodeError, EchoRequest,
    EchoResponse, EncodeError, EventPollRequest, EventPollResponse, EventPollResult,
    FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, Hello, HelloAck,
    MAX_COMMAND_TIMEOUT_MS, MAX_COMMAND_WAIT_MS, MAX_ECHO_TEXT_LEN, MAX_PAYLOAD_LEN, Message,
    MessageType, PROTOCOL_VERSION_1_0, Ping, Pong, SnapshotRequest, SnapshotResponse,
    SnapshotResult, SnapshotUnavailableReason, TickHealthRequest, TickHealthResponse, VersionRange,
    decode_frame, decode_header, encode_frame, protocol_version, protocol_version_major,
    protocol_version_minor,
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

#[test]
fn every_message_round_trips() {
    let mut disconnected = snapshot();
    disconnected.lifecycle = ClientLifecycle::Disconnected;
    let messages = [
        Message::Hello(hello()),
        Message::HelloAck(HelloAck {
            selected_version: PROTOCOL_VERSION_1_0,
            dll_instance_id: [0x5a; 16],
        }),
        Message::Ping(Ping { request_id: 1 }),
        Message::Pong(Pong { request_id: 2 }),
        Message::EchoRequest(EchoRequest {
            request_id: 3,
            text: "hello".into(),
        }),
        Message::EchoResponse(EchoResponse {
            request_id: 4,
            text: "world".into(),
        }),
        Message::TickHealthRequest(TickHealthRequest { request_id: 5 }),
        Message::TickHealthResponse(TickHealthResponse {
            request_id: 6,
            installed: true,
            relocated_bytes: 5,
            tick_count: u32::MAX,
        }),
        Message::SnapshotRequest(SnapshotRequest { request_id: 7 }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 8,
            result: SnapshotResult::Ready(Box::new(snapshot())),
        }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 9,
            result: SnapshotResult::Unavailable(SnapshotUnavailableReason::CaptureTimedOut),
        }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 10,
            result: SnapshotResult::Ready(Box::new(disconnected)),
        }),
        Message::EventPollRequest(EventPollRequest {
            request_id: 11,
            after_sequence: 40,
            max_events: 64,
            wait_ms: 50,
        }),
        Message::EventPollResponse(EventPollResponse {
            request_id: 12,
            result: EventPollResult::Events(vec![
                StateEvent {
                    sequence: 41,
                    revision: 10,
                    tick_ms: 123,
                    update: StateUpdate::Status(StatusUpdate {
                        core: Some(CoreStatus {
                            level: 99,
                            ability_level: 12,
                            max_health: 2_000,
                            max_mana: 1_500,
                            weight: 88,
                            max_weight: 120,
                            stats: CharacterStats {
                                strength: 11,
                                intelligence: 12,
                                wisdom: 13,
                                constitution: 14,
                                dexterity: 15,
                            },
                        }),
                        vitals: Some(CurrentVitals {
                            health: 1_900,
                            mana: 1_400,
                        }),
                        progression: Some(ProgressionStatus {
                            experience: 100,
                            ability_points: 200,
                            experience_to_next_level: 300,
                            ability_to_next_level: 400,
                        }),
                        gold: Some(500),
                        modifiers: Some(CharacterModifiers {
                            armor_class: -10,
                            damage: 8,
                            hit: 7,
                            magic_resistance: 60,
                            attack_element: Element::Fire,
                            defense_element: Element::Water,
                        }),
                        is_blinded: Some(true),
                        is_action_restricted: Some(true),
                    }),
                },
                StateEvent {
                    sequence: 42,
                    revision: 11,
                    tick_ms: 124,
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
                StateEvent {
                    sequence: 43,
                    revision: 12,
                    tick_ms: 125,
                    update: StateUpdate::Effect(EffectUpdate::Added(Effect {
                        icon: 300,
                        duration: EffectDuration::White,
                    })),
                },
                StateEvent {
                    sequence: 44,
                    revision: 13,
                    tick_ms: 126,
                    update: StateUpdate::Effect(EffectUpdate::Changed(Effect {
                        icon: 300,
                        duration: EffectDuration::Red,
                    })),
                },
                StateEvent {
                    sequence: 45,
                    revision: 14,
                    tick_ms: 127,
                    update: StateUpdate::Effect(EffectUpdate::Removed { icon: 300 }),
                },
                StateEvent {
                    sequence: 46,
                    revision: 15,
                    tick_ms: 128,
                    update: StateUpdate::Object(ObjectUpdate::Moved(WorldObject::Player {
                        id: 10,
                        name: Some("Eidolon".into()),
                        x: 41,
                        y: 30,
                        direction: Direction::East,
                    })),
                },
            ]),
        }),
        Message::EventPollResponse(EventPollResponse {
            request_id: 13,
            result: EventPollResult::ResyncRequired {
                missing_sequence: 42,
                latest_sequence: 900,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 14,
            operation: CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 15,
            operation: CommandOperation::Query {
                command_id: 91,
                wait_ms: 0,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 16,
            operation: CommandOperation::Cancel { command_id: 91 },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 17,
            result: CommandResult::Status(CommandStatus {
                command_id: 91,
                kind: CommandKind::Diagnostic,
                state: CommandState::Failed,
                enqueued_tick_ms: u32::MAX - 5,
                deadline_tick_ms: 994,
                started_tick_ms: Some(2),
                completed_tick_ms: Some(3),
                execution_us: Some(17),
                main_thread_id: Some(42),
                failure: Some(CommandFailure::Internal),
            }),
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 18,
            result: CommandResult::Busy,
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 19,
            result: CommandResult::Unavailable,
        }),
    ];

    for message in messages {
        let frame = Frame::new(7, 123, message);
        let bytes = encode_frame(&frame).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), frame);
    }
}

#[test]
fn command_limits_are_strictly_validated() {
    let invalid_timeout = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: MAX_COMMAND_TIMEOUT_MS + 1,
            wait_ms: 0,
        },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_timeout)),
        Err(EncodeError::InvalidCommandTimeout {
            actual: MAX_COMMAND_TIMEOUT_MS + 1,
            max: MAX_COMMAND_TIMEOUT_MS,
        })
    );

    let invalid_wait = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Query {
            command_id: 1,
            wait_ms: MAX_COMMAND_WAIT_MS + 1,
        },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_wait)),
        Err(EncodeError::InvalidCommandWait {
            actual: MAX_COMMAND_WAIT_MS + 1,
            max: MAX_COMMAND_WAIT_MS,
        })
    );

    let invalid_id = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Cancel { command_id: 0 },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_id)),
        Err(EncodeError::InvalidCommandId)
    );
}

#[test]
fn snapshot_collections_are_strictly_validated() {
    let mut invalid_slot = snapshot();
    invalid_slot
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap()[0]
        .slot = 0;
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(invalid_slot)),
            }),
        )),
        Err(EncodeError::InvalidSnapshotSlot { slot: 0, max: 60 })
    );

    let mut duplicate_slot = snapshot();
    let inventory = duplicate_slot
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap();
    inventory.push(inventory[0].clone());
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(duplicate_slot)),
            }),
        )),
        Err(EncodeError::DuplicateSnapshotSlot { slot: 1 })
    );

    let mut oversized = snapshot();
    let item = oversized
        .character
        .as_ref()
        .unwrap()
        .inventory
        .as_ref()
        .unwrap()[0]
        .clone();
    oversized.character.as_mut().unwrap().inventory = Some(vec![item; 61]);
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(oversized)),
            }),
        )),
        Err(EncodeError::SnapshotCollectionTooLong {
            length: 61,
            max: 60,
        })
    );

    let mut duplicate_effect = snapshot();
    let effects = duplicate_effect
        .character
        .as_mut()
        .unwrap()
        .effects
        .as_mut()
        .unwrap();
    effects.push(effects[0]);
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(duplicate_effect)),
            }),
        )),
        Err(EncodeError::DuplicateEffectIcon { icon: 300 })
    );

    let mut long_name = snapshot();
    long_name
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap()[0]
        .name = Some("x".repeat(128));
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(long_name)),
            }),
        )),
        Err(EncodeError::SnapshotStringTooLong {
            length: 128,
            max: 127,
        })
    );
}

#[test]
fn malformed_snapshot_slots_are_rejected_when_decoding() {
    let frame = Frame::new(
        0,
        0,
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 1,
            result: SnapshotResult::Ready(Box::new(snapshot())),
        }),
    );
    let mut invalid = encode_frame(&frame).unwrap();
    let slot = invalid
        .windows(4)
        .position(|bytes| bytes == [1, 0x23, 0x01, 7])
        .expect("inventory marker is unique");
    invalid[slot] = 0;
    assert_eq!(
        decode_frame(&invalid),
        Err(DecodeError::InvalidSnapshotSlot { slot: 0, max: 60 })
    );

    let mut duplicate = snapshot();
    let inventory = duplicate
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap();
    let mut second = inventory[0].clone();
    second.slot = 2;
    second.sprite = 0xdead;
    inventory.push(second);
    let mut duplicate = encode_frame(&Frame::new(
        0,
        0,
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 1,
            result: SnapshotResult::Ready(Box::new(duplicate)),
        }),
    ))
    .unwrap();
    let slot = duplicate
        .windows(3)
        .position(|bytes| bytes == [2, 0xad, 0xde])
        .expect("second inventory marker is unique");
    duplicate[slot] = 1;
    assert_eq!(
        decode_frame(&duplicate),
        Err(DecodeError::DuplicateSnapshotSlot { slot: 1 })
    );

    let mut invalid_duration = encode_frame(&frame).unwrap();
    let duration = invalid_duration
        .windows(3)
        .rposition(|bytes| bytes == [0x2c, 0x01, 6])
        .expect("effect marker is present")
        + 2;
    invalid_duration[duration] = 7;
    assert_eq!(
        decode_frame(&invalid_duration),
        Err(DecodeError::InvalidEffectDuration { actual: 7 })
    );
}

#[test]
fn protocol_version_packs_major_and_minor_bytes() {
    let version = protocol_version(1, 2);

    assert_eq!(version, 0x0102);
    assert_eq!(protocol_version_major(version), 1);
    assert_eq!(protocol_version_minor(version), 2);
}

#[test]
fn header_can_be_validated_before_reading_the_payload() {
    let bytes = encode_frame(&Frame::new(7, 123, Message::Ping(Ping { request_id: 9 }))).unwrap();
    let header = decode_header(&bytes[..FRAME_HEADER_LEN]).unwrap();

    assert_eq!(header.message_type, MessageType::Ping);
    assert_eq!(header.sequence, 7);
    assert_eq!(header.sender_tick_ms, 123);
    assert_eq!(header.payload_len, 4);
    assert_eq!(header.frame_len().unwrap(), FRAME_HEADER_LEN + 4);
}

#[test]
fn every_truncated_prefix_is_rejected() {
    let bytes = encode_frame(&Frame::new(7, 123, Message::Hello(hello()))).unwrap();

    for length in 0..bytes.len() {
        assert!(
            decode_frame(&bytes[..length]).is_err(),
            "prefix {length} decoded"
        );
    }
}

#[test]
fn oversized_payload_length_is_rejected_from_the_header() {
    let payload_len = u32::try_from(MAX_PAYLOAD_LEN + 1).unwrap();
    let bytes = header_with_payload_len(MessageType::Ping.wire_value(), payload_len);

    assert_eq!(
        decode_header(&bytes),
        Err(DecodeError::PayloadTooLarge {
            length: MAX_PAYLOAD_LEN + 1,
            max: MAX_PAYLOAD_LEN,
        })
    );
}

#[test]
fn hostile_u32_payload_length_is_rejected_without_allocation() {
    let bytes = header_with_payload_len(MessageType::Ping.wire_value(), u32::MAX);

    assert!(matches!(
        decode_header(&bytes),
        Err(DecodeError::PayloadTooLarge { .. })
    ));
}

#[test]
fn frame_length_arithmetic_is_checked() {
    let header = FrameHeader {
        message_type: MessageType::Ping,
        sequence: 0,
        sender_tick_ms: 0,
        payload_len: usize::MAX,
    };

    assert_eq!(header.frame_len(), Err(DecodeError::LengthOverflow));
}

#[test]
fn trailing_frame_bytes_are_rejected() {
    let mut bytes = encode_frame(&Frame::new(0, 0, Message::Ping(Ping { request_id: 1 }))).unwrap();
    bytes.push(0);

    assert!(matches!(
        decode_frame(&bytes),
        Err(DecodeError::TrailingFrameBytes { .. })
    ));
}

#[test]
fn malformed_headers_are_rejected() {
    let valid = header_with_payload_len(MessageType::Ping.wire_value(), 0);

    let mut invalid_magic = valid.clone();
    invalid_magic[0] = b'X';
    assert!(matches!(
        decode_header(&invalid_magic),
        Err(DecodeError::InvalidMagic { .. })
    ));

    let mut invalid_version = valid.clone();
    invalid_version[4..6].copy_from_slice(&2_u16.to_le_bytes());
    assert_eq!(
        decode_header(&invalid_version),
        Err(DecodeError::UnsupportedFrameVersion { actual: 2 })
    );

    let mut unknown_message = valid.clone();
    unknown_message[6..8].copy_from_slice(&99_u16.to_le_bytes());
    assert_eq!(
        decode_header(&unknown_message),
        Err(DecodeError::UnknownMessageType { actual: 99 })
    );

    let mut nonzero_flags = valid;
    nonzero_flags[10..12].copy_from_slice(&1_u16.to_le_bytes());
    assert_eq!(
        decode_header(&nonzero_flags),
        Err(DecodeError::NonZeroFlags { actual: 1 })
    );
}

#[test]
fn fixed_payload_sizes_are_exact() {
    let short = frame_for(MessageType::Ping, &[0; 3]);
    assert!(matches!(
        decode_frame(&short),
        Err(DecodeError::TruncatedMessage { .. })
    ));

    let long = frame_for(MessageType::Ping, &[0; 5]);
    assert!(matches!(
        decode_frame(&long),
        Err(DecodeError::TrailingMessageBytes { .. })
    ));

    let invalid_boolean = frame_for(
        MessageType::TickHealthResponse,
        &[1, 0, 0, 0, 2, 5, 9, 0, 0, 0],
    );
    assert_eq!(
        decode_frame(&invalid_boolean),
        Err(DecodeError::InvalidBoolean { actual: 2 })
    );
}

#[test]
fn invalid_hello_fields_are_rejected() {
    let mut invalid_range = encode_frame(&Frame::new(0, 0, Message::Hello(hello()))).unwrap();
    invalid_range[20..24].copy_from_slice(&[0x01, 0x01, 0x00, 0x01]);
    assert_eq!(
        decode_frame(&invalid_range),
        Err(DecodeError::InvalidVersionRange {
            min: 0x0101,
            max: 0x0100,
        })
    );

    let mut invalid_architecture =
        encode_frame(&Frame::new(0, 0, Message::Hello(hello()))).unwrap();
    invalid_architecture[52] = 99;
    assert_eq!(
        decode_frame(&invalid_architecture),
        Err(DecodeError::InvalidArchitecture { actual: 99 })
    );
}

#[test]
fn invalid_hello_range_is_rejected_when_encoding() {
    let mut message = hello();
    message.protocol_versions = VersionRange {
        min: 0,
        max: PROTOCOL_VERSION_1_0,
    };

    assert_eq!(
        encode_frame(&Frame::new(0, 0, Message::Hello(message))),
        Err(EncodeError::InvalidVersionRange {
            min: 0,
            max: PROTOCOL_VERSION_1_0,
        })
    );
}

#[test]
fn echo_requires_bounded_utf8() {
    let invalid_utf8 = frame_for(MessageType::EchoRequest, &[1, 0, 0, 0, 1, 0, 0xff]);
    assert_eq!(decode_frame(&invalid_utf8), Err(DecodeError::InvalidUtf8));

    let too_long = "a".repeat(MAX_ECHO_TEXT_LEN + 1);
    let message = Message::EchoRequest(EchoRequest {
        request_id: 1,
        text: too_long,
    });
    assert!(matches!(
        encode_frame(&Frame::new(0, 0, message)),
        Err(EncodeError::EchoTooLong { .. })
    ));

    let mut payload = vec![0; 6 + MAX_ECHO_TEXT_LEN + 1];
    payload[4..6].copy_from_slice(&u16::try_from(MAX_ECHO_TEXT_LEN + 1).unwrap().to_le_bytes());
    let oversized = frame_for(MessageType::EchoResponse, &payload);
    assert!(matches!(
        decode_frame(&oversized),
        Err(DecodeError::EchoTooLong { .. })
    ));
}
