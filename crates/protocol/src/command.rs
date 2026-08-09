use crate::{
    DecodeError, EncodeError,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogCommand {
    pub revision: u32,
    pub action: DialogAction,
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
pub enum WalkTarget {
    Direction(Direction),
    Destination { x: i32, y: i32 },
}

fn encode_kind(output: &mut Vec<u8>, kind: CommandKind) {
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
    }
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

fn decode_kind(reader: &mut PayloadReader<'_>) -> Result<CommandKind, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(CommandKind::Diagnostic),
        1 => Ok(CommandKind::Turn(decode_direction(reader)?)),
        2 => Ok(CommandKind::Walk(match reader.read_u8()? {
            0 => WalkTarget::Direction(decode_direction(reader)?),
            1 => WalkTarget::Destination {
                x: reader.read_i32()?,
                y: reader.read_i32()?,
            },
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
        actual => Err(DecodeError::InvalidCommandKind { actual }),
    }
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
    use super::{CommandKind, ItemSlot, SkillSlot, SlotSwap, SpellSlot, encode_kind};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Submit carries the same bounded pointer-free command representation.
#[allow(clippy::large_enum_variant)]
pub enum CommandOperation {
    Submit {
        kind: CommandKind,
        timeout_ms: u16,
        wait_ms: u16,
    },
    Query {
        command_id: u32,
        wait_ms: u16,
    },
    Cancel {
        command_id: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandRequest {
    pub request_id: u32,
    pub operation: CommandOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandState {
    Accepted,
    Executed,
    Failed,
    Cancelled,
    TimedOut,
}

impl CommandState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Accepted)
    }

    const fn wire_value(self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Executed => 1,
            Self::Failed => 2,
            Self::Cancelled => 3,
            Self::TimedOut => 4,
        }
    }

    fn from_wire(actual: u8) -> Result<Self, DecodeError> {
        match actual {
            0 => Ok(Self::Accepted),
            1 => Ok(Self::Executed),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Cancelled),
            4 => Ok(Self::TimedOut),
            actual => Err(DecodeError::InvalidCommandState { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandFailure {
    Internal,
    InvalidState,
    InvalidDestination,
    Rejected,
    NoPath,
    InvalidSkill,
    InvalidSpell,
    InvalidArguments,
    InvalidTarget,
}

impl CommandFailure {
    const fn wire_value(self) -> u8 {
        match self {
            Self::Internal => 0,
            Self::InvalidState => 1,
            Self::InvalidDestination => 2,
            Self::Rejected => 3,
            Self::NoPath => 4,
            Self::InvalidSkill => 5,
            Self::InvalidSpell => 6,
            Self::InvalidArguments => 7,
            Self::InvalidTarget => 8,
        }
    }

    fn from_wire(actual: u8) -> Result<Self, DecodeError> {
        match actual {
            0 => Ok(Self::Internal),
            1 => Ok(Self::InvalidState),
            2 => Ok(Self::InvalidDestination),
            3 => Ok(Self::Rejected),
            4 => Ok(Self::NoPath),
            5 => Ok(Self::InvalidSkill),
            6 => Ok(Self::InvalidSpell),
            7 => Ok(Self::InvalidArguments),
            8 => Ok(Self::InvalidTarget),
            actual => Err(DecodeError::InvalidCommandFailure { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandStatus {
    pub command_id: u32,
    pub kind: CommandKind,
    pub state: CommandState,
    pub enqueued_tick_ms: u32,
    pub deadline_tick_ms: u32,
    pub started_tick_ms: Option<u32>,
    pub completed_tick_ms: Option<u32>,
    pub execution_us: Option<u32>,
    pub main_thread_id: Option<u32>,
    pub failure: Option<CommandFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Retained statuses include the original bounded command without allocation.
#[allow(clippy::large_enum_variant)]
pub enum CommandResult {
    Status(CommandStatus),
    Busy,
    NotFound,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandResponse {
    pub request_id: u32,
    pub result: CommandResult,
}

pub(crate) fn encode_request(
    output: &mut Vec<u8>,
    request: CommandRequest,
) -> Result<(), EncodeError> {
    push_u32(output, request.request_id);
    match request.operation {
        CommandOperation::Submit {
            kind,
            timeout_ms,
            wait_ms,
        } => {
            validate_timeout_encode(timeout_ms)?;
            validate_wait_encode(wait_ms)?;
            output.push(0);
            encode_kind(output, kind);
            push_u16(output, timeout_ms);
            push_u16(output, wait_ms);
        }
        CommandOperation::Query {
            command_id,
            wait_ms,
        } => {
            validate_command_id_encode(command_id)?;
            validate_wait_encode(wait_ms)?;
            output.push(1);
            push_u32(output, command_id);
            push_u16(output, wait_ms);
        }
        CommandOperation::Cancel { command_id } => {
            validate_command_id_encode(command_id)?;
            output.push(2);
            push_u32(output, command_id);
        }
    }
    Ok(())
}

pub(crate) fn decode_request(
    reader: &mut PayloadReader<'_>,
) -> Result<CommandRequest, DecodeError> {
    let request_id = reader.read_u32()?;
    let operation = match reader.read_u8()? {
        0 => {
            let kind = decode_kind(reader)?;
            let timeout_ms = reader.read_u16()?;
            validate_timeout_decode(timeout_ms)?;
            let wait_ms = reader.read_u16()?;
            validate_wait_decode(wait_ms)?;
            CommandOperation::Submit {
                kind,
                timeout_ms,
                wait_ms,
            }
        }
        1 => {
            let command_id = reader.read_u32()?;
            validate_command_id_decode(command_id)?;
            let wait_ms = reader.read_u16()?;
            validate_wait_decode(wait_ms)?;
            CommandOperation::Query {
                command_id,
                wait_ms,
            }
        }
        2 => {
            let command_id = reader.read_u32()?;
            validate_command_id_decode(command_id)?;
            CommandOperation::Cancel { command_id }
        }
        actual => return Err(DecodeError::InvalidCommandOperation { actual }),
    };
    Ok(CommandRequest {
        request_id,
        operation,
    })
}

pub(crate) fn encode_response(output: &mut Vec<u8>, response: CommandResponse) {
    push_u32(output, response.request_id);
    match response.result {
        CommandResult::Status(status) => {
            output.push(0);
            encode_status(output, status);
        }
        CommandResult::Busy => output.push(1),
        CommandResult::NotFound => output.push(2),
        CommandResult::Unavailable => output.push(3),
    }
}

pub(crate) fn decode_response(
    reader: &mut PayloadReader<'_>,
) -> Result<CommandResponse, DecodeError> {
    let request_id = reader.read_u32()?;
    let result = match reader.read_u8()? {
        0 => CommandResult::Status(decode_status(reader)?),
        1 => CommandResult::Busy,
        2 => CommandResult::NotFound,
        3 => CommandResult::Unavailable,
        actual => return Err(DecodeError::InvalidCommandResult { actual }),
    };
    Ok(CommandResponse { request_id, result })
}

fn encode_status(output: &mut Vec<u8>, status: CommandStatus) {
    push_u32(output, status.command_id);
    encode_kind(output, status.kind);
    output.push(status.state.wire_value());
    push_u32(output, status.enqueued_tick_ms);
    push_u32(output, status.deadline_tick_ms);
    push_optional_u32(output, status.started_tick_ms);
    push_optional_u32(output, status.completed_tick_ms);
    push_optional_u32(output, status.execution_us);
    push_optional_u32(output, status.main_thread_id);
    match status.failure {
        Some(failure) => {
            output.push(1);
            output.push(failure.wire_value());
        }
        None => output.push(0),
    }
}

fn decode_status(reader: &mut PayloadReader<'_>) -> Result<CommandStatus, DecodeError> {
    let command_id = reader.read_u32()?;
    validate_command_id_decode(command_id)?;
    Ok(CommandStatus {
        command_id,
        kind: decode_kind(reader)?,
        state: CommandState::from_wire(reader.read_u8()?)?,
        enqueued_tick_ms: reader.read_u32()?,
        deadline_tick_ms: reader.read_u32()?,
        started_tick_ms: read_optional_u32(reader)?,
        completed_tick_ms: read_optional_u32(reader)?,
        execution_us: read_optional_u32(reader)?,
        main_thread_id: read_optional_u32(reader)?,
        failure: if reader.read_bool()? {
            Some(CommandFailure::from_wire(reader.read_u8()?)?)
        } else {
            None
        },
    })
}

fn push_optional_u32(output: &mut Vec<u8>, value: Option<u32>) {
    output.push(u8::from(value.is_some()));
    if let Some(value) = value {
        push_u32(output, value);
    }
}

fn read_optional_u32(reader: &mut PayloadReader<'_>) -> Result<Option<u32>, DecodeError> {
    if reader.read_bool()? {
        Ok(Some(reader.read_u32()?))
    } else {
        Ok(None)
    }
}

fn validate_command_id_encode(command_id: u32) -> Result<(), EncodeError> {
    if command_id == 0 {
        return Err(EncodeError::InvalidCommandId);
    }
    Ok(())
}

fn validate_command_id_decode(command_id: u32) -> Result<(), DecodeError> {
    if command_id == 0 {
        return Err(DecodeError::InvalidCommandId);
    }
    Ok(())
}

fn validate_timeout_encode(timeout_ms: u16) -> Result<(), EncodeError> {
    if timeout_ms == 0 || timeout_ms > MAX_COMMAND_TIMEOUT_MS {
        return Err(EncodeError::InvalidCommandTimeout {
            actual: timeout_ms,
            max: MAX_COMMAND_TIMEOUT_MS,
        });
    }
    Ok(())
}

fn validate_timeout_decode(timeout_ms: u16) -> Result<(), DecodeError> {
    if timeout_ms == 0 || timeout_ms > MAX_COMMAND_TIMEOUT_MS {
        return Err(DecodeError::InvalidCommandTimeout {
            actual: timeout_ms,
            max: MAX_COMMAND_TIMEOUT_MS,
        });
    }
    Ok(())
}

fn validate_wait_encode(wait_ms: u16) -> Result<(), EncodeError> {
    if wait_ms > MAX_COMMAND_WAIT_MS {
        return Err(EncodeError::InvalidCommandWait {
            actual: wait_ms,
            max: MAX_COMMAND_WAIT_MS,
        });
    }
    Ok(())
}

fn validate_wait_decode(wait_ms: u16) -> Result<(), DecodeError> {
    if wait_ms > MAX_COMMAND_WAIT_MS {
        return Err(DecodeError::InvalidCommandWait {
            actual: wait_ms,
            max: MAX_COMMAND_WAIT_MS,
        });
    }
    Ok(())
}
