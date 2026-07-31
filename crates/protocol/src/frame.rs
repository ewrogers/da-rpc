use crate::{DecodeError, EncodeError, Message, MessageType};

pub const FRAME_MAGIC: [u8; 4] = *b"DRPC";
pub const FRAME_VERSION: u16 = 1;
pub const FRAME_HEADER_LEN: usize = 20;
pub const MAX_PAYLOAD_LEN: usize = 64 * 1024;
pub const MAX_FRAME_LEN: usize = FRAME_HEADER_LEN + MAX_PAYLOAD_LEN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub message_type: MessageType,
    pub sequence: u16,
    pub sender_tick_ms: u32,
    pub payload_len: usize,
}

impl FrameHeader {
    pub fn frame_len(self) -> Result<usize, DecodeError> {
        FRAME_HEADER_LEN
            .checked_add(self.payload_len)
            .ok_or(DecodeError::LengthOverflow)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub sequence: u16,
    pub sender_tick_ms: u32,
    pub message: Message,
}

impl Frame {
    #[must_use]
    pub const fn new(sequence: u16, sender_tick_ms: u32, message: Message) -> Self {
        Self {
            sequence,
            sender_tick_ms,
            message,
        }
    }
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, EncodeError> {
    let payload = frame.message.encode_payload()?;
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(EncodeError::PayloadTooLarge {
            length: payload.len(),
            max: MAX_PAYLOAD_LEN,
        });
    }

    let payload_len = u32::try_from(payload.len()).map_err(|_| EncodeError::LengthOverflow)?;
    let frame_len = FRAME_HEADER_LEN
        .checked_add(payload.len())
        .ok_or(EncodeError::LengthOverflow)?;
    let mut output = Vec::with_capacity(frame_len);
    output.extend_from_slice(&FRAME_MAGIC);
    output.extend_from_slice(&FRAME_VERSION.to_le_bytes());
    output.extend_from_slice(&frame.message.message_type().wire_value().to_le_bytes());
    output.extend_from_slice(&frame.sequence.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&frame.sender_tick_ms.to_le_bytes());
    output.extend_from_slice(&payload_len.to_le_bytes());
    output.extend_from_slice(&payload);
    Ok(output)
}

pub fn decode_header(bytes: &[u8]) -> Result<FrameHeader, DecodeError> {
    if bytes.len() < FRAME_HEADER_LEN {
        return Err(DecodeError::TruncatedHeader {
            actual: bytes.len(),
            required: FRAME_HEADER_LEN,
        });
    }

    let actual_magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if actual_magic != FRAME_MAGIC {
        return Err(DecodeError::InvalidMagic {
            actual: actual_magic,
        });
    }

    let frame_version = read_u16(bytes, 4);
    if frame_version != FRAME_VERSION {
        return Err(DecodeError::UnsupportedFrameVersion {
            actual: frame_version,
        });
    }

    let message_type = MessageType::from_wire(read_u16(bytes, 6))?;
    let sequence = read_u16(bytes, 8);
    let flags = read_u16(bytes, 10);
    if flags != 0 {
        return Err(DecodeError::NonZeroFlags { actual: flags });
    }

    let sender_tick_ms = read_u32(bytes, 12);
    let payload_len =
        usize::try_from(read_u32(bytes, 16)).map_err(|_| DecodeError::LengthOverflow)?;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLarge {
            length: payload_len,
            max: MAX_PAYLOAD_LEN,
        });
    }

    Ok(FrameHeader {
        message_type,
        sequence,
        sender_tick_ms,
        payload_len,
    })
}

pub fn decode_frame(bytes: &[u8]) -> Result<Frame, DecodeError> {
    let header = decode_header(bytes)?;
    let expected_len = header.frame_len()?;

    if bytes.len() < expected_len {
        return Err(DecodeError::TruncatedFrame {
            actual: bytes.len(),
            required: expected_len,
        });
    }
    if bytes.len() > expected_len {
        return Err(DecodeError::TrailingFrameBytes {
            actual: bytes.len(),
            expected: expected_len,
        });
    }

    let message = Message::decode_payload(header.message_type, &bytes[FRAME_HEADER_LEN..])?;
    Ok(Frame {
        sequence: header.sequence,
        sender_tick_ms: header.sender_tick_ms,
        message,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
