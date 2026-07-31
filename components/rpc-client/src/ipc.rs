use crate::{
    IpcOperation,
    error::{ClientError, ErrorKind, Result},
    output::CommandResult,
};
use darpc_protocol::{
    EchoRequest, EndpointRole, Frame, Handshake, Hello, Message, MessageDirection, Ping,
    SequenceCounter, elapsed_tick_ms,
};
use darpc_win32::pipe::{PipeClient, sender_tick_ms};

const REQUEST_ID: u32 = 1;

pub(crate) fn execute(pid: u32, operation: IpcOperation) -> Result<CommandResult> {
    let mut session = Session::connect(pid)?;
    match operation {
        IpcOperation::Hello => Ok(CommandResult::Hello {
            requested_pid: pid,
            hello: session.hello,
            selected_version: session.selected_version,
            sequence: session.hello_sequence,
            sender_tick_ms: session.hello_tick_ms,
        }),
        IpcOperation::Ping => {
            let (request_sequence, request_tick_ms) = session.send(Message::Ping(Ping {
                request_id: REQUEST_ID,
            }))?;
            let response = session.receive()?;
            let received_tick_ms = sender_tick_ms();
            let response_sequence = response.sequence;
            let response_tick_ms = response.sender_tick_ms;
            match response.message {
                Message::Pong(message) if message.request_id == REQUEST_ID => {
                    Ok(CommandResult::Ping {
                        pid,
                        request_id: REQUEST_ID,
                        request_sequence,
                        response_sequence,
                        request_tick_ms,
                        response_tick_ms,
                        round_trip_ms: elapsed_tick_ms(request_tick_ms, received_tick_ms),
                    })
                }
                Message::Pong(message) => Err(protocol_error(
                    pid,
                    format!(
                        "Pong request ID {} does not match {REQUEST_ID}",
                        message.request_id
                    ),
                )),
                message => Err(protocol_error(
                    pid,
                    format!("expected Pong, received {:?}", message.message_type()),
                )),
            }
        }
        IpcOperation::Echo(text) => {
            let request_text = text.clone();
            let (_, request_tick_ms) = session.send(Message::EchoRequest(EchoRequest {
                request_id: REQUEST_ID,
                text,
            }))?;
            let response = session.receive()?;
            let received_tick_ms = sender_tick_ms();
            match response.message {
                Message::EchoResponse(message)
                    if message.request_id == REQUEST_ID && message.text == request_text =>
                {
                    Ok(CommandResult::Echo {
                        pid,
                        request_id: REQUEST_ID,
                        text: message.text,
                        round_trip_ms: elapsed_tick_ms(request_tick_ms, received_tick_ms),
                    })
                }
                Message::EchoResponse(message) => Err(protocol_error(
                    pid,
                    format!(
                        concat!(
                            "EchoResponse request ID {} and {} bytes did not match ",
                            "request ID {} and {} bytes"
                        ),
                        message.request_id,
                        message.text.len(),
                        REQUEST_ID,
                        request_text.len(),
                    ),
                )),
                message => Err(protocol_error(
                    pid,
                    format!(
                        "expected EchoResponse, received {:?}",
                        message.message_type()
                    ),
                )),
            }
        }
    }
}

struct Session {
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

impl Session {
    fn connect(pid: u32) -> Result<Self> {
        let pipe = PipeClient::connect(pid)
            .map_err(|error| ClientError::from_io(pid, "failed to connect to daRPC pipe", error))?;
        let mut handshake = Handshake::new(EndpointRole::Controller);
        let mut incoming_sequence = SequenceCounter::new();
        let mut outgoing_sequence = SequenceCounter::new();

        let hello_frame = pipe
            .receive_frame()
            .map_err(|error| ClientError::from_io(pid, "failed to receive Hello", error))?;
        incoming_sequence
            .observe(hello_frame.sequence)
            .map_err(|error| protocol_error(pid, error.to_string()))?;
        handshake
            .observe(MessageDirection::Inbound, &hello_frame.message)
            .map_err(|error| incompatible(pid, error.to_string()))?;
        let hello = match &hello_frame.message {
            Message::Hello(hello) => *hello,
            message => {
                return Err(protocol_error(
                    pid,
                    format!("expected Hello, received {:?}", message.message_type()),
                ));
            }
        };
        if hello.process_id != pid {
            return Err(incompatible(
                pid,
                format!(
                    "Hello process ID {} does not match requested PID {pid}",
                    hello.process_id
                ),
            ));
        }

        let acknowledgement = handshake
            .pending_acknowledgement()
            .ok_or_else(|| incompatible(pid, "Hello did not produce an acknowledgement"))?;
        let acknowledgement = Message::HelloAck(acknowledgement);
        handshake
            .observe(MessageDirection::Outbound, &acknowledgement)
            .map_err(|error| incompatible(pid, error.to_string()))?;
        let frame = Frame::new(outgoing_sequence.take(), sender_tick_ms(), acknowledgement);
        pipe.send_frame(&frame)
            .map_err(|error| ClientError::from_io(pid, "failed to send HelloAck", error))?;
        let selected_version = handshake
            .selected_version()
            .ok_or_else(|| incompatible(pid, "Hello handshake did not become ready"))?;

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

    fn send(&mut self, message: Message) -> Result<(u16, u32)> {
        self.handshake
            .observe(MessageDirection::Outbound, &message)
            .map_err(|error| protocol_error(self.pid, error.to_string()))?;
        let sequence = self.outgoing_sequence.take();
        let tick = sender_tick_ms();
        self.pipe
            .send_frame(&Frame::new(sequence, tick, message))
            .map_err(|error| ClientError::from_io(self.pid, "failed to send request", error))?;
        Ok((sequence, tick))
    }

    fn receive(&mut self) -> Result<Frame> {
        let frame = self
            .pipe
            .receive_frame()
            .map_err(|error| ClientError::from_io(self.pid, "failed to receive response", error))?;
        self.incoming_sequence
            .observe(frame.sequence)
            .map_err(|error| protocol_error(self.pid, error.to_string()))?;
        self.handshake
            .observe(MessageDirection::Inbound, &frame.message)
            .map_err(|error| protocol_error(self.pid, error.to_string()))?;
        Ok(frame)
    }
}

fn incompatible(pid: u32, message: impl Into<String>) -> ClientError {
    ClientError::new(ErrorKind::Incompatible, message).with_pid(pid)
}

fn protocol_error(pid: u32, message: impl Into<String>) -> ClientError {
    ClientError::new(ErrorKind::Protocol, message).with_pid(pid)
}
