use crate::{
    DecodeError, EncodeError,
    command::{self, CommandRequest, CommandResponse},
    event::{self, EventPollRequest, EventPollResponse},
    snapshot::{
        self, SnapshotRequest, SnapshotResponse, SnapshotResult, SnapshotUnavailableReason,
    },
};

pub const MAX_ECHO_TEXT_LEN: usize = 4 * 1024;
pub const PROTOCOL_VERSION_1_0: u16 = protocol_version(1, 0);
pub const PROTOCOL_VERSION_1_1: u16 = protocol_version(1, 1);
pub const PROTOCOL_VERSION_1_2: u16 = protocol_version(1, 2);
pub const SUPPORTED_VERSIONS: VersionRange = VersionRange {
    min: PROTOCOL_VERSION_1_2,
    max: PROTOCOL_VERSION_1_2,
};

#[must_use]
pub const fn protocol_version(major: u8, minor: u8) -> u16 {
    ((major as u16) << 8) | minor as u16
}

#[must_use]
pub const fn protocol_version_major(version: u16) -> u8 {
    (version >> 8) as u8
}

#[must_use]
pub const fn protocol_version_minor(version: u16) -> u8 {
    version as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionRange {
    pub min: u16,
    pub max: u16,
}

impl VersionRange {
    #[must_use]
    pub const fn contains(self, version: u16) -> bool {
        self.min <= version && version <= self.max
    }

    pub(crate) fn validate_encode(self) -> Result<(), EncodeError> {
        if self.min == 0 || self.min > self.max {
            return Err(EncodeError::InvalidVersionRange {
                min: self.min,
                max: self.max,
            });
        }
        Ok(())
    }

    pub(crate) fn validate_decode(self) -> Result<(), DecodeError> {
        if self.min == 0 || self.min > self.max {
            return Err(DecodeError::InvalidVersionRange {
                min: self.min,
                max: self.max,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Architecture {
    X86,
    X86_64,
}

impl Architecture {
    const fn wire_value(self) -> u8 {
        match self {
            Self::X86 => 1,
            Self::X86_64 => 2,
        }
    }

    fn from_wire(value: u8) -> Result<Self, DecodeError> {
        match value {
            1 => Ok(Self::X86),
            2 => Ok(Self::X86_64),
            actual => Err(DecodeError::InvalidArchitecture { actual }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hello {
    pub protocol_versions: VersionRange,
    pub dll_instance_id: [u8; 16],
    pub process_id: u32,
    pub process_creation_time: u64,
    pub architecture: Architecture,
    pub dll_version: ComponentVersion,
    pub executable_fingerprint: [u8; 32],
    pub client_version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HelloAck {
    pub selected_version: u16,
    pub dll_instance_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ping {
    pub request_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pong {
    pub request_id: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EchoRequest {
    pub request_id: u32,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EchoResponse {
    pub request_id: u32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickHealthRequest {
    pub request_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickHealthResponse {
    pub request_id: u32,
    pub installed: bool,
    pub relocated_bytes: u8,
    pub tick_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 1,
    HelloAck = 2,
    Ping = 3,
    Pong = 4,
    EchoRequest = 5,
    EchoResponse = 6,
    TickHealthRequest = 7,
    TickHealthResponse = 8,
    SnapshotRequest = 9,
    SnapshotResponse = 10,
    EventPollRequest = 11,
    EventPollResponse = 12,
    CommandRequest = 13,
    CommandResponse = 14,
}

impl MessageType {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        self as u16
    }

    pub(crate) fn from_wire(value: u16) -> Result<Self, DecodeError> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloAck),
            3 => Ok(Self::Ping),
            4 => Ok(Self::Pong),
            5 => Ok(Self::EchoRequest),
            6 => Ok(Self::EchoResponse),
            7 => Ok(Self::TickHealthRequest),
            8 => Ok(Self::TickHealthResponse),
            9 => Ok(Self::SnapshotRequest),
            10 => Ok(Self::SnapshotResponse),
            11 => Ok(Self::EventPollRequest),
            12 => Ok(Self::EventPollResponse),
            13 => Ok(Self::CommandRequest),
            14 => Ok(Self::CommandResponse),
            actual => Err(DecodeError::UnknownMessageType { actual }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Hello(Hello),
    HelloAck(HelloAck),
    Ping(Ping),
    Pong(Pong),
    EchoRequest(EchoRequest),
    EchoResponse(EchoResponse),
    TickHealthRequest(TickHealthRequest),
    TickHealthResponse(TickHealthResponse),
    SnapshotRequest(SnapshotRequest),
    SnapshotResponse(SnapshotResponse),
    EventPollRequest(EventPollRequest),
    EventPollResponse(EventPollResponse),
    CommandRequest(CommandRequest),
    CommandResponse(CommandResponse),
}

impl Message {
    #[must_use]
    pub const fn message_type(&self) -> MessageType {
        match self {
            Self::Hello(_) => MessageType::Hello,
            Self::HelloAck(_) => MessageType::HelloAck,
            Self::Ping(_) => MessageType::Ping,
            Self::Pong(_) => MessageType::Pong,
            Self::EchoRequest(_) => MessageType::EchoRequest,
            Self::EchoResponse(_) => MessageType::EchoResponse,
            Self::TickHealthRequest(_) => MessageType::TickHealthRequest,
            Self::TickHealthResponse(_) => MessageType::TickHealthResponse,
            Self::SnapshotRequest(_) => MessageType::SnapshotRequest,
            Self::SnapshotResponse(_) => MessageType::SnapshotResponse,
            Self::EventPollRequest(_) => MessageType::EventPollRequest,
            Self::EventPollResponse(_) => MessageType::EventPollResponse,
            Self::CommandRequest(_) => MessageType::CommandRequest,
            Self::CommandResponse(_) => MessageType::CommandResponse,
        }
    }

    #[must_use]
    pub const fn is_handshake(&self) -> bool {
        matches!(self, Self::Hello(_) | Self::HelloAck(_))
    }

    pub(crate) fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        let mut output = Vec::new();
        match self {
            Self::Hello(message) => {
                message.protocol_versions.validate_encode()?;
                output.reserve(75);
                push_u16(&mut output, message.protocol_versions.min);
                push_u16(&mut output, message.protocol_versions.max);
                output.extend_from_slice(&message.dll_instance_id);
                push_u32(&mut output, message.process_id);
                push_u64(&mut output, message.process_creation_time);
                output.push(message.architecture.wire_value());
                push_u16(&mut output, message.dll_version.major);
                push_u16(&mut output, message.dll_version.minor);
                push_u16(&mut output, message.dll_version.patch);
                output.extend_from_slice(&message.executable_fingerprint);
                push_u32(&mut output, message.client_version);
            }
            Self::HelloAck(message) => {
                output.reserve(18);
                push_u16(&mut output, message.selected_version);
                output.extend_from_slice(&message.dll_instance_id);
            }
            Self::Ping(message) => push_u32(&mut output, message.request_id),
            Self::Pong(message) => push_u32(&mut output, message.request_id),
            Self::EchoRequest(message) => {
                encode_echo(&mut output, message.request_id, &message.text)?;
            }
            Self::EchoResponse(message) => {
                encode_echo(&mut output, message.request_id, &message.text)?;
            }
            Self::TickHealthRequest(message) => push_u32(&mut output, message.request_id),
            Self::TickHealthResponse(message) => {
                output.reserve(10);
                push_u32(&mut output, message.request_id);
                output.push(u8::from(message.installed));
                output.push(message.relocated_bytes);
                push_u32(&mut output, message.tick_count);
            }
            Self::SnapshotRequest(message) => push_u32(&mut output, message.request_id),
            Self::SnapshotResponse(message) => {
                push_u32(&mut output, message.request_id);
                match &message.result {
                    SnapshotResult::Ready(snapshot) => {
                        output.push(1);
                        snapshot::encode(&mut output, snapshot)?;
                    }
                    SnapshotResult::Unavailable(reason) => {
                        output.push(0);
                        output.push(reason.wire_value());
                    }
                }
            }
            Self::EventPollRequest(message) => event::encode_request(&mut output, *message),
            Self::EventPollResponse(message) => event::encode_response(&mut output, message)?,
            Self::CommandRequest(message) => command::encode_request(&mut output, *message)?,
            Self::CommandResponse(message) => command::encode_response(&mut output, message)?,
        }
        Ok(output)
    }

    pub(crate) fn decode_payload(
        message_type: MessageType,
        payload: &[u8],
    ) -> Result<Self, DecodeError> {
        let mut reader = PayloadReader::new(message_type, payload);
        let message = match message_type {
            MessageType::Hello => {
                let protocol_versions = VersionRange {
                    min: reader.read_u16()?,
                    max: reader.read_u16()?,
                };
                protocol_versions.validate_decode()?;
                Self::Hello(Hello {
                    protocol_versions,
                    dll_instance_id: reader.read_array()?,
                    process_id: reader.read_u32()?,
                    process_creation_time: reader.read_u64()?,
                    architecture: Architecture::from_wire(reader.read_u8()?)?,
                    dll_version: ComponentVersion {
                        major: reader.read_u16()?,
                        minor: reader.read_u16()?,
                        patch: reader.read_u16()?,
                    },
                    executable_fingerprint: reader.read_array()?,
                    client_version: reader.read_u32()?,
                })
            }
            MessageType::HelloAck => Self::HelloAck(HelloAck {
                selected_version: reader.read_u16()?,
                dll_instance_id: reader.read_array()?,
            }),
            MessageType::Ping => Self::Ping(Ping {
                request_id: reader.read_u32()?,
            }),
            MessageType::Pong => Self::Pong(Pong {
                request_id: reader.read_u32()?,
            }),
            MessageType::EchoRequest => {
                let (request_id, text) = decode_echo(&mut reader)?;
                Self::EchoRequest(EchoRequest { request_id, text })
            }
            MessageType::EchoResponse => {
                let (request_id, text) = decode_echo(&mut reader)?;
                Self::EchoResponse(EchoResponse { request_id, text })
            }
            MessageType::TickHealthRequest => Self::TickHealthRequest(TickHealthRequest {
                request_id: reader.read_u32()?,
            }),
            MessageType::TickHealthResponse => Self::TickHealthResponse(TickHealthResponse {
                request_id: reader.read_u32()?,
                installed: reader.read_bool()?,
                relocated_bytes: reader.read_u8()?,
                tick_count: reader.read_u32()?,
            }),
            MessageType::SnapshotRequest => Self::SnapshotRequest(SnapshotRequest {
                request_id: reader.read_u32()?,
            }),
            MessageType::SnapshotResponse => {
                let request_id = reader.read_u32()?;
                let result = match reader.read_u8()? {
                    0 => SnapshotResult::Unavailable(SnapshotUnavailableReason::from_wire(
                        reader.read_u8()?,
                    )?),
                    1 => SnapshotResult::Ready(Box::new(snapshot::decode(&mut reader)?)),
                    actual => return Err(DecodeError::InvalidSnapshotStatus { actual }),
                };
                Self::SnapshotResponse(SnapshotResponse { request_id, result })
            }
            MessageType::EventPollRequest => {
                Self::EventPollRequest(event::decode_request(&mut reader)?)
            }
            MessageType::EventPollResponse => {
                Self::EventPollResponse(event::decode_response(&mut reader)?)
            }
            MessageType::CommandRequest => {
                Self::CommandRequest(command::decode_request(&mut reader)?)
            }
            MessageType::CommandResponse => {
                Self::CommandResponse(command::decode_response(&mut reader)?)
            }
        };
        reader.finish()?;
        Ok(message)
    }
}

fn encode_echo(output: &mut Vec<u8>, request_id: u32, text: &str) -> Result<(), EncodeError> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_ECHO_TEXT_LEN {
        return Err(EncodeError::EchoTooLong {
            length: bytes.len(),
            max: MAX_ECHO_TEXT_LEN,
        });
    }
    let length = u16::try_from(bytes.len()).map_err(|_| EncodeError::LengthOverflow)?;
    output.reserve(6 + bytes.len());
    push_u32(output, request_id);
    push_u16(output, length);
    output.extend_from_slice(bytes);
    Ok(())
}

fn decode_echo(reader: &mut PayloadReader<'_>) -> Result<(u32, String), DecodeError> {
    let request_id = reader.read_u32()?;
    let length = usize::from(reader.read_u16()?);
    if length > MAX_ECHO_TEXT_LEN {
        return Err(DecodeError::EchoTooLong {
            length,
            max: MAX_ECHO_TEXT_LEN,
        });
    }
    let bytes = reader.take(length)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DecodeError::InvalidUtf8)?
        .to_owned();
    Ok((request_id, text))
}

pub(crate) fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_bool(output: &mut Vec<u8>, value: bool) {
    output.push(u8::from(value));
}

pub(crate) fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) struct PayloadReader<'a> {
    message_type: MessageType,
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    const fn new(message_type: MessageType, bytes: &'a [u8]) -> Self {
        Self {
            message_type,
            bytes,
            offset: 0,
        }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn read_i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.read_u8()? as i8)
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool, DecodeError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            actual => Err(DecodeError::InvalidBoolean { actual }),
        }
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_array()?;
        Ok(u16::from_le_bytes(bytes))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.read_array()?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, DecodeError> {
        let bytes = self.read_array()?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn read_u64(&mut self) -> Result<u64, DecodeError> {
        let bytes = self.read_array()?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let bytes = self.take(N)?;
        let mut output = [0; N];
        output.copy_from_slice(bytes);
        Ok(output)
    }

    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], DecodeError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if length > remaining {
            return Err(DecodeError::TruncatedMessage {
                message_type: self.message_type,
                needed: length,
                remaining,
            });
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or(DecodeError::LengthOverflow)?;
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn finish(self) -> Result<(), DecodeError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining != 0 {
            return Err(DecodeError::TrailingMessageBytes {
                message_type: self.message_type,
                remaining,
            });
        }
        Ok(())
    }
}
