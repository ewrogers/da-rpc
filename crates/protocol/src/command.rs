use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_i32, push_u16, push_u32},
};
use darpc_model::Direction;

pub const DEFAULT_COMMAND_TIMEOUT_MS: u16 = 1_000;
pub const MAX_COMMAND_TIMEOUT_MS: u16 = 5_000;
pub const MAX_COMMAND_WAIT_MS: u16 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Diagnostic,
    Turn(Direction),
    Walk(WalkTarget),
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
        actual => Err(DecodeError::InvalidCommandKind { actual }),
    }
}

fn decode_direction(reader: &mut PayloadReader<'_>) -> Result<Direction, DecodeError> {
    let actual = reader.read_u8()?;
    Direction::from_raw(actual).ok_or(DecodeError::InvalidDirection { actual })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

impl CommandFailure {
    const fn wire_value(self) -> u8 {
        match self {
            Self::Internal => 0,
            Self::InvalidState => 1,
            Self::InvalidDestination => 2,
            Self::Rejected => 3,
            Self::NoPath => 4,
        }
    }

    fn from_wire(actual: u8) -> Result<Self, DecodeError> {
        match actual {
            0 => Ok(Self::Internal),
            1 => Ok(Self::InvalidState),
            2 => Ok(Self::InvalidDestination),
            3 => Ok(Self::Rejected),
            4 => Ok(Self::NoPath),
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
