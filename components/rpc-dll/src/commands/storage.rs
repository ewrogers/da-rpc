use super::*;

#[derive(Clone, Copy)]
// Route and exclusion tiles stay in fixed inline storage so command publishing
// never allocates or transfers ownership across threads.
#[allow(clippy::large_enum_variant)]
pub(super) enum StoredInput {
    Spell(SpellInput),
    Dialog(DialogText),
    Group(GroupText),
    Chant(ChantText),
    Raw(RawPacket),
    Message(StoredMessage),
    Tiles(StoredTiles),
}

impl StoredInput {
    pub(super) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Spell(value) => value.as_bytes(),
            Self::Dialog(value) => value.as_bytes(),
            Self::Group(value) => value.as_bytes(),
            Self::Chant(value) => value.as_bytes(),
            Self::Raw(value) => value.payload(),
            Self::Message(value) => value.as_bytes(),
            Self::Tiles(value) => value.as_bytes(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct StoredTiles {
    length: u16,
    bytes: [u8; MAX_COMMAND_TILE_BYTES],
}

impl StoredTiles {
    fn new(tiles: &[RouteTile]) -> Self {
        let mut bytes = [0; MAX_COMMAND_TILE_BYTES];
        for (index, tile) in tiles.iter().enumerate() {
            let offset = index * 4;
            bytes[offset..offset + 2].copy_from_slice(&tile.x.to_le_bytes());
            bytes[offset + 2..offset + 4].copy_from_slice(&tile.y.to_le_bytes());
        }
        Self {
            length: u16::try_from(tiles.len() * 4).expect("bounded route bytes fit u16"),
            bytes,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy)]
pub(super) struct StoredMessage {
    length: u8,
    bytes: [u8; MAX_MESSAGE_RECIPIENT_LEN + MAX_MESSAGE_CONTENT_LEN + 1],
}

impl StoredMessage {
    fn new(message: MessageCommand) -> Self {
        let mut bytes = [0; MAX_MESSAGE_RECIPIENT_LEN + MAX_MESSAGE_CONTENT_LEN + 1];
        let mut length = 0;
        if let Some(recipient) = message.recipient() {
            let recipient = recipient.as_bytes();
            bytes[0] = u8::try_from(recipient.len()).expect("message recipient length fits u8");
            bytes[1..1 + recipient.len()].copy_from_slice(recipient);
            length = recipient.len() + 1;
        }
        let content = message.content();
        let content = content.as_bytes();
        bytes[length..length + content.len()].copy_from_slice(content);
        length += content.len();
        Self {
            length: u8::try_from(length).expect("stored message input length fits u8"),
            bytes,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

pub(super) fn stored_kind(kind: CommandKind) -> (u8, u32, u32, u32, Option<StoredInput>) {
    match kind {
        CommandKind::Diagnostic => (0, 0, 0, 0, None),
        CommandKind::Turn(direction) => (1, direction.raw() as u32, 0, 0, None),
        CommandKind::Walk(WalkTarget::Direction(direction)) => {
            (2, direction.raw() as u32, 0, 0, None)
        }
        CommandKind::Walk(WalkTarget::Destination { x, y }) => (3, x as u32, y as u32, 0, None),
        CommandKind::Walk(WalkTarget::Route(route)) => (
            46,
            route.map_id(),
            u32::try_from(route.tiles().len()).expect("bounded route length fits u32"),
            0,
            Some(StoredInput::Tiles(StoredTiles::new(route.tiles()))),
        ),
        CommandKind::UseSkill(slot) => (4, slot.get() as u32, 0, 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::None,
        }) => (5, slot.get() as u32, 0, 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Target(SpellTarget::Object(id)),
        }) => (6, slot.get() as u32, id.get(), 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Target(SpellTarget::Tile { x, y }),
        }) => (7, slot.get() as u32, x as u32, y as u32, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Input(input),
        }) => (8, slot.get() as u32, 0, 0, Some(StoredInput::Spell(input))),
        CommandKind::UseItem(slot) => (9, slot.get() as u32, 0, 0, None),
        CommandKind::DropItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Tile(position),
        }) => (10, slot.get() as u32, quantity, pack_tile(position), None),
        CommandKind::DropItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Object(id),
        }) => (11, slot.get() as u32, quantity, id.get(), None),
        CommandKind::DropGold(GoldTransfer {
            amount,
            target: TransferTarget::Tile(position),
        }) => (12, amount, pack_tile(position), 0, None),
        CommandKind::DropGold(GoldTransfer {
            amount,
            target: TransferTarget::Object(id),
        }) => (13, amount, id.get(), 0, None),
        CommandKind::PickupItem(position) => (14, position.x as u32, position.y as u32, 0, None),
        CommandKind::Unequip(slot) => (15, slot.raw() as u32, 0, 0, None),
        CommandKind::Emote(code) => (16, code as u32, 0, 0, None),
        CommandKind::GiveItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Tile(position),
        }) => (17, slot.get() as u32, quantity, pack_tile(position), None),
        CommandKind::GiveItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Object(id),
        }) => (18, slot.get() as u32, quantity, id.get(), None),
        CommandKind::GiveGold(GoldTransfer {
            amount,
            target: TransferTarget::Tile(position),
        }) => (19, amount, pack_tile(position), 0, None),
        CommandKind::GiveGold(GoldTransfer {
            amount,
            target: TransferTarget::Object(id),
        }) => (20, amount, id.get(), 0, None),
        CommandKind::SwapSlots(SlotSwap::Inventory {
            source,
            destination,
        }) => (21, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::SwapSlots(SlotSwap::Spellbook {
            source,
            destination,
        }) => (22, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::SwapSlots(SlotSwap::Skillbook {
            source,
            destination,
        }) => (23, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::Interact(id) => (24, id.get(), 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Select { index, quantity },
        }) => (25, revision, u32::from(index), u32::from(quantity), None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Input(input),
        }) => (26, revision, 0, 0, Some(StoredInput::Dialog(input))),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Previous,
        }) => (27, revision, 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Next,
        }) => (28, revision, 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Close,
        }) => (29, revision, 0, 0, None),
        CommandKind::Group(GroupCommand::Toggle) => (33, 0, 0, 0, None),
        CommandKind::Group(GroupCommand::Invite(target)) => {
            (30, 0, 0, 0, Some(StoredInput::Group(target)))
        }
        CommandKind::Group(GroupCommand::Respond {
            invitation_id,
            action: GroupInvitationAction::Accept,
        }) => (31, invitation_id, 0, 0, None),
        CommandKind::Group(GroupCommand::Respond {
            invitation_id,
            action: GroupInvitationAction::Decline,
        }) => (32, invitation_id, 0, 0, None),
        CommandKind::Who => (34, 0, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::AddItem { slot, quantity }) => {
            (35, u32::from(slot.get()), u32::from(quantity), 0, None)
        }
        CommandKind::Exchange(ExchangeCommand::SetGold(amount)) => (36, amount, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::Accept) => (37, 0, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::Cancel) => (38, 0, 0, 0, None),
        CommandKind::Chant(text) => (39, 0, 0, 0, Some(StoredInput::Chant(text))),
        CommandKind::Legend => (40, 0, 0, 0, None),
        CommandKind::Raw(packet) => (
            41,
            match packet.direction() {
                RawPacketDirection::Client => 0,
                RawPacketDirection::Server => 1,
            },
            u32::from(packet.command()),
            0,
            Some(StoredInput::Raw(packet)),
        ),
        CommandKind::Assail => (42, 0, 0, 0, None),
        CommandKind::InspectPlayer(id) => (43, id.get(), 0, 0, None),
        CommandKind::Resync => (44, 0, 0, 0, None),
        CommandKind::Message(message) => (
            45,
            match message {
                MessageCommand::Say(_) => 0,
                MessageCommand::Shout(_) => 1,
                MessageCommand::Whisper { .. } => 2,
                MessageCommand::Guild(_) => 3,
                MessageCommand::Group(_) => 4,
            },
            0,
            0,
            Some(StoredInput::Message(StoredMessage::new(message))),
        ),
        CommandKind::SetPathExclusions(exclusions) => (
            47,
            exclusions.map_id(),
            u32::try_from(exclusions.tiles().len()).expect("bounded exclusion length fits u32"),
            0,
            Some(StoredInput::Tiles(StoredTiles::new(exclusions.tiles()))),
        ),
        CommandKind::RemovePathExclusions { map_id } => (48, map_id, 0, 0, None),
        CommandKind::ClearPathExclusions => (49, 0, 0, 0, None),
        CommandKind::AddStat(stat) => (50, u32::from(stat.flag()), 0, 0, None),
        CommandKind::SelectFieldMapDestination(command) => (
            51,
            command.revision,
            u32::from(command.destination_index),
            0,
            None,
        ),
    }
}

