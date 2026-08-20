use super::*;
use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_i32, push_u16, push_u32},
};
use darpc_model::{
    CharacterClass, LegendMark, TilePosition, UserState, WalkMode, WhoList, WhoPlayer, WorldObject,
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactRouteInvalidStateReason {
    MapTransitionPending,
    NativeMapUnavailable,
    NativeMapMismatch,
    NativeTransitionUnavailable,
    ConfirmedMapMismatch,
    ConfirmedPositionMismatch,
    MapDimensionsUnavailable,
}

impl ExactRouteInvalidStateReason {
    const fn wire_value(self) -> u8 {
        match self {
            Self::MapTransitionPending => 0,
            Self::NativeMapUnavailable => 1,
            Self::NativeMapMismatch => 2,
            Self::NativeTransitionUnavailable => 3,
            Self::ConfirmedMapMismatch => 4,
            Self::ConfirmedPositionMismatch => 5,
            Self::MapDimensionsUnavailable => 6,
        }
    }

    fn from_wire(actual: u8) -> Result<Self, DecodeError> {
        match actual {
            0 => Ok(Self::MapTransitionPending),
            1 => Ok(Self::NativeMapUnavailable),
            2 => Ok(Self::NativeMapMismatch),
            3 => Ok(Self::NativeTransitionUnavailable),
            4 => Ok(Self::ConfirmedMapMismatch),
            5 => Ok(Self::ConfirmedPositionMismatch),
            6 => Ok(Self::MapDimensionsUnavailable),
            actual => Err(DecodeError::InvalidExactRouteStateReason { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactRouteInvalidState {
    pub reason: ExactRouteInvalidStateReason,
    pub route_map_id: u32,
    pub packet_map_id: Option<u32>,
    pub native_map_id: Option<u32>,
    pub packet_position: Option<TilePosition>,
    pub native_position: Option<TilePosition>,
    pub staged_position: Option<TilePosition>,
    pub transition_active: Option<bool>,
    pub route_mode: Option<WalkMode>,
    pub current_destination: Option<TilePosition>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
// Retained statuses include the original bounded command without allocation.
#[allow(clippy::large_enum_variant)]
pub enum CommandResult {
    Status(CommandStatus),
    ExactRouteInvalidState {
        status: CommandStatus,
        diagnostics: ExactRouteInvalidState,
    },
    Who {
        status: CommandStatus,
        list: WhoList,
    },
    Legend {
        status: CommandStatus,
        marks: Vec<LegendMark>,
    },
    Player {
        status: CommandStatus,
        player: Box<WorldObject>,
    },
    Busy,
    NotFound,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

pub(crate) fn encode_response(
    output: &mut Vec<u8>,
    response: &CommandResponse,
) -> Result<(), EncodeError> {
    push_u32(output, response.request_id);
    match &response.result {
        CommandResult::Status(status) => {
            output.push(0);
            encode_status(output, *status);
        }
        CommandResult::ExactRouteInvalidState {
            status,
            diagnostics,
        } => {
            output.push(7);
            encode_status(output, *status);
            encode_exact_route_invalid_state(output, *diagnostics);
        }
        CommandResult::Who { status, list } => {
            output.push(4);
            encode_status(output, *status);
            encode_who(output, list)?;
        }
        CommandResult::Legend { status, marks } => {
            output.push(5);
            encode_status(output, *status);
            crate::legend::encode(output, marks)?;
        }
        CommandResult::Player { status, player } => {
            output.push(6);
            encode_status(output, *status);
            crate::snapshot::objects::encode_object(output, player)?;
            let profile = match player.as_ref() {
                WorldObject::Player {
                    profile: Some(profile),
                    ..
                } => profile,
                _ => {
                    return Err(EncodeError::InvalidPlayerProfileTarget { id: player.id() });
                }
            };
            crate::player::encode_profile(output, profile)?;
        }
        CommandResult::Busy => output.push(1),
        CommandResult::NotFound => output.push(2),
        CommandResult::Unavailable => output.push(3),
    }
    Ok(())
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
        4 => CommandResult::Who {
            status: decode_status(reader)?,
            list: decode_who(reader)?,
        },
        5 => CommandResult::Legend {
            status: decode_status(reader)?,
            marks: crate::legend::decode(reader)?,
        },
        6 => {
            let status = decode_status(reader)?;
            let mut player = crate::snapshot::objects::decode_object(reader)?;
            let profile = Box::new(crate::player::decode_profile(reader)?);
            match &mut player {
                WorldObject::Player {
                    profile: current, ..
                } => *current = Some(profile),
                _ => {
                    return Err(DecodeError::InvalidPlayerProfileTarget { id: player.id() });
                }
            }
            CommandResult::Player {
                status,
                player: Box::new(player),
            }
        }
        7 => CommandResult::ExactRouteInvalidState {
            status: decode_status(reader)?,
            diagnostics: decode_exact_route_invalid_state(reader)?,
        },
        actual => return Err(DecodeError::InvalidCommandResult { actual }),
    };
    Ok(CommandResponse { request_id, result })
}

fn encode_exact_route_invalid_state(output: &mut Vec<u8>, value: ExactRouteInvalidState) {
    output.push(value.reason.wire_value());
    push_u32(output, value.route_map_id);
    push_optional_u32(output, value.packet_map_id);
    push_optional_u32(output, value.native_map_id);
    push_optional_position(output, value.packet_position);
    push_optional_position(output, value.native_position);
    push_optional_position(output, value.staged_position);
    match value.transition_active {
        Some(active) => {
            output.push(1);
            output.push(u8::from(active));
        }
        None => output.push(0),
    }
    match value.route_mode {
        Some(mode) => {
            output.push(1);
            output.push(match mode {
                WalkMode::NativeRoute => 0,
                WalkMode::ExactRoute => 1,
                WalkMode::Direct => 2,
                WalkMode::Pursuit => 3,
            });
        }
        None => output.push(0),
    }
    push_optional_position(output, value.current_destination);
}

fn decode_exact_route_invalid_state(
    reader: &mut PayloadReader<'_>,
) -> Result<ExactRouteInvalidState, DecodeError> {
    Ok(ExactRouteInvalidState {
        reason: ExactRouteInvalidStateReason::from_wire(reader.read_u8()?)?,
        route_map_id: reader.read_u32()?,
        packet_map_id: read_optional_u32(reader)?,
        native_map_id: read_optional_u32(reader)?,
        packet_position: read_optional_position(reader)?,
        native_position: read_optional_position(reader)?,
        staged_position: read_optional_position(reader)?,
        transition_active: if reader.read_bool()? {
            Some(reader.read_bool()?)
        } else {
            None
        },
        route_mode: if reader.read_bool()? {
            Some(match reader.read_u8()? {
                0 => WalkMode::NativeRoute,
                1 => WalkMode::ExactRoute,
                2 => WalkMode::Direct,
                3 => WalkMode::Pursuit,
                actual => return Err(DecodeError::InvalidMovementMode { actual }),
            })
        } else {
            None
        },
        current_destination: read_optional_position(reader)?,
    })
}

fn push_optional_position(output: &mut Vec<u8>, value: Option<TilePosition>) {
    output.push(u8::from(value.is_some()));
    if let Some(value) = value {
        push_i32(output, value.x);
        push_i32(output, value.y);
    }
}

fn read_optional_position(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<TilePosition>, DecodeError> {
    if reader.read_bool()? {
        Ok(Some(TilePosition {
            x: reader.read_i32()?,
            y: reader.read_i32()?,
        }))
    } else {
        Ok(None)
    }
}

fn encode_who(output: &mut Vec<u8>, list: &WhoList) -> Result<(), EncodeError> {
    if list.players.len() > MAX_WHO_PLAYERS {
        return Err(EncodeError::WhoListTooLong {
            length: list.players.len(),
            max: MAX_WHO_PLAYERS,
        });
    }
    push_u16(output, list.world_count);
    push_u16(output, list.country_count);
    push_u16(
        output,
        u16::try_from(list.players.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    for player in &list.players {
        encode_who_string(output, &player.name, MAX_WHO_NAME_LEN)?;
        encode_who_string(output, &player.title, MAX_WHO_TITLE_LEN)?;
        output.push(player.class.raw());
        output.push(player.state.raw());
        output.push(player.color);
        output.push(u8::from(player.is_master));
        output.push(u8::from(player.is_guildmate));
    }
    Ok(())
}

fn decode_who(reader: &mut PayloadReader<'_>) -> Result<WhoList, DecodeError> {
    let world_count = reader.read_u16()?;
    let country_count = reader.read_u16()?;
    let count = usize::from(reader.read_u16()?);
    if count > MAX_WHO_PLAYERS {
        return Err(DecodeError::WhoListTooLong {
            length: count,
            max: MAX_WHO_PLAYERS,
        });
    }
    let mut players = Vec::with_capacity(count);
    for _ in 0..count {
        players.push(WhoPlayer {
            name: decode_who_string(reader, MAX_WHO_NAME_LEN)?,
            title: decode_who_string(reader, MAX_WHO_TITLE_LEN)?,
            class: CharacterClass::from_raw(reader.read_u8()?),
            state: UserState::from_raw(reader.read_u8()?),
            color: reader.read_u8()?,
            is_master: reader.read_bool()?,
            is_guildmate: reader.read_bool()?,
        });
    }
    Ok(WhoList {
        world_count,
        country_count,
        players,
    })
}

fn encode_who_string(output: &mut Vec<u8>, value: &str, max: usize) -> Result<(), EncodeError> {
    if value.len() > max {
        return Err(EncodeError::WhoStringTooLong {
            length: value.len(),
            max,
        });
    }
    output.push(u8::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow)?);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_who_string(reader: &mut PayloadReader<'_>, max: usize) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u8()?);
    if length > max {
        return Err(DecodeError::WhoStringTooLong { length, max });
    }
    String::from_utf8(reader.take(length)?.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
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
