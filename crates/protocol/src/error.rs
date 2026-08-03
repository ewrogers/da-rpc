use crate::{MessageType, protocol_version_major, protocol_version_minor};
use std::{error::Error, fmt};

/// Failure while converting a typed message into its wire representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    InvalidVersionRange { min: u16, max: u16 },
    EchoTooLong { length: usize, max: usize },
    SnapshotStringTooLong { length: usize, max: usize },
    SnapshotCollectionTooLong { length: usize, max: usize },
    InvalidSnapshotSlot { slot: u8, max: u8 },
    DuplicateSnapshotSlot { slot: u8 },
    DuplicateEffectIcon { icon: u16 },
    DuplicateWorldObjectId { id: u32 },
    EventBatchTooLong { length: usize, max: usize },
    EventStringTooLong { length: usize, max: usize },
    InvalidCollectionBatch { index: u8, count: u8 },
    EmptyCollectionUpdate,
    InvalidMovementOutcome,
    InvalidAbilitySlot { slot: u8 },
    InvalidSpellProgress { line: u8, total: u8 },
    InvalidSpellValues { count: usize },
    InvalidCommandId,
    InvalidCommandTimeout { actual: u16, max: u16 },
    InvalidCommandWait { actual: u16, max: u16 },
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
            Self::SnapshotStringTooLong { length, max } => write!(
                formatter,
                "snapshot string is {length} bytes; maximum is {max}"
            ),
            Self::SnapshotCollectionTooLong { length, max } => write!(
                formatter,
                "snapshot collection has {length} entries; maximum is {max}"
            ),
            Self::InvalidSnapshotSlot { slot, max } => {
                write!(formatter, "snapshot slot {slot} is outside 1..={max}")
            }
            Self::DuplicateSnapshotSlot { slot } => {
                write!(formatter, "snapshot slot {slot} appears more than once")
            }
            Self::DuplicateEffectIcon { icon } => {
                write!(formatter, "spell effect icon {icon} appears more than once")
            }
            Self::DuplicateWorldObjectId { id } => {
                write!(formatter, "world object ID {id} appears more than once")
            }
            Self::EventBatchTooLong { length, max } => {
                write!(
                    formatter,
                    "event batch has {length} entries; maximum is {max}"
                )
            }
            Self::EventStringTooLong { length, max } => write!(
                formatter,
                "state-event string is {length} bytes; maximum is {max}"
            ),
            Self::InvalidCollectionBatch { index, count } => {
                write!(
                    formatter,
                    "invalid collection batch position {index} of {count}"
                )
            }
            Self::EmptyCollectionUpdate => {
                formatter.write_str("collection update has no before or after value")
            }
            Self::InvalidMovementOutcome => formatter
                .write_str("movement completion requires matching destination and reached fields"),
            Self::InvalidAbilitySlot { slot } => {
                write!(formatter, "ability slot {slot} is outside 1..=90")
            }
            Self::InvalidSpellProgress { line, total } => {
                write!(
                    formatter,
                    "spell chant line {line} is invalid for {total} total lines"
                )
            }
            Self::InvalidSpellValues { count } => {
                write!(
                    formatter,
                    "spell cast contains {count} numeric values; expected 1..=4"
                )
            }
            Self::InvalidCommandId => formatter.write_str("command ID must be nonzero"),
            Self::InvalidCommandTimeout { actual, max } => write!(
                formatter,
                "command timeout is {actual} ms; expected 1..={max} ms"
            ),
            Self::InvalidCommandWait { actual, max } => write!(
                formatter,
                "command wait is {actual} ms; maximum is {max} ms"
            ),
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
    SnapshotStringTooLong {
        length: usize,
        max: usize,
    },
    SnapshotCollectionTooLong {
        length: usize,
        max: usize,
    },
    InvalidSnapshotSlot {
        slot: u8,
        max: u8,
    },
    DuplicateSnapshotSlot {
        slot: u8,
    },
    DuplicateEffectIcon {
        icon: u16,
    },
    DuplicateWorldObjectId {
        id: u32,
    },
    InvalidWorldObjectType {
        actual: u8,
    },
    InvalidCreatureKind {
        actual: u8,
    },
    InvalidDirection {
        actual: u8,
    },
    InvalidEffectDuration {
        actual: u8,
    },
    InvalidEffectUpdateType {
        actual: u8,
    },
    InvalidObjectUpdateType {
        actual: u8,
    },
    InvalidMessageKind {
        actual: u8,
    },
    InvalidCollectionBatch {
        index: u8,
        count: u8,
    },
    InvalidCollectionChange {
        actual: u8,
    },
    InvalidCollectionFields {
        actual: u8,
    },
    CollectionSlotMismatch {
        slot: u8,
    },
    InvalidSnapshotStatus {
        actual: u8,
    },
    InvalidSnapshotUnavailableReason {
        actual: u8,
    },
    EventBatchTooLong {
        length: usize,
        max: usize,
    },
    EventStringTooLong {
        length: usize,
        max: usize,
    },
    InvalidEventPollStatus {
        actual: u8,
    },
    InvalidStateUpdateType {
        actual: u8,
    },
    InvalidStatusFields {
        actual: u8,
    },
    InvalidMovementUpdateType {
        actual: u8,
    },
    InvalidMovementDestination {
        actual: u8,
    },
    InvalidMovementOutcome {
        actual: u8,
        has_destination: bool,
    },
    InvalidAbilityUpdateType {
        actual: u8,
    },
    InvalidAbilitySlot {
        actual: u8,
    },
    InvalidSpellProgress {
        line: u8,
        total: u8,
    },
    InvalidSpellCastArguments {
        actual: u8,
    },
    InvalidSpellCancellationSource {
        actual: u8,
    },
    InvalidSpellValues {
        count: usize,
    },
    InvalidClientLifecycle {
        actual: u8,
    },
    InvalidCommandId,
    InvalidCommandOperation {
        actual: u8,
    },
    InvalidCommandKind {
        actual: u8,
    },
    InvalidWalkTarget {
        actual: u8,
    },
    InvalidSkillSlot {
        actual: u8,
        max: u8,
    },
    InvalidSpellSlot {
        actual: u8,
        max: u8,
    },
    InvalidSpellArguments {
        actual: u8,
    },
    InvalidSpellTarget,
    InvalidSpellInput,
    InvalidCommandState {
        actual: u8,
    },
    InvalidCommandFailure {
        actual: u8,
    },
    InvalidCommandResult {
        actual: u8,
    },
    InvalidCommandTimeout {
        actual: u16,
        max: u16,
    },
    InvalidCommandWait {
        actual: u16,
        max: u16,
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
            Self::SnapshotStringTooLong { length, max } => write!(
                formatter,
                "snapshot string is {length} bytes; maximum is {max}"
            ),
            Self::SnapshotCollectionTooLong { length, max } => write!(
                formatter,
                "snapshot collection has {length} entries; maximum is {max}"
            ),
            Self::InvalidSnapshotSlot { slot, max } => {
                write!(formatter, "snapshot slot {slot} is outside 1..={max}")
            }
            Self::DuplicateSnapshotSlot { slot } => {
                write!(formatter, "snapshot slot {slot} appears more than once")
            }
            Self::DuplicateEffectIcon { icon } => {
                write!(formatter, "spell effect icon {icon} appears more than once")
            }
            Self::DuplicateWorldObjectId { id } => {
                write!(formatter, "world object ID {id} appears more than once")
            }
            Self::InvalidWorldObjectType { actual } => {
                write!(formatter, "invalid world object type {actual}")
            }
            Self::InvalidCreatureKind { actual } => {
                write!(formatter, "invalid creature kind {actual}")
            }
            Self::InvalidDirection { actual } => {
                write!(formatter, "invalid direction {actual}")
            }
            Self::InvalidEffectDuration { actual } => {
                write!(formatter, "invalid spell effect duration {actual}")
            }
            Self::InvalidEffectUpdateType { actual } => {
                write!(formatter, "invalid spell effect update type {actual}")
            }
            Self::InvalidObjectUpdateType { actual } => {
                write!(formatter, "invalid world object update type {actual}")
            }
            Self::InvalidMessageKind { actual } => {
                write!(formatter, "invalid client message kind {actual}")
            }
            Self::InvalidCollectionBatch { index, count } => {
                write!(
                    formatter,
                    "invalid collection batch position {index} of {count}"
                )
            }
            Self::InvalidCollectionChange { actual } => {
                write!(formatter, "invalid collection change type {actual}")
            }
            Self::InvalidCollectionFields { actual } => {
                write!(formatter, "invalid collection field mask 0x{actual:02X}")
            }
            Self::CollectionSlotMismatch { slot } => {
                write!(formatter, "collection value does not belong to slot {slot}")
            }
            Self::InvalidSnapshotStatus { actual } => {
                write!(formatter, "invalid snapshot status {actual}")
            }
            Self::InvalidSnapshotUnavailableReason { actual } => {
                write!(formatter, "invalid snapshot unavailable reason {actual}")
            }
            Self::EventBatchTooLong { length, max } => {
                write!(
                    formatter,
                    "event batch has {length} entries; maximum is {max}"
                )
            }
            Self::EventStringTooLong { length, max } => write!(
                formatter,
                "state-event string is {length} bytes; maximum is {max}"
            ),
            Self::InvalidEventPollStatus { actual } => {
                write!(formatter, "invalid event poll status {actual}")
            }
            Self::InvalidStateUpdateType { actual } => {
                write!(formatter, "invalid state update type {actual}")
            }
            Self::InvalidStatusFields { actual } => {
                write!(formatter, "invalid status field mask 0x{actual:02X}")
            }
            Self::InvalidMovementUpdateType { actual } => {
                write!(formatter, "invalid movement update type {actual}")
            }
            Self::InvalidMovementDestination { actual } => {
                write!(formatter, "invalid movement destination marker {actual}")
            }
            Self::InvalidMovementOutcome {
                actual,
                has_destination,
            } => write!(
                formatter,
                "invalid movement outcome {actual} with destination present={has_destination}"
            ),
            Self::InvalidAbilityUpdateType { actual } => {
                write!(formatter, "invalid ability update type {actual}")
            }
            Self::InvalidAbilitySlot { actual } => {
                write!(formatter, "ability slot {actual} is outside 1..=90")
            }
            Self::InvalidSpellProgress { line, total } => {
                write!(
                    formatter,
                    "spell chant line {line} is invalid for {total} total lines"
                )
            }
            Self::InvalidSpellCastArguments { actual } => {
                write!(formatter, "invalid spell cast argument type {actual}")
            }
            Self::InvalidSpellCancellationSource { actual } => {
                write!(formatter, "invalid spell cancellation source {actual}")
            }
            Self::InvalidSpellValues { count } => {
                write!(
                    formatter,
                    "spell cast contains {count} numeric values; expected 1..=4"
                )
            }
            Self::InvalidClientLifecycle { actual } => {
                write!(formatter, "invalid client lifecycle {actual}")
            }
            Self::InvalidCommandId => formatter.write_str("command ID must be nonzero"),
            Self::InvalidCommandOperation { actual } => {
                write!(formatter, "invalid command operation {actual}")
            }
            Self::InvalidCommandKind { actual } => {
                write!(formatter, "invalid command kind {actual}")
            }
            Self::InvalidWalkTarget { actual } => {
                write!(formatter, "invalid walk target {actual}")
            }
            Self::InvalidSkillSlot { actual, max } => {
                write!(formatter, "skill slot {actual} is outside 1..={max}")
            }
            Self::InvalidSpellSlot { actual, max } => {
                write!(formatter, "spell slot {actual} is outside 1..={max}")
            }
            Self::InvalidSpellArguments { actual } => {
                write!(formatter, "invalid spell argument type {actual}")
            }
            Self::InvalidSpellTarget => formatter.write_str("spell object target must be nonzero"),
            Self::InvalidSpellInput => {
                formatter.write_str("spell input must contain from 1 through 100 ASCII bytes")
            }
            Self::InvalidCommandState { actual } => {
                write!(formatter, "invalid command state {actual}")
            }
            Self::InvalidCommandFailure { actual } => {
                write!(formatter, "invalid command failure {actual}")
            }
            Self::InvalidCommandResult { actual } => {
                write!(formatter, "invalid command result {actual}")
            }
            Self::InvalidCommandTimeout { actual, max } => write!(
                formatter,
                "command timeout is {actual} ms; expected 1..={max} ms"
            ),
            Self::InvalidCommandWait { actual, max } => write!(
                formatter,
                "command wait is {actual} ms; maximum is {max} ms"
            ),
            Self::InvalidUtf8 => formatter.write_str("message text is not valid UTF-8"),
        }
    }
}

impl Error for DecodeError {}