pub(super) fn kind_from_value(
    value: u8,
    argument_x: u32,
    argument_y: u32,
    argument_z: u32,
    input: &[u8],
) -> CommandKind {
    match value {
        1 => CommandKind::Turn(stored_direction(argument_x)),
        2 => CommandKind::Walk(WalkTarget::Direction(stored_direction(argument_x))),
        3 => CommandKind::Walk(WalkTarget::Destination {
            x: argument_x as i32,
            y: argument_y as i32,
        }),
        4 => match SkillSlot::new(argument_x as u8) {
            Some(slot) => CommandKind::UseSkill(slot),
            None => CommandKind::Diagnostic,
        },
        5..=8 => {
            let Some(slot) = SpellSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let arguments = match value {
                5 => SpellArguments::None,
                6 => match std::num::NonZeroU32::new(argument_y) {
                    Some(id) => SpellArguments::Target(SpellTarget::Object(id)),
                    None => return CommandKind::Diagnostic,
                },
                7 => SpellArguments::Target(SpellTarget::Tile {
                    x: argument_y as i32,
                    y: argument_z as i32,
                }),
                8 => {
                    let Ok(input) = std::str::from_utf8(input) else {
                        return CommandKind::Diagnostic;
                    };
                    let Some(input) = SpellInput::new(input) else {
                        return CommandKind::Diagnostic;
                    };
                    SpellArguments::Input(input)
                }
                _ => unreachable!(),
            };
            CommandKind::CastSpell(SpellCast { slot, arguments })
        }
        9 => ItemSlot::new(argument_x as u8)
            .map(CommandKind::UseItem)
            .unwrap_or(CommandKind::Diagnostic),
        10 | 11 => {
            let Some(slot) = ItemSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let target = if value == 10 {
                TransferTarget::Tile(unpack_tile(argument_z))
            } else {
                let Some(id) = std::num::NonZeroU32::new(argument_z) else {
                    return CommandKind::Diagnostic;
                };
                TransferTarget::Object(id)
            };
            CommandKind::DropItem(ItemTransfer {
                slot,
                quantity: argument_y,
                target,
            })
        }
        12 => CommandKind::DropGold(GoldTransfer {
            amount: argument_x,
            target: TransferTarget::Tile(unpack_tile(argument_y)),
        }),
        13 => match std::num::NonZeroU32::new(argument_y) {
            Some(id) => CommandKind::DropGold(GoldTransfer {
                amount: argument_x,
                target: TransferTarget::Object(id),
            }),
            None => CommandKind::Diagnostic,
        },
        14 => CommandKind::PickupItem(TilePosition {
            x: argument_x as i32,
            y: argument_y as i32,
        }),
        15 => EquipmentSlot::from_raw(argument_x as u8)
            .map(CommandKind::Unequip)
            .unwrap_or(CommandKind::Diagnostic),
        16 => CommandKind::Emote(argument_x as u8),
        17 | 18 => {
            let Some(slot) = ItemSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let target = if value == 17 {
                TransferTarget::Tile(unpack_tile(argument_z))
            } else {
                let Some(id) = std::num::NonZeroU32::new(argument_z) else {
                    return CommandKind::Diagnostic;
                };
                TransferTarget::Object(id)
            };
            CommandKind::GiveItem(ItemTransfer {
                slot,
                quantity: argument_y,
                target,
            })
        }
        19 => CommandKind::GiveGold(GoldTransfer {
            amount: argument_x,
            target: TransferTarget::Tile(unpack_tile(argument_y)),
        }),
        20 => match std::num::NonZeroU32::new(argument_y) {
            Some(id) => CommandKind::GiveGold(GoldTransfer {
                amount: argument_x,
                target: TransferTarget::Object(id),
            }),
            None => CommandKind::Diagnostic,
        },
        21..=23 => {
            let swap = match value {
                21 => ItemSlot::new(argument_x as u8)
                    .zip(ItemSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Inventory {
                        source,
                        destination,
                    }),
                22 => SpellSlot::new(argument_x as u8)
                    .zip(SpellSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Spellbook {
                        source,
                        destination,
                    }),
                23 => SkillSlot::new(argument_x as u8)
                    .zip(SkillSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Skillbook {
                        source,
                        destination,
                    }),
                _ => None,
            };
            swap.map(CommandKind::SwapSlots)
                .unwrap_or(CommandKind::Diagnostic)
        }
        24 => std::num::NonZeroU32::new(argument_x)
            .map(CommandKind::Interact)
            .unwrap_or(CommandKind::Diagnostic),
        25 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Select {
                index: argument_y as u16,
                quantity: argument_z as u8,
            },
        }),
        26 => {
            let Ok(text) = std::str::from_utf8(input) else {
                return CommandKind::Diagnostic;
            };
            let Some(text) = DialogText::new(text) else {
                return CommandKind::Diagnostic;
            };
            CommandKind::Dialog(DialogCommand {
                revision: argument_x,
                action: DialogAction::Input(text),
            })
        }
        27 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Previous,
        }),
        28 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Next,
        }),
        29 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Close,
        }),
        30 => {
            let Ok(target) = std::str::from_utf8(input) else {
                return CommandKind::Diagnostic;
            };
            GroupText::new(target)
                .map(|target| CommandKind::Group(GroupCommand::Invite(target)))
                .unwrap_or(CommandKind::Diagnostic)
        }
        31 | 32 => CommandKind::Group(GroupCommand::Respond {
            invitation_id: argument_x,
            action: if value == 31 {
                GroupInvitationAction::Accept
            } else {
                GroupInvitationAction::Decline
            },
        }),
        33 => CommandKind::Group(GroupCommand::Toggle),
        34 => CommandKind::Who,
        35 => ItemSlot::new(argument_x as u8)
            .map(|slot| {
                CommandKind::Exchange(ExchangeCommand::AddItem {
                    slot,
                    quantity: argument_y as u8,
                })
            })
            .unwrap_or(CommandKind::Diagnostic),
        36 => CommandKind::Exchange(ExchangeCommand::SetGold(argument_x)),
        37 => CommandKind::Exchange(ExchangeCommand::Accept),
        38 => CommandKind::Exchange(ExchangeCommand::Cancel),
        39 => std::str::from_utf8(input)
            .ok()
            .and_then(ChantText::new)
            .map(CommandKind::Chant)
            .unwrap_or(CommandKind::Diagnostic),
        40 => CommandKind::Legend,
        41 => {
            let direction = match argument_x {
                0 => RawPacketDirection::Client,
                1 => RawPacketDirection::Server,
                _ => return CommandKind::Diagnostic,
            };
            RawPacket::new(direction, argument_y as u8, input)
                .map(CommandKind::Raw)
                .unwrap_or(CommandKind::Diagnostic)
        }
        42 => CommandKind::Assail,
        43 => NonZeroU32::new(argument_x)
            .map(CommandKind::InspectPlayer)
            .unwrap_or(CommandKind::Diagnostic),
        44 => CommandKind::Resync,
        45 => stored_message(argument_x, input).unwrap_or(CommandKind::Diagnostic),
        46 => stored_tiles(argument_y, input)
            .and_then(|(tiles, length)| WalkRoute::new(argument_x, &tiles[..length]))
            .map(|route| CommandKind::Walk(WalkTarget::Route(route)))
            .unwrap_or(CommandKind::Diagnostic),
        47 => stored_tiles(argument_y, input)
            .and_then(|(tiles, length)| PathExclusions::new(argument_x, &tiles[..length]))
            .map(CommandKind::SetPathExclusions)
            .unwrap_or(CommandKind::Diagnostic),
        48 => CommandKind::RemovePathExclusions { map_id: argument_x },
        49 => CommandKind::ClearPathExclusions,
        50 => CharacterStat::from_flag(argument_x as u8)
            .map(CommandKind::AddStat)
            .unwrap_or(CommandKind::Diagnostic),
        51 => CommandKind::SelectFieldMapDestination(FieldMapSelectionCommand {
            revision: argument_x,
            destination_index: argument_y as u8,
        }),
        _ => CommandKind::Diagnostic,
    }
}

