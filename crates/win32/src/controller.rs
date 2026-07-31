use crate::pipe::{PipeClient, sender_tick_ms};
use darpc_protocol::{
    EndpointRole, Frame, Handshake, Hello, Message, MessageDirection, SequenceCounter,
};
use std::{error::Error, fmt, io};

pub struct ControllerSession {
    pid: u32,
    pipe: PipeClient,
    handshake: Handshake,
    incoming_sequence: SequenceCounter,
    outgoing_sequence: SequenceCounter,
    hello: Hello,
    selected_version: u16,
    hello_sequence: u16,
    hello_tick_ms: u32,
}

impl ControllerSession {
    pub fn connect(pid: u32) -> Result<Self, ControllerError> {
        let pipe = PipeClient::connect(pid)
            .map_err(|error| ControllerError::io("failed to connect to daRPC pipe", error))?;
        let mut handshake = Handshake::new(EndpointRole::Controller);
        let mut incoming_sequence = SequenceCounter::new();
        let mut outgoing_sequence = SequenceCounter::new();

        let hello_frame = pipe
            .receive_frame()
            .map_err(|error| ControllerError::io("failed to receive Hello", error))?;
        incoming_sequence
            .observe(hello_frame.sequence)
            .map_err(|error| ControllerError::Protocol(error.to_string()))?;
        let hello = match &hello_frame.message {
            Message::Hello(hello) => *hello,
            message => {
                return Err(ControllerError::Protocol(format!(
                    "expected Hello, received {:?}",
                    message.message_type()
                )));
            }
        };
        if hello.process_id != pid {
            return Err(ControllerError::Incompatible {
                hello: Some(hello),
                message: format!(
                    "Hello process ID {} does not match requested PID {pid}",
                    hello.process_id
                ),
            });
        }
        handshake
            .observe(MessageDirection::Inbound, &hello_frame.message)
            .map_err(|error| ControllerError::Incompatible {
                hello: Some(hello),
                message: error.to_string(),
            })?;

        let acknowledgement =
            handshake
                .pending_acknowledgement()
                .ok_or_else(|| ControllerError::Incompatible {
                    hello: Some(hello),
                    message: "Hello did not produce an acknowledgement".into(),
                })?;
        let acknowledgement = Message::HelloAck(acknowledgement);
        handshake
            .observe(MessageDirection::Outbound, &acknowledgement)
            .map_err(|error| ControllerError::Incompatible {
                hello: Some(hello),
                message: error.to_string(),
            })?;
        let frame = Frame::new(outgoing_sequence.take(), sender_tick_ms(), acknowledgement);
        pipe.send_frame(&frame)
            .map_err(|error| ControllerError::io("failed to send HelloAck", error))?;
        let selected_version =
            handshake
                .selected_version()
                .ok_or_else(|| ControllerError::Incompatible {
                    hello: Some(hello),
                    message: "Hello handshake did not become ready".into(),
                })?;

        Ok(Self {
            pid,
            pipe,
            handshake,
            incoming_sequence,
            outgoing_sequence,
            hello,
            selected_version,
            hello_sequence: hello_frame.sequence,
            hello_tick_ms: hello_frame.sender_tick_ms,
        })
    }

    #[must_use]
    pub const fn hello(&self) -> Hello {
        self.hello
    }

    #[must_use]
    pub const fn selected_version(&self) -> u16 {
        self.selected_version
    }

    #[must_use]
    pub const fn hello_sequence(&self) -> u16 {
        self.hello_sequence
    }

    #[must_use]
    pub const fn hello_tick_ms(&self) -> u32 {
        self.hello_tick_ms
    }

    pub fn send(&mut self, message: Message) -> Result<SentFrame, ControllerError> {
        self.handshake
            .observe(MessageDirection::Outbound, &message)
            .map_err(|error| ControllerError::Protocol(error.to_string()))?;
        let sequence = self.outgoing_sequence.take();
        let sender_tick_ms = sender_tick_ms();
        self.pipe
            .send_frame(&Frame::new(sequence, sender_tick_ms, message))
            .map_err(|error| ControllerError::io("failed to send frame", error))?;
        Ok(SentFrame {
            sequence,
            sender_tick_ms,
        })
    }

    pub fn receive(&mut self) -> Result<Frame, ControllerError> {
        let frame = self
            .pipe
            .receive_frame()
            .map_err(|error| ControllerError::io("failed to receive frame", error))?;
        self.incoming_sequence
            .observe(frame.sequence)
            .map_err(|error| ControllerError::Protocol(error.to_string()))?;
        self.handshake
            .observe(MessageDirection::Inbound, &frame.message)
            .map_err(|error| ControllerError::Protocol(error.to_string()))?;
        Ok(frame)
    }

    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SentFrame {
    pub sequence: u16,
    pub sender_tick_ms: u32,
}

#[derive(Debug)]
pub enum ControllerError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Incompatible {
        hello: Option<Hello>,
        message: String,
    },
    Protocol(String),
}

impl ControllerError {
    fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }

    #[must_use]
    pub const fn incompatible_hello(&self) -> Option<Hello> {
        match self {
            Self::Incompatible { hello, .. } => *hello,
            _ => None,
        }
    }
}

impl fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Incompatible { message, .. } | Self::Protocol(message) => message.fmt(formatter),
        }
    }
}

impl Error for ControllerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Incompatible { .. } | Self::Protocol(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControllerError, ControllerSession};
    use crate::pipe::{PipeServer, StopEvent, sender_tick_ms};
    use darpc_protocol::{
        Architecture, ComponentVersion, Frame, Hello, Message, VersionRange, protocol_version,
    };
    use std::{process, thread};

    #[test]
    fn preserves_identity_from_an_incompatible_hello() {
        let pid = process::id();
        let stop = StopEvent::new().unwrap();
        let server = PipeServer::bind(pid, stop).unwrap();
        let hello = Hello {
            protocol_versions: VersionRange {
                min: protocol_version(2, 0),
                max: protocol_version(2, 0),
            },
            dll_instance_id: [0xAB; 16],
            process_id: pid,
            process_creation_time: 42,
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 1,
                minor: 2,
                patch: 3,
            },
            executable_fingerprint: [0xCD; 32],
            layout_id: 741,
        };
        let worker = thread::spawn(move || {
            server.accept().unwrap();
            server
                .send_frame(&Frame::new(0, sender_tick_ms(), Message::Hello(hello)))
                .unwrap();
            let _ = server.receive_frame();
            server.disconnect().unwrap();
        });

        let error = match ControllerSession::connect(pid) {
            Ok(_) => panic!("incompatible Hello unexpectedly connected"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ControllerError::Incompatible { .. }),
            "unexpected error: {error:?}"
        );
        assert_eq!(error.incompatible_hello(), Some(hello));
        worker.join().unwrap();
    }
}
