use crate::message::{PayloadReader, push_u32, push_u64};
use crate::{DecodeError, DiagnosticsRequest, DiagnosticsResponse, EncodeError};

pub const HOOK_TIMING_STAGE_COUNT: usize = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DiagnosticsOperation {
    Query = 0,
    EnableHookTiming = 1,
    Disable = 2,
    Reset = 3,
}

impl DiagnosticsOperation {
    const fn wire_value(self) -> u8 {
        self as u8
    }

    fn from_wire(value: u8) -> Result<Self, DecodeError> {
        match value {
            0 => Ok(Self::Query),
            1 => Ok(Self::EnableHookTiming),
            2 => Ok(Self::Disable),
            3 => Ok(Self::Reset),
            actual => Err(DecodeError::InvalidDiagnosticsOperation { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DiagnosticsMode {
    Disabled = 0,
    HookTiming = 1,
}

impl DiagnosticsMode {
    const fn wire_value(self) -> u8 {
        self as u8
    }

    fn from_wire(value: u8) -> Result<Self, DecodeError> {
        match value {
            0 => Ok(Self::Disabled),
            1 => Ok(Self::HookTiming),
            actual => Err(DecodeError::InvalidDiagnosticsMode { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HookTimingStage {
    Tick = 0,
    Movement = 1,
    Commands = 2,
    Player = 3,
    State = 4,
    Snapshot = 5,
    Event = 6,
}

impl HookTimingStage {
    const fn wire_value(self) -> u8 {
        self as u8
    }

    fn from_wire(value: u8) -> Result<Self, DecodeError> {
        match value {
            0 => Ok(Self::Tick),
            1 => Ok(Self::Movement),
            2 => Ok(Self::Commands),
            3 => Ok(Self::Player),
            4 => Ok(Self::State),
            5 => Ok(Self::Snapshot),
            6 => Ok(Self::Event),
            actual => Err(DecodeError::InvalidHookTimingStage { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookTimingRecord {
    pub stage: HookTimingStage,
    pub budget_us: u32,
    pub call_count: u64,
    pub total_duration_us: u64,
    pub maximum_duration_us: u32,
    pub over_budget_count: u64,
    pub last_duration_us: u32,
}

pub(crate) fn encode_request(output: &mut Vec<u8>, message: DiagnosticsRequest) {
    output.reserve(5);
    push_u32(output, message.request_id);
    output.push(message.operation.wire_value());
}

pub(crate) fn decode_request(
    reader: &mut PayloadReader<'_>,
) -> Result<DiagnosticsRequest, DecodeError> {
    Ok(DiagnosticsRequest {
        request_id: reader.read_u32()?,
        operation: DiagnosticsOperation::from_wire(reader.read_u8()?)?,
    })
}

pub(crate) fn encode_response(
    output: &mut Vec<u8>,
    message: &DiagnosticsResponse,
) -> Result<(), EncodeError> {
    output.reserve(5 + HOOK_TIMING_STAGE_COUNT * 33);
    push_u32(output, message.request_id);
    output.push(message.mode.wire_value());
    for record in message.hook_timings {
        output.push(record.stage.wire_value());
        push_u32(output, record.budget_us);
        push_u64(output, record.call_count);
        push_u64(output, record.total_duration_us);
        push_u32(output, record.maximum_duration_us);
        push_u64(output, record.over_budget_count);
        push_u32(output, record.last_duration_us);
    }
    Ok(())
}

pub(crate) fn decode_response(
    reader: &mut PayloadReader<'_>,
) -> Result<DiagnosticsResponse, DecodeError> {
    let request_id = reader.read_u32()?;
    let mode = DiagnosticsMode::from_wire(reader.read_u8()?)?;
    let mut hook_timings = [HookTimingRecord {
        stage: HookTimingStage::Tick,
        budget_us: 0,
        call_count: 0,
        total_duration_us: 0,
        maximum_duration_us: 0,
        over_budget_count: 0,
        last_duration_us: 0,
    }; HOOK_TIMING_STAGE_COUNT];
    for record in &mut hook_timings {
        *record = HookTimingRecord {
            stage: HookTimingStage::from_wire(reader.read_u8()?)?,
            budget_us: reader.read_u32()?,
            call_count: reader.read_u64()?,
            total_duration_us: reader.read_u64()?,
            maximum_duration_us: reader.read_u32()?,
            over_budget_count: reader.read_u64()?,
            last_duration_us: reader.read_u32()?,
        };
    }
    Ok(DiagnosticsResponse {
        request_id,
        mode,
        hook_timings,
    })
}
