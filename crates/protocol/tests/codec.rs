use darpc_model::{
    AbilityUpdate, ActionSource, ActionUpdate, AudioUpdate, CharacterAppearance, CharacterClass,
    CharacterModifiers, CharacterProfileUpdate, CharacterProgression, CharacterSnapshot,
    CharacterStats, CharacterVitals, ClientCommand, ClientLifecycle, ClientMessage, ClientSnapshot,
    CollectionChange, CooldownStatus, CoreStatus, CreatureKind, CurrentVitals, DialogChoice,
    DialogInteraction, DialogKind, DialogNavigation, DialogSpeaker, DialogSpriteType, DialogState,
    DialogTarget, DialogUpdate, Direction, Effect, EffectDuration, EffectUpdate, Element,
    EntityUpdate, EquipmentItem, EquipmentSlot, ExchangeItem, ExchangeOffer, ExchangeParty,
    ExchangeState, ExchangeUpdate, FieldMapDestination, FieldMapSelection, FieldMapState,
    FieldMapUpdate, Gender, GroupInvitation, GroupMember, GroupState, GroupUpdate, HumanVisual,
    InventoryItem, LegendIcon, LegendMark, LegendUpdate, LifecycleUpdate, LocationUpdate,
    MapChange, MapDownload, MapDownloadUpdate, MapLocation, MessageDialog, MessageDialogsState,
    MessageKind, MovementStopReason, MovementUpdate, Nation, ObjectUpdate, PlannedRoute,
    PlayerEquipmentItem, PlayerIdentity, PlayerInspectionChanges, PlayerInspectionTrigger,
    PlayerProfile, PlayerUpdate, PlayerVisual, ProgressionStatus, Skill, SlotUpdate, Spell,
    SpellCancellationSource, SpellCastArguments, SpellTargetType, StateEvent, StateUpdate,
    StatusUpdate, TilePosition, UserState, WalkMode, WhoList, WhoPlayer, WorldObject,
};
use darpc_protocol::{
    Architecture, ChantText, CharacterStat, CommandFailure, CommandKind, CommandOperation,
    CommandRequest, CommandResponse, CommandResult, CommandState, CommandStatus, ComponentVersion,
    DecodeError, DiagnosticsMode, DiagnosticsOperation, DiagnosticsRequest, DiagnosticsResponse,
    DialogAction, DialogCommand, DialogText, EchoRequest, EchoResponse, EncodeError,
    EventPollRequest, EventPollResponse, EventPollResult, ExactRouteInvalidState,
    ExactRouteInvalidStateReason, ExchangeCommand, FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION,
    FieldMapSelectionCommand, Frame, FrameHeader, GoldTransfer, GroupCommand,
    GroupInvitationAction, GroupText, Hello, HelloAck, HookTimingRecord, HookTimingStage, ItemSlot,
    ItemTransfer, MAX_COMMAND_TIMEOUT_MS, MAX_COMMAND_WAIT_MS, MAX_ECHO_TEXT_LEN, MAX_PAYLOAD_LEN,
    Message, MessageCommand, MessageContent, MessageDialogCommand, MessageRecipient, MessageType,
    PROTOCOL_VERSION_1_0, Ping, Pong, RawPacket, RawPacketDirection, RouteTile, SkillSlot,
    SlotSwap, SnapshotRequest, SnapshotResponse, SnapshotResult, SnapshotUnavailableReason,
    SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, TickHealthRequest,
    TickHealthResponse, TilePosition as CommandTilePosition, TransferTarget, VersionRange,
    WalkRoute, WalkTarget, decode_frame, decode_header, encode_frame, protocol_version,
    protocol_version_major, protocol_version_minor,
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

fn player_visual() -> PlayerVisual {
    PlayerVisual::Human(HumanVisual {
        gender: Gender::Female,
        head_sprite: 101,
        body_sprite: 2,
        arms_sprite: 102,
        boots_sprite: 103,
        pants_sprite: 104,
        armor_sprite: 105,
        weapon_sprite: 106,
        shield_sprite: 107,
        overcoat_sprite: 108,
        accessory1_sprite: 109,
        accessory2_sprite: 110,
        accessory3_sprite: 111,
        hair_color: 3,
        skin_color: 4,
        boots_color: 5,
        pants_color: 6,
        overcoat_color: 7,
        accessory1_color: 8,
        accessory2_color: 9,
        accessory3_color: 10,
        rest_position: 11,
        face_shape: 12,
        is_translucent: false,
    })
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
            identity: None,
            appearance: Some(CharacterAppearance {
                gender: Gender::Male,
                hair_style: 17,
                hair_color: 6,
                body_sprite: 1,
            }),
            class: CharacterClass::Wizard,
            is_hidden: false,
            is_action_restricted: false,
            is_blinded: true,
            is_walking: false,
            movement_source: None,
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
                stat_points: 3,
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
                    cooldown_ms: None,
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
                    cooldown_ms: Some(1_000),
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
                is_hidden: false,
                visual: Some(player_visual()),
                profile: None,
            },
            WorldObject::Creature {
                id: 11,
                kind: CreatureKind::Monster,
                is_solid: false,
                sprite: Some(45),
                name: None,
                x: 41,
                y: 30,
                direction: Direction::West,
            },
            WorldObject::Item {
                id: 12,
                sprite: 7,
                dye_color: 2,
                x: 42,
                y: 30,
                z_index: 0,
            },
        ]),
        dialog: Some(dialog_state()),
        active_field_map: Some(field_map_state()),
        message_dialogs: MessageDialogsState {
            revision: 8,
            dialogs: vec![MessageDialog {
                id: 3,
                text: Some("You sense danger nearby.".into()),
                truncated: false,
            }],
        },
        active_bulletin: None,
        group: Some(group_state()),
        exchange: Some(exchange_state()),
        legend: Some(vec![LegendMark {
            text: "Found the hidden grove".into(),
            tag: "Quest".into(),
            color: 7,
            icon: LegendIcon::Wizard,
        }]),
        planned_route: Some(PlannedRoute {
            source: ActionSource::Command { command_id: 41 },
            generation: 42,
            tiles: vec![
                TilePosition { x: 11, y: 22 },
                TilePosition { x: 12, y: 22 },
                TilePosition { x: 12, y: 23 },
            ],
        }),
    }
}

