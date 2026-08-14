use super::*;

#[test]
fn command_messages_round_trip() {
    assert!(PathExclusions::new(65_536, &[RouteTile { x: 1, y: 1 }]).is_none());
    assert!(PathExclusions::new(1, &[RouteTile { x: 400, y: 1 }]).is_none());

    let messages = [
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
            operation: CommandOperation::Submit {
                kind: CommandKind::Turn(Direction::West),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 16,
            operation: CommandOperation::Submit {
                kind: CommandKind::Walk(WalkTarget::Direction(Direction::North)),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 17,
            operation: CommandOperation::Submit {
                kind: CommandKind::Walk(WalkTarget::Destination { x: 120, y: 85 }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 170,
            operation: CommandOperation::Submit {
                kind: CommandKind::Walk(WalkTarget::Route(
                    WalkRoute::new(
                        3000,
                        &[
                            RouteTile { x: 10, y: 20 },
                            RouteTile { x: 11, y: 20 },
                            RouteTile { x: 11, y: 21 },
                        ],
                    )
                    .unwrap(),
                )),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 171,
            operation: CommandOperation::Submit {
                kind: CommandKind::SetPathExclusions(
                    PathExclusions::new(
                        3000,
                        &[RouteTile { x: 40, y: 50 }, RouteTile { x: 41, y: 50 }],
                    )
                    .unwrap(),
                ),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 172,
            operation: CommandOperation::Submit {
                kind: CommandKind::RemovePathExclusions { map_id: 3000 },
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 173,
            operation: CommandOperation::Submit {
                kind: CommandKind::ClearPathExclusions,
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 18,
            operation: CommandOperation::Submit {
                kind: CommandKind::UseSkill(SkillSlot::new(7).unwrap()),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 19,
            operation: CommandOperation::Query {
                command_id: 91,
                wait_ms: 0,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 20,
            operation: CommandOperation::Cancel { command_id: 91 },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 21,
            result: CommandResult::Status(CommandStatus {
                command_id: 91,
                kind: CommandKind::Walk(WalkTarget::Destination { x: 120, y: 85 }),
                state: CommandState::Failed,
                enqueued_tick_ms: u32::MAX - 5,
                deadline_tick_ms: 994,
                started_tick_ms: Some(2),
                completed_tick_ms: Some(3),
                execution_us: Some(17),
                main_thread_id: Some(42),
                failure: Some(CommandFailure::NoPath),
            }),
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 234,
            operation: CommandOperation::Submit {
                kind: CommandKind::InspectPlayer(std::num::NonZeroU32::new(77).unwrap()),
                timeout_ms: 3_000,
                wait_ms: 1_000,
            },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 235,
            result: CommandResult::Player {
                status: CommandStatus {
                    command_id: 94,
                    kind: CommandKind::InspectPlayer(std::num::NonZeroU32::new(77).unwrap()),
                    state: CommandState::Executed,
                    enqueued_tick_ms: 140,
                    deadline_tick_ms: 3_140,
                    started_tick_ms: Some(141),
                    completed_tick_ms: Some(145),
                    execution_us: Some(9),
                    main_thread_id: Some(42),
                    failure: None,
                },
                player: Box::new(WorldObject::Player {
                    id: 77,
                    name: Some("Eidolon".into()),
                    x: 120,
                    y: 85,
                    direction: Direction::South,
                    is_hidden: false,
                    profile: Some(Box::new(player_profile())),
                }),
            },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 22,
            result: CommandResult::Busy,
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 23,
            result: CommandResult::Unavailable,
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 230,
            operation: CommandOperation::Submit {
                kind: CommandKind::Who,
                timeout_ms: 3_000,
                wait_ms: 1_000,
            },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 231,
            result: CommandResult::Who {
                status: CommandStatus {
                    command_id: 92,
                    kind: CommandKind::Who,
                    state: CommandState::Executed,
                    enqueued_tick_ms: 100,
                    deadline_tick_ms: 3_100,
                    started_tick_ms: Some(101),
                    completed_tick_ms: Some(120),
                    execution_us: Some(20),
                    main_thread_id: Some(42),
                    failure: None,
                },
                list: WhoList {
                    world_count: 100,
                    country_count: 2,
                    players: vec![
                        WhoPlayer {
                            name: "ZiLo".into(),
                            title: "Aisling".into(),
                            class: CharacterClass::Priest,
                            state: UserState::NeedGroup,
                            color: 3,
                            is_master: true,
                            is_guildmate: true,
                        },
                        WhoPlayer {
                            name: "Eidolon".into(),
                            title: String::new(),
                            class: CharacterClass::Rogue,
                            state: UserState::Awake,
                            color: 0,
                            is_master: false,
                            is_guildmate: false,
                        },
                    ],
                },
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 232,
            operation: CommandOperation::Submit {
                kind: CommandKind::Legend,
                timeout_ms: 3_000,
                wait_ms: 1_000,
            },
        }),
        Message::CommandResponse(CommandResponse {
            request_id: 233,
            result: CommandResult::Legend {
                status: CommandStatus {
                    command_id: 93,
                    kind: CommandKind::Legend,
                    state: CommandState::Executed,
                    enqueued_tick_ms: 130,
                    deadline_tick_ms: 3_130,
                    started_tick_ms: Some(131),
                    completed_tick_ms: Some(140),
                    execution_us: Some(8),
                    main_thread_id: Some(42),
                    failure: None,
                },
                marks: vec![LegendMark {
                    text: "Found the hidden grove".into(),
                    tag: "Quest".into(),
                    color: 7,
                    icon: LegendIcon::Wizard,
                }],
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 24,
            operation: CommandOperation::Submit {
                kind: CommandKind::CastSpell(SpellCast {
                    slot: SpellSlot::new(4).unwrap(),
                    arguments: SpellArguments::Target(SpellTarget::Object(
                        std::num::NonZeroU32::new(0x1122_3344).unwrap(),
                    )),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 25,
            operation: CommandOperation::Submit {
                kind: CommandKind::CastSpell(SpellCast {
                    slot: SpellSlot::new(5).unwrap(),
                    arguments: SpellArguments::Target(SpellTarget::Tile { x: 30, y: 40 }),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 26,
            operation: CommandOperation::Submit {
                kind: CommandKind::CastSpell(SpellCast {
                    slot: SpellSlot::new(6).unwrap(),
                    arguments: SpellArguments::Input(SpellInput::new("Eidolon").unwrap()),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 27,
            operation: CommandOperation::Submit {
                kind: CommandKind::UseItem(ItemSlot::new(12).unwrap()),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 28,
            operation: CommandOperation::Submit {
                kind: CommandKind::DropItem(ItemTransfer {
                    slot: ItemSlot::new(12).unwrap(),
                    quantity: 2,
                    target: TransferTarget::Tile(CommandTilePosition { x: 30, y: 40 }),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 29,
            operation: CommandOperation::Submit {
                kind: CommandKind::DropGold(GoldTransfer {
                    amount: 500,
                    target: TransferTarget::Object(std::num::NonZeroU32::new(0x1122_3344).unwrap()),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 30,
            operation: CommandOperation::Submit {
                kind: CommandKind::PickupItem(CommandTilePosition { x: 30, y: 40 }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 31,
            operation: CommandOperation::Submit {
                kind: CommandKind::Unequip(EquipmentSlot::Armor),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 32,
            operation: CommandOperation::Submit {
                kind: CommandKind::Emote(12),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 33,
            operation: CommandOperation::Submit {
                kind: CommandKind::GiveItem(ItemTransfer {
                    slot: ItemSlot::new(12).unwrap(),
                    quantity: 2,
                    target: TransferTarget::Object(std::num::NonZeroU32::new(0x1122_3344).unwrap()),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 34,
            operation: CommandOperation::Submit {
                kind: CommandKind::GiveGold(GoldTransfer {
                    amount: 500,
                    target: TransferTarget::Object(std::num::NonZeroU32::new(0x1122_3344).unwrap()),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 35,
            operation: CommandOperation::Submit {
                kind: CommandKind::SwapSlots(SlotSwap::Inventory {
                    source: ItemSlot::new(1).unwrap(),
                    destination: ItemSlot::new(59).unwrap(),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 36,
            operation: CommandOperation::Submit {
                kind: CommandKind::Interact(std::num::NonZeroU32::new(0x1122_3344).unwrap()),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 37,
            operation: CommandOperation::Submit {
                kind: CommandKind::Dialog(DialogCommand {
                    revision: 7,
                    action: DialogAction::Select {
                        index: 0,
                        quantity: 1,
                    },
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 38,
            operation: CommandOperation::Submit {
                kind: CommandKind::Dialog(DialogCommand {
                    revision: 8,
                    action: DialogAction::Input(DialogText::new("ZiLo").unwrap()),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 39,
            operation: CommandOperation::Submit {
                kind: CommandKind::Group(GroupCommand::Toggle),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 40,
            operation: CommandOperation::Submit {
                kind: CommandKind::Group(GroupCommand::Invite(GroupText::new("ZiLo").unwrap())),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 41,
            operation: CommandOperation::Submit {
                kind: CommandKind::Group(GroupCommand::Respond {
                    invitation_id: 7,
                    action: GroupInvitationAction::Accept,
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 42,
            operation: CommandOperation::Submit {
                kind: CommandKind::Exchange(ExchangeCommand::AddItem {
                    slot: ItemSlot::new(3).unwrap(),
                    quantity: 5,
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 43,
            operation: CommandOperation::Submit {
                kind: CommandKind::Exchange(ExchangeCommand::SetGold(1_000)),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 44,
            operation: CommandOperation::Submit {
                kind: CommandKind::Exchange(ExchangeCommand::Accept),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 45,
            operation: CommandOperation::Submit {
                kind: CommandKind::Exchange(ExchangeCommand::Cancel),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 46,
            operation: CommandOperation::Submit {
                kind: CommandKind::Chant(ChantText::new("MiXeD, punctuation!  ").unwrap()),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 47,
            operation: CommandOperation::Submit {
                kind: CommandKind::Raw(
                    RawPacket::new(RawPacketDirection::Client, 0x7e, &[0x00, 0x03, 0x02]).unwrap(),
                ),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 48,
            operation: CommandOperation::Submit {
                kind: CommandKind::Raw(
                    RawPacket::new(RawPacketDirection::Server, 0x3a, &[]).unwrap(),
                ),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 49,
            operation: CommandOperation::Submit {
                kind: CommandKind::Assail,
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 50,
            operation: CommandOperation::Submit {
                kind: CommandKind::Resync,
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
        Message::CommandRequest(CommandRequest {
            request_id: 51,
            operation: CommandOperation::Submit {
                kind: CommandKind::Message(MessageCommand::Whisper {
                    recipient: MessageRecipient::new("Eidolon").unwrap(),
                    content: MessageContent::new("hello").unwrap(),
                }),
                timeout_ms: 1_000,
                wait_ms: 50,
            },
        }),
    ];

    for message in messages {
        let frame = Frame::new(7, 123, message);
        let bytes = encode_frame(&frame).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), frame);
    }
}
