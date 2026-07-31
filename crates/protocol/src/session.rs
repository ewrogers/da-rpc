use crate::{HelloAck, Message, MessageType, SUPPORTED_VERSIONS, VersionRange};
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointRole {
    Dll,
    Controller,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageDirection {
    Inbound,
    Outbound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakePhase {
    SendHello,
    ReceiveHello,
    SendHelloAck,
    ReceiveHelloAck,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    InvalidVersionRange {
        min: u16,
        max: u16,
    },
    UnsupportedVersionRange {
        min: u16,
        max: u16,
    },
    InvalidSelectedVersion {
        selected: u16,
    },
    InstanceMismatch,
    UnexpectedMessage {
        role: EndpointRole,
        phase: HandshakePhase,
        direction: MessageDirection,
        message_type: MessageType,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersionRange { min, max } => {
                write!(formatter, "invalid protocol version range {min}..={max}")
            }
            Self::UnsupportedVersionRange { min, max } => {
                write!(
                    formatter,
                    "unsupported protocol version range {min}..={max}"
                )
            }
            Self::InvalidSelectedVersion { selected } => {
                write!(formatter, "invalid selected protocol version {selected}")
            }
            Self::InstanceMismatch => formatter.write_str("DLL instance ID does not match Hello"),
            Self::UnexpectedMessage {
                role,
                phase,
                direction,
                message_type,
            } => write!(
                formatter,
                "unexpected {direction:?} {message_type:?} for {role:?} during {phase:?}"
            ),
        }
    }
}

impl Error for SessionError {}

pub fn negotiate_version(remote: VersionRange) -> Result<u16, SessionError> {
    if remote.min == 0 || remote.min > remote.max {
        return Err(SessionError::InvalidVersionRange {
            min: remote.min,
            max: remote.max,
        });
    }

    let min = remote.min.max(SUPPORTED_VERSIONS.min);
    let max = remote.max.min(SUPPORTED_VERSIONS.max);
    if min > max {
        return Err(SessionError::UnsupportedVersionRange {
            min: remote.min,
            max: remote.max,
        });
    }
    Ok(max)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    DllSendHello,
    DllReceiveHelloAck {
        offered: VersionRange,
        dll_instance_id: [u8; 16],
    },
    ControllerReceiveHello,
    ControllerSendHelloAck {
        acknowledgement: HelloAck,
    },
    Ready {
        selected_version: u16,
        dll_instance_id: [u8; 16],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handshake {
    role: EndpointRole,
    state: State,
}

impl Handshake {
    #[must_use]
    pub const fn new(role: EndpointRole) -> Self {
        let state = match role {
            EndpointRole::Dll => State::DllSendHello,
            EndpointRole::Controller => State::ControllerReceiveHello,
        };
        Self { role, state }
    }

    #[must_use]
    pub const fn phase(self) -> HandshakePhase {
        match self.state {
            State::DllSendHello => HandshakePhase::SendHello,
            State::DllReceiveHelloAck { .. } => HandshakePhase::ReceiveHelloAck,
            State::ControllerReceiveHello => HandshakePhase::ReceiveHello,
            State::ControllerSendHelloAck { .. } => HandshakePhase::SendHelloAck,
            State::Ready { .. } => HandshakePhase::Ready,
        }
    }

    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self.state, State::Ready { .. })
    }

    #[must_use]
    pub const fn selected_version(self) -> Option<u16> {
        match self.state {
            State::Ready {
                selected_version, ..
            } => Some(selected_version),
            _ => None,
        }
    }

    #[must_use]
    pub const fn dll_instance_id(self) -> Option<[u8; 16]> {
        match self.state {
            State::Ready {
                dll_instance_id, ..
            } => Some(dll_instance_id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn pending_acknowledgement(self) -> Option<HelloAck> {
        match self.state {
            State::ControllerSendHelloAck { acknowledgement } => Some(acknowledgement),
            _ => None,
        }
    }

    pub fn observe(
        &mut self,
        direction: MessageDirection,
        message: &Message,
    ) -> Result<(), SessionError> {
        let next = match (self.state, direction, message) {
            (State::DllSendHello, MessageDirection::Outbound, Message::Hello(hello)) => {
                negotiate_version(hello.protocol_versions)?;
                State::DllReceiveHelloAck {
                    offered: hello.protocol_versions,
                    dll_instance_id: hello.dll_instance_id,
                }
            }
            (
                State::DllReceiveHelloAck {
                    offered,
                    dll_instance_id,
                },
                MessageDirection::Inbound,
                Message::HelloAck(acknowledgement),
            ) => {
                if acknowledgement.dll_instance_id != dll_instance_id {
                    return Err(SessionError::InstanceMismatch);
                }
                if !offered.contains(acknowledgement.selected_version)
                    || !SUPPORTED_VERSIONS.contains(acknowledgement.selected_version)
                {
                    return Err(SessionError::InvalidSelectedVersion {
                        selected: acknowledgement.selected_version,
                    });
                }
                State::Ready {
                    selected_version: acknowledgement.selected_version,
                    dll_instance_id,
                }
            }
            (State::ControllerReceiveHello, MessageDirection::Inbound, Message::Hello(hello)) => {
                State::ControllerSendHelloAck {
                    acknowledgement: HelloAck {
                        selected_version: negotiate_version(hello.protocol_versions)?,
                        dll_instance_id: hello.dll_instance_id,
                    },
                }
            }
            (
                State::ControllerSendHelloAck { acknowledgement },
                MessageDirection::Outbound,
                Message::HelloAck(actual),
            ) if *actual == acknowledgement => State::Ready {
                selected_version: acknowledgement.selected_version,
                dll_instance_id: acknowledgement.dll_instance_id,
            },
            (State::Ready { .. }, _, message) if !message.is_handshake() => self.state,
            _ => {
                return Err(SessionError::UnexpectedMessage {
                    role: self.role,
                    phase: self.phase(),
                    direction,
                    message_type: message.message_type(),
                });
            }
        };

        self.state = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SequenceCounter {
    next: u16,
}

impl SequenceCounter {
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    #[must_use]
    pub const fn from_next(next: u16) -> Self {
        Self { next }
    }

    #[must_use]
    pub const fn expected(self) -> u16 {
        self.next
    }

    pub fn take(&mut self) -> u16 {
        let current = self.next;
        self.next = self.next.wrapping_add(1);
        current
    }

    pub fn observe(&mut self, actual: u16) -> Result<(), SequenceError> {
        if actual != self.next {
            return Err(SequenceError {
                expected: self.next,
                actual,
            });
        }
        self.next = self.next.wrapping_add(1);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceError {
    pub expected: u16,
    pub actual: u16,
}

impl fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unexpected frame sequence {}; expected {}",
            self.actual, self.expected
        )
    }
}

impl Error for SequenceError {}
