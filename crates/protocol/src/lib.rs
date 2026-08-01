//! Binary interprocess communication protocol for daRPC.

mod error;
mod frame;
mod message;
mod session;
mod snapshot;

pub use error::{DecodeError, EncodeError};
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
