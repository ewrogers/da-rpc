use crate::{
    DecodeError,
    message::{PayloadReader, push_i32, push_u16, push_u32},
};
use darpc_model::{Direction, EquipmentSlot};
use std::num::NonZeroU32;

pub const DEFAULT_COMMAND_TIMEOUT_MS: u16 = 1_000;
pub const MAX_COMMAND_TIMEOUT_MS: u16 = 5_000;
pub const MAX_COMMAND_WAIT_MS: u16 = 1_000;
pub const MAX_SKILL_SLOT: u8 = 90;
pub const MAX_SPELL_SLOT: u8 = 90;
pub const MAX_ITEM_SLOT: u8 = 59;
pub const MAX_SPELL_INPUT_LEN: usize = 100;
pub const MAX_DIALOG_INPUT_LEN: usize = u8::MAX as usize;
pub const MAX_CHANT_TEXT_LEN: usize = u8::MAX as usize;
pub const MAX_MESSAGE_CONTENT_LEN: usize = 100;
pub const MAX_MESSAGE_RECIPIENT_LEN: usize = 15;
pub const MAX_GROUP_NAME_LEN: usize = 28;
pub const MAX_WHO_PLAYERS: usize = 768;
pub const MAX_WHO_NAME_LEN: usize = 24;
pub const MAX_WHO_TITLE_LEN: usize = 48;
pub const MAX_RAW_PACKET_PAYLOAD_LEN: usize = u8::MAX as usize;
pub const MAX_WALK_ROUTE_TILES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawPacketDirection {
    Client,
    Server,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterStat {
    Strength,
    Dexterity,
    Intelligence,
    Wisdom,
    Constitution,
}

impl CharacterStat {
    #[must_use]
    pub const fn flag(self) -> u8 {
        match self {
            Self::Strength => 0x01,
            Self::Dexterity => 0x02,
            Self::Intelligence => 0x04,
            Self::Wisdom => 0x08,
            Self::Constitution => 0x10,
        }
    }

    pub const fn from_flag(flag: u8) -> Option<Self> {
        match flag {
            0x01 => Some(Self::Strength),
            0x02 => Some(Self::Dexterity),
            0x04 => Some(Self::Intelligence),
            0x08 => Some(Self::Wisdom),
            0x10 => Some(Self::Constitution),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawPacket {
    direction: RawPacketDirection,
    command: u8,
    payload_length: u8,
    payload: [u8; MAX_RAW_PACKET_PAYLOAD_LEN],
}

impl RawPacket {
    #[must_use]
    pub fn new(direction: RawPacketDirection, command: u8, payload: &[u8]) -> Option<Self> {
        let payload_length = u8::try_from(payload.len()).ok()?;
        let mut stored = [0; MAX_RAW_PACKET_PAYLOAD_LEN];
        stored[..payload.len()].copy_from_slice(payload);
        Some(Self {
            direction,
            command,
            payload_length,
            payload: stored,
        })
    }

    #[must_use]
    pub const fn direction(self) -> RawPacketDirection {
        self.direction
    }

    #[must_use]
    pub const fn command(self) -> u8 {
        self.command
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload[..usize::from(self.payload_length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Walk routes stay inline so command transfer remains bounded, allocation-free,
// and Copy inside the injected DLL's command queue.
#[allow(clippy::large_enum_variant)]
pub enum CommandKind {
    Diagnostic,
    Turn(Direction),
    Walk(WalkTarget),
    UseSkill(SkillSlot),
    CastSpell(SpellCast),
    UseItem(ItemSlot),
    DropItem(ItemTransfer),
    DropGold(GoldTransfer),
    PickupItem(TilePosition),
    Unequip(EquipmentSlot),
    Emote(u8),
    GiveItem(ItemTransfer),
    GiveGold(GoldTransfer),
    SwapSlots(SlotSwap),
    Interact(NonZeroU32),
    Dialog(DialogCommand),
    Group(GroupCommand),
    Who,
    Exchange(ExchangeCommand),
    Chant(ChantText),
    Legend,
    Raw(RawPacket),
    Assail,
    InspectPlayer(NonZeroU32),
    Resync,
    Message(MessageCommand),
    AddStat(CharacterStat),
    SelectFieldMapDestination(FieldMapSelectionCommand),
    DismissMessageDialog(MessageDialogCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageCommand {
    Say(MessageContent),
    Shout(MessageContent),
    Whisper {
        recipient: MessageRecipient,
        content: MessageContent,
    },
    Guild(MessageContent),
    Group(MessageContent),
}

impl MessageCommand {
    #[must_use]
    pub const fn content(self) -> MessageContent {
        match self {
            Self::Say(content)
            | Self::Shout(content)
            | Self::Whisper { content, .. }
            | Self::Guild(content)
            | Self::Group(content) => content,
        }
    }

    #[must_use]
    pub const fn recipient(self) -> Option<MessageRecipient> {
        match self {
            Self::Whisper { recipient, .. } => Some(recipient),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageContent {
    length: u8,
    bytes: [u8; MAX_MESSAGE_CONTENT_LEN],
}

impl MessageContent {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        let value = value.as_bytes();
        if value.is_empty() || value.len() > MAX_MESSAGE_CONTENT_LEN || !value.is_ascii() {
            return None;
        }
        let mut bytes = [0; MAX_MESSAGE_CONTENT_LEN];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: u8::try_from(value.len()).expect("message content limit fits u8"),
            bytes,
        })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageRecipient {
    length: u8,
    bytes: [u8; MAX_MESSAGE_RECIPIENT_LEN],
}

impl MessageRecipient {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        Self::from_value(value, false)
    }

    #[must_use]
    pub fn channel(value: &str) -> Option<Self> {
        Self::from_value(value, true)
    }

    fn from_value(value: &str, channel: bool) -> Option<Self> {
        let value = value.as_bytes();
        if value.is_empty()
            || value.len() > MAX_MESSAGE_RECIPIENT_LEN
            || !value.is_ascii()
            || value.iter().any(u8::is_ascii_whitespace)
            || (!channel && matches!(value, b"!" | b"!!" | b"#" | b"@" | b"$"))
            || (channel && !matches!(value, b"!" | b"!!"))
        {
            return None;
        }
        let mut bytes = [0; MAX_MESSAGE_RECIPIENT_LEN];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: u8::try_from(value.len()).expect("message recipient limit fits u8"),
            bytes,
        })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChantText {
    length: u8,
    bytes: [u8; MAX_CHANT_TEXT_LEN],
}

impl ChantText {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        Self::from_parts(b"", value)
    }

    #[must_use]
    pub fn sell(item_name: &str) -> Option<Self> {
        Self::from_parts(b"buy my ", item_name)
    }

    #[must_use]
    pub fn sell_all(item_name: &str) -> Option<Self> {
        Self::from_parts(b"buy my all ", item_name)
    }

    #[must_use]
    pub fn deposit(item_name: &str) -> Option<Self> {
        Self::from_parts(b"i will deposit ", item_name)
    }

    #[must_use]
    pub fn withdraw(item_name: &str) -> Option<Self> {
        Self::from_parts(b"give my ", item_name).and_then(|text| text.with_suffix(b" back"))
    }

    #[must_use]
    pub fn repair(item_name: &str) -> Option<Self> {
        Self::from_parts(b"repair my ", item_name)
    }

    #[must_use]
    pub fn repair_all() -> Self {
        Self::new("repair all").expect("the fixed repair-all chant is valid")
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }

    fn from_parts(prefix: &[u8], value: &str) -> Option<Self> {
        let value = value.as_bytes();
        let length = prefix.len().checked_add(value.len())?;
        if value.is_empty() || length > MAX_CHANT_TEXT_LEN || !value.is_ascii() {
            return None;
        }
        let mut bytes = [0; MAX_CHANT_TEXT_LEN];
        bytes[..prefix.len()].copy_from_slice(prefix);
        bytes[prefix.len()..length].copy_from_slice(value);
        Some(Self {
            length: length as u8,
            bytes,
        })
    }

    fn with_suffix(mut self, suffix: &[u8]) -> Option<Self> {
        let start = usize::from(self.length);
        let length = start.checked_add(suffix.len())?;
        if length > MAX_CHANT_TEXT_LEN {
            return None;
        }
        self.bytes[start..length].copy_from_slice(suffix);
        self.length = length as u8;
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExchangeCommand {
    AddItem { slot: ItemSlot, quantity: u8 },
    SetGold(u32),
    Accept,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupCommand {
    Toggle,
    Invite(GroupText),
    Respond {
        invitation_id: u32,
        action: GroupInvitationAction,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupInvitationAction {
    Accept,
    Decline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupText {
    length: u8,
    bytes: [u8; MAX_GROUP_NAME_LEN],
}

impl GroupText {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        let value = value.as_bytes();
        if value.is_empty() || value.len() > MAX_GROUP_NAME_LEN || !value.is_ascii() {
            return None;
        }
        let mut bytes = [0; MAX_GROUP_NAME_LEN];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: value.len() as u8,
            bytes,
        })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogCommand {
    pub revision: u32,
    pub action: DialogAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldMapSelectionCommand {
    pub revision: u32,
    pub destination_index: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageDialogCommand {
    pub revision: u32,
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Dialog text remains inline so decoded commands are bounded and pointer-free.
#[allow(clippy::large_enum_variant)]
pub enum DialogAction {
    Select { index: u16, quantity: u8 },
    Input(DialogText),
    Previous,
    Next,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogText {
    length: u8,
    bytes: [u8; MAX_DIALOG_INPUT_LEN],
}

impl DialogText {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        let value = value.as_bytes();
        if value.is_empty() || value.len() > MAX_DIALOG_INPUT_LEN || !value.is_ascii() {
            return None;
        }
        let mut bytes = [0; MAX_DIALOG_INPUT_LEN];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: value.len() as u8,
            bytes,
        })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotSwap {
    Inventory {
        source: ItemSlot,
        destination: ItemSlot,
    },
    Spellbook {
        source: SpellSlot,
        destination: SpellSlot,
    },
    Skillbook {
        source: SkillSlot,
        destination: SkillSlot,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemTransfer {
    pub slot: ItemSlot,
    pub quantity: u32,
    pub target: TransferTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoldTransfer {
    pub amount: u32,
    pub target: TransferTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferTarget {
    Tile(TilePosition),
    Object(NonZeroU32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpellCast {
    pub slot: SpellSlot,
    pub arguments: SpellArguments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpellArguments {
    None,
    Target(SpellTarget),
    Input(SpellInput),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpellTarget {
    Object(NonZeroU32),
    Tile { x: i32, y: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpellInput {
    length: u8,
    bytes: [u8; MAX_SPELL_INPUT_LEN],
}

impl SpellInput {
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        Self::from_bytes(value.as_bytes())
    }

    fn from_bytes(value: &[u8]) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_SPELL_INPUT_LEN || !value.is_ascii() {
            return None;
        }
        let mut bytes = [0; MAX_SPELL_INPUT_LEN];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: u8::try_from(value.len()).expect("spell input limit fits u8"),
            bytes,
        })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillSlot(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemSlot(u8);

impl ItemSlot {
    #[must_use]
    pub const fn new(slot: u8) -> Option<Self> {
        if slot > 0 && slot <= MAX_ITEM_SLOT {
            Some(Self(slot))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl SkillSlot {
    #[must_use]
    pub const fn new(slot: u8) -> Option<Self> {
        if slot > 0 && slot <= MAX_SKILL_SLOT {
            Some(Self(slot))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpellSlot(u8);

impl SpellSlot {
    #[must_use]
    pub const fn new(slot: u8) -> Option<Self> {
        if slot > 0 && slot <= MAX_SPELL_SLOT {
            Some(Self(slot))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Exact routes remain inline so decoded and queued commands are bounded and
// pointer-free across the IPC-worker to main-thread handoff.
#[allow(clippy::large_enum_variant)]
pub enum WalkTarget {
    Direction(Direction),
    Destination { x: i32, y: i32 },
    Route(WalkRoute),
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteTile {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalkRoute {
    map_id: u32,
    length: u16,
    tiles: [RouteTile; MAX_WALK_ROUTE_TILES],
}

impl WalkRoute {
    #[must_use]
    pub fn new(map_id: u32, tiles: &[RouteTile]) -> Option<Self> {
        let length = u16::try_from(tiles.len()).ok()?;
        if tiles.is_empty() || tiles.len() > MAX_WALK_ROUTE_TILES {
            return None;
        }
        let mut stored = [RouteTile { x: 0, y: 0 }; MAX_WALK_ROUTE_TILES];
        stored[..tiles.len()].copy_from_slice(tiles);
        Some(Self {
            map_id,
            length,
            tiles: stored,
        })
    }

    #[must_use]
    pub const fn map_id(self) -> u32 {
        self.map_id
    }

    #[must_use]
    pub fn tiles(&self) -> &[RouteTile] {
        &self.tiles[..usize::from(self.length)]
    }
}

pub(super) fn encode_kind(output: &mut Vec<u8>, kind: CommandKind) {
    match kind {
        CommandKind::Diagnostic => output.push(0),
        CommandKind::Turn(direction) => {
            output.push(1);
            output.push(direction.raw());
        }
        CommandKind::Walk(WalkTarget::Direction(direction)) => {
            output.push(2);
            output.push(0);
            output.push(direction.raw());
        }
        CommandKind::Walk(WalkTarget::Destination { x, y }) => {
            output.push(2);
            output.push(1);
            push_i32(output, x);
            push_i32(output, y);
        }
        CommandKind::Walk(WalkTarget::Route(route)) => {
            output.push(2);
            output.push(2);
            encode_route_tiles(output, route.map_id(), route.tiles());
        }
        CommandKind::Walk(WalkTarget::Cancel) => {
            output.push(2);
            output.push(3);
        }
        CommandKind::UseSkill(slot) => {
            output.push(3);
            output.push(slot.get());
        }
        CommandKind::CastSpell(cast) => {
            output.push(4);
            output.push(cast.slot.get());
            match cast.arguments {
                SpellArguments::None => output.push(0),
                SpellArguments::Target(SpellTarget::Object(id)) => {
                    output.push(1);
                    push_u32(output, id.get());
                }
                SpellArguments::Target(SpellTarget::Tile { x, y }) => {
                    output.push(2);
                    push_i32(output, x);
                    push_i32(output, y);
                }
                SpellArguments::Input(input) => {
                    output.push(3);
                    output.push(input.length);
                    output.extend_from_slice(input.as_bytes());
                }
            }
        }
        CommandKind::UseItem(slot) => {
            output.push(5);
            output.push(slot.get());
        }
        CommandKind::DropItem(transfer) => {
            output.push(6);
            output.push(transfer.slot.get());
            push_u32(output, transfer.quantity);
            encode_transfer_target(output, transfer.target);
        }
        CommandKind::DropGold(transfer) => {
            output.push(7);
            push_u32(output, transfer.amount);
            encode_transfer_target(output, transfer.target);
        }
        CommandKind::PickupItem(position) => {
            output.push(8);
            push_i32(output, position.x);
            push_i32(output, position.y);
        }
        CommandKind::Unequip(slot) => {
            output.push(9);
            output.push(slot.raw());
        }
        CommandKind::Emote(code) => {
            output.push(10);
            output.push(code);
        }
        CommandKind::GiveItem(transfer) => {
            output.push(11);
            output.push(transfer.slot.get());
            push_u32(output, transfer.quantity);
            encode_transfer_target(output, transfer.target);
        }
        CommandKind::GiveGold(transfer) => {
            output.push(12);
            push_u32(output, transfer.amount);
            encode_transfer_target(output, transfer.target);
        }
        CommandKind::SwapSlots(swap) => {
            output.push(13);
            match swap {
                SlotSwap::Inventory {
                    source,
                    destination,
                } => {
                    output.push(0);
                    output.push(source.get());
                    output.push(destination.get());
                }
                SlotSwap::Spellbook {
                    source,
                    destination,
                } => {
                    output.push(1);
                    output.push(source.get());
                    output.push(destination.get());
                }
                SlotSwap::Skillbook {
                    source,
                    destination,
                } => {
                    output.push(2);
                    output.push(source.get());
                    output.push(destination.get());
                }
            }
        }
        CommandKind::Interact(id) => {
            output.push(14);
            push_u32(output, id.get());
        }
        CommandKind::Dialog(command) => {
            output.push(15);
            push_u32(output, command.revision);
            match command.action {
                DialogAction::Select { index, quantity } => {
                    output.push(1);
                    push_u16(output, index);
                    output.push(quantity);
                }
                DialogAction::Input(input) => {
                    output.push(2);
                    output.push(input.length);
                    output.extend_from_slice(input.as_bytes());
                }
                DialogAction::Previous => output.push(3),
                DialogAction::Next => output.push(4),
                DialogAction::Close => output.push(5),
            }
        }
        CommandKind::Group(command) => {
            output.push(16);
            match command {
                GroupCommand::Toggle => output.push(4),
                GroupCommand::Invite(target) => {
                    output.push(1);
                    output.push(target.length);
                    output.extend_from_slice(target.as_bytes());
                }
                GroupCommand::Respond {
                    invitation_id,
                    action,
                } => {
                    output.push(match action {
                        GroupInvitationAction::Accept => 2,
                        GroupInvitationAction::Decline => 3,
                    });
                    push_u32(output, invitation_id);
                }
            }
        }
        CommandKind::Who => output.push(17),
        CommandKind::Exchange(command) => {
            output.push(18);
            match command {
                ExchangeCommand::AddItem { slot, quantity } => {
                    output.extend_from_slice(&[1, slot.get(), quantity]);
                }
                ExchangeCommand::SetGold(amount) => {
                    output.push(2);
                    push_u32(output, amount);
                }
                ExchangeCommand::Accept => output.push(3),
                ExchangeCommand::Cancel => output.push(4),
            }
        }
        CommandKind::Chant(text) => {
            output.push(19);
            output.push(text.length);
            output.extend_from_slice(text.as_bytes());
        }
        CommandKind::Legend => output.push(20),
        CommandKind::Raw(packet) => {
            output.push(21);
            output.push(match packet.direction {
                RawPacketDirection::Client => 0,
                RawPacketDirection::Server => 1,
            });
            output.push(packet.command);
            output.push(packet.payload_length);
            output.extend_from_slice(packet.payload());
        }
        CommandKind::Assail => output.push(22),
        CommandKind::InspectPlayer(id) => {
            output.push(23);
            push_u32(output, id.get());
        }
        CommandKind::Resync => output.push(24),
        CommandKind::Message(message) => {
            output.push(25);
            match message {
                MessageCommand::Say(content) => encode_message(output, 0, None, content),
                MessageCommand::Shout(content) => encode_message(output, 1, None, content),
                MessageCommand::Whisper { recipient, content } => {
                    encode_message(output, 2, Some(recipient), content);
                }
                MessageCommand::Guild(content) => encode_message(output, 3, None, content),
                MessageCommand::Group(content) => encode_message(output, 4, None, content),
            }
        }
        CommandKind::AddStat(stat) => {
            output.push(29);
            output.push(stat.flag());
        }
        CommandKind::SelectFieldMapDestination(command) => {
            output.push(30);
            push_u32(output, command.revision);
            output.push(command.destination_index);
        }
        CommandKind::DismissMessageDialog(command) => {
            output.push(31);
            push_u32(output, command.revision);
            push_u32(output, command.id);
        }
    }
}

fn encode_message(
    output: &mut Vec<u8>,
    channel: u8,
    recipient: Option<MessageRecipient>,
    content: MessageContent,
) {
    output.push(channel);
    if let Some(recipient) = recipient {
        output.push(recipient.length);
        output.extend_from_slice(recipient.as_bytes());
    }
    output.push(content.length);
    output.extend_from_slice(content.as_bytes());
}

fn encode_transfer_target(output: &mut Vec<u8>, target: TransferTarget) {
    match target {
        TransferTarget::Tile(position) => {
            output.push(0);
            push_i32(output, position.x);
            push_i32(output, position.y);
        }
        TransferTarget::Object(id) => {
            output.push(1);
            push_u32(output, id.get());
        }
    }
}

pub(super) fn decode_kind(reader: &mut PayloadReader<'_>) -> Result<CommandKind, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(CommandKind::Diagnostic),
        1 => Ok(CommandKind::Turn(decode_direction(reader)?)),
        2 => Ok(CommandKind::Walk(match reader.read_u8()? {
            0 => WalkTarget::Direction(decode_direction(reader)?),
            1 => WalkTarget::Destination {
                x: reader.read_i32()?,
                y: reader.read_i32()?,
            },
            2 => {
                let map_id = reader.read_u32()?;
                let tiles = decode_route_tiles(reader, MAX_WALK_ROUTE_TILES)?;
                WalkTarget::Route(WalkRoute::new(map_id, &tiles).ok_or(
                    DecodeError::InvalidRouteTileCount {
                        actual: tiles.len(),
                        max: MAX_WALK_ROUTE_TILES,
                    },
                )?)
            }
            3 => WalkTarget::Cancel,
            actual => return Err(DecodeError::InvalidWalkTarget { actual }),
        })),
        3 => {
            let actual = reader.read_u8()?;
            SkillSlot::new(actual)
                .map(CommandKind::UseSkill)
                .ok_or(DecodeError::InvalidSkillSlot {
                    actual,
                    max: MAX_SKILL_SLOT,
                })
        }
        4 => {
            let actual = reader.read_u8()?;
            let slot = SpellSlot::new(actual).ok_or(DecodeError::InvalidSpellSlot {
                actual,
                max: MAX_SPELL_SLOT,
            })?;
            let arguments = match reader.read_u8()? {
                0 => SpellArguments::None,
                1 => SpellArguments::Target(SpellTarget::Object(
                    NonZeroU32::new(reader.read_u32()?).ok_or(DecodeError::InvalidSpellTarget)?,
                )),
                2 => SpellArguments::Target(SpellTarget::Tile {
                    x: reader.read_i32()?,
                    y: reader.read_i32()?,
                }),
                3 => {
                    let length = usize::from(reader.read_u8()?);
                    let bytes = reader.take(length)?;
                    SpellArguments::Input(
                        SpellInput::from_bytes(bytes).ok_or(DecodeError::InvalidSpellInput)?,
                    )
                }
                actual => return Err(DecodeError::InvalidSpellArguments { actual }),
            };
            Ok(CommandKind::CastSpell(SpellCast { slot, arguments }))
        }
        5 => decode_item_slot(reader).map(CommandKind::UseItem),
        6 => Ok(CommandKind::DropItem(ItemTransfer {
            slot: decode_item_slot(reader)?,
            quantity: reader.read_u32()?,
            target: decode_transfer_target(reader)?,
        })),
        7 => Ok(CommandKind::DropGold(GoldTransfer {
            amount: reader.read_u32()?,
            target: decode_transfer_target(reader)?,
        })),
        8 => Ok(CommandKind::PickupItem(TilePosition {
            x: reader.read_i32()?,
            y: reader.read_i32()?,
        })),
        9 => {
            let actual = reader.read_u8()?;
            EquipmentSlot::from_raw(actual)
                .map(CommandKind::Unequip)
                .ok_or(DecodeError::InvalidEquipmentSlot { actual })
        }
        10 => Ok(CommandKind::Emote(reader.read_u8()?)),
        11 => Ok(CommandKind::GiveItem(ItemTransfer {
            slot: decode_item_slot(reader)?,
            quantity: reader.read_u32()?,
            target: decode_transfer_target(reader)?,
        })),
        12 => Ok(CommandKind::GiveGold(GoldTransfer {
            amount: reader.read_u32()?,
            target: decode_transfer_target(reader)?,
        })),
        13 => decode_slot_swap(reader).map(CommandKind::SwapSlots),
        14 => NonZeroU32::new(reader.read_u32()?)
            .map(CommandKind::Interact)
            .ok_or(DecodeError::InvalidTransferTarget { actual: 1 }),
        15 => {
            let revision = reader.read_u32()?;
            let action = match reader.read_u8()? {
                1 => DialogAction::Select {
                    index: reader.read_u16()?,
                    quantity: reader.read_u8()?,
                },
                2 => {
                    let length = usize::from(reader.read_u8()?);
                    let text = std::str::from_utf8(reader.take(length)?)
                        .map_err(|_| DecodeError::InvalidDialogText)?;
                    DialogAction::Input(
                        DialogText::new(text).ok_or(DecodeError::InvalidDialogText)?,
                    )
                }
                3 => DialogAction::Previous,
                4 => DialogAction::Next,
                5 => DialogAction::Close,
                actual => return Err(DecodeError::InvalidDialogField { actual }),
            };
            Ok(CommandKind::Dialog(DialogCommand { revision, action }))
        }
        16 => match reader.read_u8()? {
            1 => {
                let length = usize::from(reader.read_u8()?);
                let target = std::str::from_utf8(reader.take(length)?)
                    .ok()
                    .and_then(GroupText::new)
                    .ok_or(DecodeError::InvalidGroupField { actual: 1 })?;
                Ok(CommandKind::Group(GroupCommand::Invite(target)))
            }
            action @ (2 | 3) => Ok(CommandKind::Group(GroupCommand::Respond {
                invitation_id: reader.read_u32()?,
                action: if action == 2 {
                    GroupInvitationAction::Accept
                } else {
                    GroupInvitationAction::Decline
                },
            })),
            4 => Ok(CommandKind::Group(GroupCommand::Toggle)),
            actual => Err(DecodeError::InvalidGroupField { actual }),
        },
        17 => Ok(CommandKind::Who),
        18 => Ok(CommandKind::Exchange(match reader.read_u8()? {
            1 => ExchangeCommand::AddItem {
                slot: decode_item_slot(reader)?,
                quantity: reader.read_u8()?,
            },
            2 => ExchangeCommand::SetGold(reader.read_u32()?),
            3 => ExchangeCommand::Accept,
            4 => ExchangeCommand::Cancel,
            actual => return Err(DecodeError::InvalidExchangeField { actual }),
        })),
        19 => {
            let length = usize::from(reader.read_u8()?);
            let text = std::str::from_utf8(reader.take(length)?)
                .ok()
                .and_then(ChantText::new)
                .ok_or(DecodeError::InvalidChantText)?;
            Ok(CommandKind::Chant(text))
        }
        20 => Ok(CommandKind::Legend),
        21 => {
            let direction = match reader.read_u8()? {
                0 => RawPacketDirection::Client,
                1 => RawPacketDirection::Server,
                actual => return Err(DecodeError::InvalidRawPacketDirection { actual }),
            };
            let command = reader.read_u8()?;
            let length = usize::from(reader.read_u8()?);
            let payload = reader.take(length)?;
            Ok(CommandKind::Raw(
                RawPacket::new(direction, command, payload)
                    .expect("a wire-sized raw packet payload always fits"),
            ))
        }
        22 => Ok(CommandKind::Assail),
        23 => NonZeroU32::new(reader.read_u32()?)
            .map(CommandKind::InspectPlayer)
            .ok_or(DecodeError::InvalidTransferTarget { actual: 1 }),
        24 => Ok(CommandKind::Resync),
        25 => {
            let channel = reader.read_u8()?;
            let recipient = if channel == 2 {
                let length = usize::from(reader.read_u8()?);
                let value = std::str::from_utf8(reader.take(length)?).ok();
                Some(
                    value
                        .and_then(MessageRecipient::new)
                        .ok_or(DecodeError::InvalidMessageRecipient)?,
                )
            } else {
                None
            };
            let length = usize::from(reader.read_u8()?);
            let content = std::str::from_utf8(reader.take(length)?)
                .ok()
                .and_then(MessageContent::new)
                .ok_or(DecodeError::InvalidMessageContent)?;
            let message = match channel {
                0 => MessageCommand::Say(content),
                1 => MessageCommand::Shout(content),
                2 => MessageCommand::Whisper {
                    recipient: recipient.expect("whisper channel decoded a recipient"),
                    content,
                },
                3 => MessageCommand::Guild(content),
                4 => MessageCommand::Group(content),
                actual => return Err(DecodeError::InvalidMessageChannel { actual }),
            };
            Ok(CommandKind::Message(message))
        }
        29 => {
            let flag = reader.read_u8()?;
            CharacterStat::from_flag(flag)
                .map(CommandKind::AddStat)
                .ok_or(DecodeError::InvalidCharacterStat { actual: flag })
        }
        30 => Ok(CommandKind::SelectFieldMapDestination(
            FieldMapSelectionCommand {
                revision: reader.read_u32()?,
                destination_index: reader.read_u8()?,
            },
        )),
        31 => Ok(CommandKind::DismissMessageDialog(MessageDialogCommand {
            revision: reader.read_u32()?,
            id: reader.read_u32()?,
        })),
        actual => Err(DecodeError::InvalidCommandKind { actual }),
    }
}

fn encode_route_tiles(output: &mut Vec<u8>, map_id: u32, tiles: &[RouteTile]) {
    push_u32(output, map_id);
    push_u16(
        output,
        u16::try_from(tiles.len()).expect("bounded route tile count fits u16"),
    );
    for tile in tiles {
        push_u16(output, tile.x);
        push_u16(output, tile.y);
    }
}

fn decode_route_tiles(
    reader: &mut PayloadReader<'_>,
    max: usize,
) -> Result<Vec<RouteTile>, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::InvalidRouteTileCount {
            actual: length,
            max,
        });
    }
    let mut tiles = Vec::with_capacity(length);
    for _ in 0..length {
        tiles.push(RouteTile {
            x: reader.read_u16()?,
            y: reader.read_u16()?,
        });
    }
    Ok(tiles)
}

fn decode_slot_swap(reader: &mut PayloadReader<'_>) -> Result<SlotSwap, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(SlotSwap::Inventory {
            source: decode_item_slot(reader)?,
            destination: decode_item_slot(reader)?,
        }),
        1 => Ok(SlotSwap::Spellbook {
            source: decode_spell_slot(reader)?,
            destination: decode_spell_slot(reader)?,
        }),
        2 => Ok(SlotSwap::Skillbook {
            source: decode_skill_slot(reader)?,
            destination: decode_skill_slot(reader)?,
        }),
        actual => Err(DecodeError::InvalidCommandKind { actual }),
    }
}

fn decode_skill_slot(reader: &mut PayloadReader<'_>) -> Result<SkillSlot, DecodeError> {
    let actual = reader.read_u8()?;
    SkillSlot::new(actual).ok_or(DecodeError::InvalidSkillSlot {
        actual,
        max: MAX_SKILL_SLOT,
    })
}

fn decode_spell_slot(reader: &mut PayloadReader<'_>) -> Result<SpellSlot, DecodeError> {
    let actual = reader.read_u8()?;
    SpellSlot::new(actual).ok_or(DecodeError::InvalidSpellSlot {
        actual,
        max: MAX_SPELL_SLOT,
    })
}

fn decode_item_slot(reader: &mut PayloadReader<'_>) -> Result<ItemSlot, DecodeError> {
    let actual = reader.read_u8()?;
    ItemSlot::new(actual).ok_or(DecodeError::InvalidItemSlot {
        actual,
        max: MAX_ITEM_SLOT,
    })
}

fn decode_transfer_target(reader: &mut PayloadReader<'_>) -> Result<TransferTarget, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(TransferTarget::Tile(TilePosition {
            x: reader.read_i32()?,
            y: reader.read_i32()?,
        })),
        1 => NonZeroU32::new(reader.read_u32()?)
            .map(TransferTarget::Object)
            .ok_or(DecodeError::InvalidTransferTarget { actual: 1 }),
        actual => Err(DecodeError::InvalidTransferTarget { actual }),
    }
}

fn decode_direction(reader: &mut PayloadReader<'_>) -> Result<Direction, DecodeError> {
    let actual = reader.read_u8()?;
    Direction::from_raw(actual).ok_or(DecodeError::InvalidDirection { actual })
}

#[cfg(test)]
mod tests {
    use super::{
        ChantText, CharacterStat, CommandKind, ItemSlot, MAX_MESSAGE_CONTENT_LEN,
        MAX_RAW_PACKET_PAYLOAD_LEN, MessageCommand, MessageContent, MessageRecipient, RawPacket,
        RawPacketDirection, SkillSlot, SlotSwap, SpellSlot, encode_kind,
    };

    #[test]
    fn npc_chants_preserve_verbatim_item_names() {
        let item = "Dark-Belt  (Fine)";
        let cases = [
            (ChantText::sell(item), "buy my Dark-Belt  (Fine)"),
            (ChantText::sell_all(item), "buy my all Dark-Belt  (Fine)"),
            (ChantText::deposit(item), "i will deposit Dark-Belt  (Fine)"),
            (ChantText::withdraw(item), "give my Dark-Belt  (Fine) back"),
            (ChantText::repair(item), "repair my Dark-Belt  (Fine)"),
            (Some(ChantText::repair_all()), "repair all"),
        ];
        for (actual, expected) in cases {
            assert_eq!(actual.unwrap().as_bytes(), expected.as_bytes());
        }
    }

    #[test]
    fn chant_command_encoding_preserves_exact_text() {
        let command = CommandKind::Chant(ChantText::new("MiXeD, punctuation!  ").unwrap());
        let mut encoded = Vec::new();
        encode_kind(&mut encoded, command);
        assert_eq!(encoded, b"\x13\x15MiXeD, punctuation!  ");
    }

    #[test]
    fn raw_packet_encoding_is_bounded_and_preserves_bytes() {
        let packet = RawPacket::new(RawPacketDirection::Client, 0x7e, &[0x00, 0x03, 0x02]).unwrap();
        let mut encoded = Vec::new();
        encode_kind(&mut encoded, CommandKind::Raw(packet));
        assert_eq!(encoded, [21, 0, 0x7e, 3, 0x00, 0x03, 0x02]);
        assert!(
            RawPacket::new(
                RawPacketDirection::Server,
                0,
                &vec![0; MAX_RAW_PACKET_PAYLOAD_LEN + 1],
            )
            .is_none()
        );
    }

    #[test]
    fn assail_command_has_a_stable_wire_discriminant() {
        let mut encoded = Vec::new();
        encode_kind(&mut encoded, CommandKind::Assail);
        assert_eq!(encoded, [22]);
    }

    #[test]
    fn resync_command_has_a_stable_wire_discriminant() {
        let mut encoded = Vec::new();
        encode_kind(&mut encoded, CommandKind::Resync);
        assert_eq!(encoded, [24]);
    }

    #[test]
    fn add_stat_commands_have_stable_wire_flags() {
        for (stat, flag) in [
            (CharacterStat::Strength, 0x01),
            (CharacterStat::Dexterity, 0x02),
            (CharacterStat::Intelligence, 0x04),
            (CharacterStat::Wisdom, 0x08),
            (CharacterStat::Constitution, 0x10),
        ] {
            let mut encoded = Vec::new();
            encode_kind(&mut encoded, CommandKind::AddStat(stat));
            assert_eq!(encoded, [29, flag]);
        }
    }

    #[test]
    fn message_commands_have_a_stable_bounded_encoding() {
        let content = MessageContent::new("hello").unwrap();
        let recipient = MessageRecipient::new("Eidolon").unwrap();
        let cases = [
            (
                MessageCommand::Say(content),
                b"\x19\x00\x05hello".as_slice(),
            ),
            (
                MessageCommand::Shout(content),
                b"\x19\x01\x05hello".as_slice(),
            ),
            (
                MessageCommand::Whisper { recipient, content },
                b"\x19\x02\x07Eidolon\x05hello".as_slice(),
            ),
            (
                MessageCommand::Guild(content),
                b"\x19\x03\x05hello".as_slice(),
            ),
            (
                MessageCommand::Group(content),
                b"\x19\x04\x05hello".as_slice(),
            ),
        ];
        for (message, expected) in cases {
            let mut encoded = Vec::new();
            encode_kind(&mut encoded, CommandKind::Message(message));
            assert_eq!(encoded, expected);
        }
        assert!(MessageContent::new("").is_none());
        assert!(MessageContent::new(&"x".repeat(MAX_MESSAGE_CONTENT_LEN)).is_some());
        assert!(MessageContent::new(&"x".repeat(MAX_MESSAGE_CONTENT_LEN + 1)).is_none());
        assert!(MessageRecipient::new("!!").is_none());
        assert!(MessageRecipient::new("#").is_none());
    }

    #[test]
    fn slot_swap_command_encoding_preserves_native_panel_order() {
        let cases = [
            (
                SlotSwap::Inventory {
                    source: ItemSlot::new(1).unwrap(),
                    destination: ItemSlot::new(59).unwrap(),
                },
                [13, 0, 1, 59],
            ),
            (
                SlotSwap::Spellbook {
                    source: SpellSlot::new(2).unwrap(),
                    destination: SpellSlot::new(90).unwrap(),
                },
                [13, 1, 2, 90],
            ),
            (
                SlotSwap::Skillbook {
                    source: SkillSlot::new(3).unwrap(),
                    destination: SkillSlot::new(89).unwrap(),
                },
                [13, 2, 3, 89],
            ),
        ];
        for (swap, expected) in cases {
            let mut encoded = Vec::new();
            encode_kind(&mut encoded, CommandKind::SwapSlots(swap));
            assert_eq!(encoded, expected);
        }
    }
}
