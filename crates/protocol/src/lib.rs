//! Binary interprocess communication protocol for daRPC.

mod error;
mod frame;
mod message;
mod session;

pub use error::{DecodeError, EncodeError};
pub use frame::{
    FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, MAX_FRAME_LEN,
    MAX_PAYLOAD_LEN, decode_frame, decode_header, encode_frame,
};
pub use message::{
    Architecture, ComponentVersion, EchoRequest, EchoResponse, Hello, HelloAck, MAX_ECHO_TEXT_LEN,
    Message, MessageType, Ping, Pong, SUPPORTED_VERSIONS, VersionRange,
};
pub use session::{
    EndpointRole, Handshake, HandshakePhase, MessageDirection, SequenceCounter, SequenceError,
    SessionError, negotiate_version,
};

/// Returns the elapsed millisecond ticks using the same wrapping arithmetic as
/// Windows `timeGetTime`.
#[must_use]
pub const fn elapsed_tick_ms(start: u32, end: u32) -> u32 {
    end.wrapping_sub(start)
}
