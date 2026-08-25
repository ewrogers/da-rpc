//! Binary interprocess communication protocol for daRPC.

mod command;
mod diagnostics;
mod dialog;
mod error;
mod event;
mod exchange;
mod field_map;
mod frame;
mod group;
mod legend;
mod message;
mod message_dialog;
mod player;
mod session;
mod snapshot;

pub use command::{
    ChantText, CharacterStat, CommandFailure, CommandKind, CommandOperation, CommandRequest,
    CommandResponse, CommandResult, CommandState, CommandStatus, DEFAULT_COMMAND_TIMEOUT_MS,
    DialogAction, DialogCommand, DialogText, ExactRouteInvalidState, ExactRouteInvalidStateReason,
    ExchangeCommand, FieldMapSelectionCommand, GoldTransfer, GroupCommand, GroupInvitationAction,
    GroupText, ItemSlot, ItemTransfer, LookTarget, MAX_CHANT_TEXT_LEN, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS, MAX_DIALOG_INPUT_LEN, MAX_GROUP_NAME_LEN, MAX_ITEM_SLOT,
    MAX_MESSAGE_CONTENT_LEN, MAX_MESSAGE_RECIPIENT_LEN, MAX_RAW_PACKET_PAYLOAD_LEN, MAX_SKILL_SLOT,
    MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT, MAX_WALK_ROUTE_TILES, MAX_WHO_NAME_LEN, MAX_WHO_PLAYERS,
    MAX_WHO_TITLE_LEN, MessageCommand, MessageContent, MessageDialogCommand, MessageRecipient,
    RawPacket, RawPacketDirection, RouteTile, SkillSlot, SlotSwap, SpellArguments, SpellCast,
    SpellInput, SpellSlot, SpellTarget, TilePosition, TransferTarget, WalkRoute, WalkTarget,
};
pub use diagnostics::{
    DiagnosticsMode, DiagnosticsOperation, HOOK_TIMING_STAGE_COUNT, HookTimingRecord,
    HookTimingStage,
};
pub use error::{DecodeError, EncodeError};
pub use event::{
    EventPollRequest, EventPollResponse, EventPollResult, MAX_EVENT_POLL_WAIT_MS,
    MAX_EVENTS_PER_POLL, MAX_LOOK_RESULT_TEXT_LEN,
};
pub use exchange::{MAX_EXCHANGE_ITEMS, MAX_EXCHANGE_MESSAGE_LEN, MAX_EXCHANGE_NAME_LEN};
pub use field_map::{MAX_FIELD_MAP_DESTINATIONS, MAX_FIELD_MAP_TEXT_LEN};
pub use frame::{
    FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, MAX_FRAME_LEN,
    MAX_PAYLOAD_LEN, decode_frame, decode_header, encode_frame,
};
pub use legend::{MAX_LEGEND_MARKS, MAX_LEGEND_TAG_LEN, MAX_LEGEND_TEXT_LEN};
pub use message::{
    Architecture, ComponentVersion, DiagnosticsRequest, DiagnosticsResponse, EchoRequest,
    EchoResponse, Hello, HelloAck, MAX_ECHO_TEXT_LEN, Message, MessageType, PROTOCOL_VERSION_1_0,
    PROTOCOL_VERSION_1_1, PROTOCOL_VERSION_1_2, PROTOCOL_VERSION_1_3, PROTOCOL_VERSION_1_4,
    PROTOCOL_VERSION_1_5, PROTOCOL_VERSION_1_6, PROTOCOL_VERSION_1_7, Ping, Pong,
    SUPPORTED_VERSIONS, TickHealthRequest, TickHealthResponse, VersionRange, protocol_version,
    protocol_version_major, protocol_version_minor,
};
pub use message_dialog::{MAX_MESSAGE_DIALOG_TEXT_LEN, MAX_MESSAGE_DIALOGS};
pub use player::{MAX_PLAYER_EQUIPMENT_ITEMS, MAX_PLAYER_IDENTITY_TEXT_LEN};
pub use session::{
    EndpointRole, Handshake, HandshakePhase, MessageDirection, SequenceCounter, SequenceError,
    SessionError, negotiate_version,
};
pub use snapshot::{
    MAX_CHARACTER_NAME_LEN, MAX_MAP_NAME_LEN, MAX_PLANNED_ROUTE_TILES, SnapshotRequest,
    SnapshotResponse, SnapshotResult, SnapshotUnavailableReason,
};

/// Returns the elapsed millisecond ticks using the same wrapping arithmetic as
/// Windows `timeGetTime`.
#[must_use]
pub const fn elapsed_tick_ms(start: u32, end: u32) -> u32 {
    end.wrapping_sub(start)
}