fn player_profile() -> PlayerProfile {
    PlayerProfile {
        identity: PlayerIdentity {
            nation: Nation::Mileth,
            title: "Mentor".into(),
            guild_rank: "Leader".into(),
            display_class: "Summoner".into(),
            guild: "Guild".into(),
        },
        user_state: UserState::Grouped,
        is_group_open: true,
        equipment: vec![PlayerEquipmentItem {
            slot: EquipmentSlot::Necklace,
            sprite: 0x4321,
            dye_color: 4,
        }],
        legend: vec![LegendMark {
            text: "Found the grove".into(),
            tag: "Quest".into(),
            color: 3,
            icon: LegendIcon::Aisling,
        }],
        inspected_tick_ms: 145,
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

fn field_map_state() -> FieldMapState {
    FieldMapState {
        revision: 14,
        field_name: "field001".into(),
        current_node_index: Some(0),
        destinations: vec![
            FieldMapDestination {
                index: 0,
                screen_x: 125,
                screen_y: 90,
                name: "Mileth".into(),
                checksum: 0x1234,
                map_id: 100,
                map_x: 10,
                map_y: 20,
            },
            FieldMapDestination {
                index: 1,
                screen_x: 250,
                screen_y: 180,
                name: "Suomi".into(),
                checksum: 0x5678,
                map_id: 200,
                map_x: 30,
                map_y: 40,
            },
        ],
        selection: Some(FieldMapSelection {
            destination_index: 1,
        }),
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
