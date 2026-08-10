//! Binary interprocess communication protocol for daRPC.

mod command;
mod dialog;
mod error;
mod event;
mod exchange;
mod frame;
mod group;
mod message;
mod session;
mod snapshot;

pub use command::{
    ChantText, CommandFailure, CommandKind, CommandOperation, CommandRequest, CommandResponse,
    CommandResult, CommandState, CommandStatus, DEFAULT_COMMAND_TIMEOUT_MS, DialogAction,
    DialogCommand, DialogText, ExchangeCommand, GoldTransfer, GroupCommand, GroupInvitationAction,
    GroupText, ItemSlot, ItemTransfer, MAX_CHANT_TEXT_LEN, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS, MAX_DIALOG_INPUT_LEN, MAX_GROUP_NAME_LEN, MAX_ITEM_SLOT, MAX_SKILL_SLOT,
    MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT, MAX_WHO_NAME_LEN, MAX_WHO_PLAYERS, MAX_WHO_TITLE_LEN,
    SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget,
    TilePosition, TransferTarget, WalkTarget,
};
pub use error::{DecodeError, EncodeError};
pub use event::{
    EventPollRequest, EventPollResponse, EventPollResult, MAX_EVENT_POLL_WAIT_MS,
    MAX_EVENTS_PER_POLL,
};
pub use exchange::{MAX_EXCHANGE_ITEMS, MAX_EXCHANGE_MESSAGE_LEN, MAX_EXCHANGE_NAME_LEN};
pub use frame::{
    FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, MAX_FRAME_LEN,
    MAX_PAYLOAD_LEN, decode_frame, decode_header, encode_frame,
};
pub use message::{
    Architecture, ComponentVersion, EchoRequest, EchoResponse, Hello, HelloAck, MAX_ECHO_TEXT_LEN,
    Message, MessageType, PROTOCOL_VERSION_1_0, Ping, Pong, SUPPORTED_VERSIONS, TickHealthRequest,
    TickHealthResponse, VersionRange, protocol_version, protocol_version_major,
    protocol_version_minor,
};
pub use session::{
    EndpointRole, Handshake, HandshakePhase, MessageDirection, SequenceCounter, SequenceError,
    SessionError, negotiate_version,
};
pub use snapshot::{
    MAX_CHARACTER_NAME_LEN, MAX_MAP_NAME_LEN, SnapshotRequest, SnapshotResponse, SnapshotResult,
    SnapshotUnavailableReason,
};

/// Returns the elapsed millisecond ticks using the same wrapping arithmetic as
/// Windows `timeGetTime`.
#[must_use]
pub const fn elapsed_tick_ms(start: u32, end: u32) -> u32 {
    end.wrapping_sub(start)
}
