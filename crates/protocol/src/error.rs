use crate::{MessageType, protocol_version_major, protocol_version_minor};
use std::{error::Error, fmt};

/// Failure while converting a typed message into its wire representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    InvalidVersionRange { min: u16, max: u16 },
    EchoTooLong { length: usize, max: usize },
    PayloadTooLarge { length: usize, max: usize },
    LengthOverflow,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersionRange { min, max } => {
                write!(
                    formatter,
                    "invalid protocol version range {}.{}..={}.{}",
                    protocol_version_major(*min),
                    protocol_version_minor(*min),
                    protocol_version_major(*max),
                    protocol_version_minor(*max)
                )
            }
            Self::EchoTooLong { length, max } => {
                write!(formatter, "echo text is {length} bytes; maximum is {max}")
            }
            Self::PayloadTooLarge { length, max } => {
                write!(formatter, "payload is {length} bytes; maximum is {max}")
            }
            Self::LengthOverflow => formatter.write_str("encoded frame length overflow"),
        }
    }
}

impl Error for EncodeError {}

/// Failure while validating or decoding untrusted wire bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    TruncatedHeader {
        actual: usize,
        required: usize,
    },
    InvalidMagic {
        actual: [u8; 4],
    },
    UnsupportedFrameVersion {
        actual: u16,
    },
    UnknownMessageType {
        actual: u16,
    },
    NonZeroFlags {
        actual: u16,
    },
    PayloadTooLarge {
        length: usize,
        max: usize,
    },
    LengthOverflow,
    TruncatedFrame {
        actual: usize,
        required: usize,
    },
    TrailingFrameBytes {
        actual: usize,
        expected: usize,
    },
    TruncatedMessage {
        message_type: MessageType,
        needed: usize,
        remaining: usize,
    },
    TrailingMessageBytes {
        message_type: MessageType,
        remaining: usize,
    },
    InvalidVersionRange {
        min: u16,
        max: u16,
    },
    InvalidArchitecture {
        actual: u8,
    },
    InvalidBoolean {
        actual: u8,
    },
    EchoTooLong {
        length: usize,
        max: usize,
    },
    InvalidUtf8,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { actual, required } => write!(
                formatter,
                "frame header is truncated: received {actual} bytes, require {required}"
            ),
            Self::InvalidMagic { actual } => {
                write!(formatter, "invalid frame magic {actual:02X?}")
            }
            Self::UnsupportedFrameVersion { actual } => {
                write!(formatter, "unsupported frame version {actual}")
            }
            Self::UnknownMessageType { actual } => {
                write!(formatter, "unknown message type {actual}")
            }
            Self::NonZeroFlags { actual } => {
                write!(formatter, "unsupported frame flags 0x{actual:04X}")
            }
            Self::PayloadTooLarge { length, max } => {
                write!(formatter, "payload is {length} bytes; maximum is {max}")
            }
            Self::LengthOverflow => formatter.write_str("decoded frame length overflow"),
            Self::TruncatedFrame { actual, required } => write!(
                formatter,
                "frame is truncated: received {actual} bytes, require {required}"
            ),
            Self::TrailingFrameBytes { actual, expected } => write!(
                formatter,
                "frame has trailing bytes: received {actual} bytes, expected {expected}"
            ),
            Self::TruncatedMessage {
                message_type,
                needed,
                remaining,
            } => write!(
                formatter,
                "{message_type:?} payload is truncated: need {needed} bytes, {remaining} remain"
            ),
            Self::TrailingMessageBytes {
                message_type,
                remaining,
            } => write!(
                formatter,
                "{message_type:?} payload has {remaining} trailing bytes"
            ),
            Self::InvalidVersionRange { min, max } => {
                write!(
                    formatter,
                    "invalid protocol version range {}.{}..={}.{}",
                    protocol_version_major(*min),
                    protocol_version_minor(*min),
                    protocol_version_major(*max),
                    protocol_version_minor(*max)
                )
            }
            Self::InvalidArchitecture { actual } => {
                write!(formatter, "invalid architecture value {actual}")
            }
            Self::InvalidBoolean { actual } => {
                write!(formatter, "invalid Boolean value {actual}")
            }
            Self::EchoTooLong { length, max } => {
                write!(formatter, "echo text is {length} bytes; maximum is {max}")
            }
            Self::InvalidUtf8 => formatter.write_str("echo text is not valid UTF-8"),
        }
    }
}

impl Error for DecodeError {}