fn stored_tiles(count: u32, input: &[u8]) -> Option<([RouteTile; MAX_WALK_ROUTE_TILES], usize)> {
    let count = usize::try_from(count).ok()?;
    if count > MAX_WALK_ROUTE_TILES || input.len() != count.checked_mul(4)? {
        return None;
    }
    let mut tiles = [RouteTile { x: 0, y: 0 }; MAX_WALK_ROUTE_TILES];
    for (tile, bytes) in tiles.iter_mut().zip(input.chunks_exact(4)) {
        *tile = RouteTile {
            x: u16::from_le_bytes(bytes[..2].try_into().ok()?),
            y: u16::from_le_bytes(bytes[2..].try_into().ok()?),
        };
    }
    Some((tiles, count))
}

fn stored_message(channel: u32, input: &[u8]) -> Option<CommandKind> {
    let (recipient, content) = if channel == 2 {
        let recipient_length = usize::from(*input.first()?);
        let recipient_end = recipient_length.checked_add(1)?;
        let recipient = std::str::from_utf8(input.get(1..recipient_end)?).ok()?;
        (
            Some(MessageRecipient::new(recipient)?),
            input.get(recipient_end..)?,
        )
    } else {
        (None, input)
    };
    let content = MessageContent::new(std::str::from_utf8(content).ok()?)?;
    let message = match channel {
        0 => MessageCommand::Say(content),
        1 => MessageCommand::Shout(content),
        2 => MessageCommand::Whisper {
            recipient: recipient?,
            content,
        },
        3 => MessageCommand::Guild(content),
        4 => MessageCommand::Group(content),
        _ => return None,
    };
    Some(CommandKind::Message(message))
}

fn pack_tile(position: TilePosition) -> u32 {
    let Ok(x) = u16::try_from(position.x) else {
        return u32::MAX;
    };
    let Ok(y) = u16::try_from(position.y) else {
        return u32::MAX;
    };
    u32::from(x) | (u32::from(y) << 16)
}

fn unpack_tile(value: u32) -> TilePosition {
    TilePosition {
        x: i32::from(value as u16),
        y: i32::from((value >> 16) as u16),
    }
}

const fn stored_direction(value: u32) -> Direction {
    match Direction::from_raw(value as u8) {
        Some(direction) => direction,
        None => Direction::North,
    }
}
