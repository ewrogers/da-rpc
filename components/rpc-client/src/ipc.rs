use crate::{
    Operation,
    error::{ClientError, ErrorKind, Result},
    output::CommandResult,
};
use darpc_protocol::{
    CommandKind, CommandOperation, CommandRequest, DEFAULT_COMMAND_TIMEOUT_MS, EchoRequest,
    MAX_COMMAND_WAIT_MS, Message, Ping, SnapshotRequest, SnapshotResult, SnapshotUnavailableReason,
    TickHealthRequest, TickHealthResponse, elapsed_tick_ms,
};
use darpc_win32::{
    controller::{ControllerError, ControllerSession},
    pipe::sender_tick_ms,
};
use std::{
    thread,
    time::{Duration, Instant},
};

const REQUEST_ID: u32 = 1;
const SECOND_REQUEST_ID: u32 = 2;
const TICK_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) fn execute(pid: u32, operation: Operation) -> Result<CommandResult> {
    let mut session =
        ControllerSession::connect(pid).map_err(|error| controller_error(pid, error))?;
    match operation {
        Operation::Hello => Ok(CommandResult::Hello {
            requested_pid: pid,
            hello: session.hello(),
            selected_version: session.selected_version(),
            sequence: session.hello_sequence(),
            sender_tick_ms: session.hello_tick_ms(),
        }),
        Operation::Ping => {
            let request = session
                .send(Message::Ping(Ping {
                    request_id: REQUEST_ID,
                }))
                .map_err(|error| controller_error(pid, error))?;
            let response = session
                .receive()
                .map_err(|error| controller_error(pid, error))?;
            let received_tick_ms = sender_tick_ms();
            let response_sequence = response.sequence;
            let response_tick_ms = response.sender_tick_ms;
            match response.message {
                Message::Pong(message) if message.request_id == REQUEST_ID => {
                    Ok(CommandResult::Ping {
                        pid,
                        request_id: REQUEST_ID,
                        request_sequence: request.sequence,
                        response_sequence,
                        request_tick_ms: request.sender_tick_ms,
                        response_tick_ms,
                        round_trip_ms: elapsed_tick_ms(request.sender_tick_ms, received_tick_ms),
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
        Operation::TickHealth => {
            let (first, first_tick_ms) = request_tick_health(&mut session, pid, REQUEST_ID)?;
            thread::sleep(TICK_SAMPLE_INTERVAL);
            let (second, second_tick_ms) =
                request_tick_health(&mut session, pid, SECOND_REQUEST_ID)?;

            if first.installed != second.installed
                || first.relocated_bytes != second.relocated_bytes
            {
                return Err(protocol_error(
                    pid,
                    "tick hook changed during health sampling",
                ));
            }

            Ok(CommandResult::TickHealth {
                pid,
                installed: second.installed,
                relocated_bytes: second.relocated_bytes,
                first_tick_count: first.tick_count,
                tick_count: second.tick_count,
                tick_delta: second.tick_count.wrapping_sub(first.tick_count),
                sample_ms: elapsed_tick_ms(first_tick_ms, second_tick_ms),
            })
        }
        Operation::Snapshot => {
            let request = session
                .send(Message::SnapshotRequest(SnapshotRequest {
                    request_id: REQUEST_ID,
                }))
                .map_err(|error| controller_error(pid, error))?;
            let response = session
                .receive()
                .map_err(|error| controller_error(pid, error))?;
            let received_tick_ms = sender_tick_ms();
            match response.message {
                Message::SnapshotResponse(message) if message.request_id == REQUEST_ID => {
                    match message.result {
                        SnapshotResult::Ready(snapshot) => Ok(CommandResult::Snapshot {
                            pid,
                            request_id: REQUEST_ID,
                            snapshot,
                            round_trip_ms: elapsed_tick_ms(
                                request.sender_tick_ms,
                                received_tick_ms,
                            ),
                        }),
                        SnapshotResult::Unavailable(reason) => {
                            Err(snapshot_unavailable(pid, reason))
                        }
                    }
                }
                Message::SnapshotResponse(message) => Err(protocol_error(
                    pid,
                    format!(
                        "SnapshotResponse request ID {} does not match {REQUEST_ID}",
                        message.request_id
                    ),
                )),
                message => Err(protocol_error(
                    pid,
                    format!(
                        "expected SnapshotResponse, received {:?}",
                        message.message_type()
                    ),
                )),
            }
        }
        Operation::Echo(text) => {
            let request_text = text.clone();
            let request = session
                .send(Message::EchoRequest(EchoRequest {
                    request_id: REQUEST_ID,
                    text,
                }))
                .map_err(|error| controller_error(pid, error))?;
            let response = session
                .receive()
                .map_err(|error| controller_error(pid, error))?;
            let received_tick_ms = sender_tick_ms();
            match response.message {
                Message::EchoResponse(message)
                    if message.request_id == REQUEST_ID && message.text == request_text =>
                {
                    Ok(CommandResult::Echo {
                        pid,
                        request_id: REQUEST_ID,
                        text: message.text,
                        round_trip_ms: elapsed_tick_ms(request.sender_tick_ms, received_tick_ms),
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
        Operation::Diagnostic => request_command(
            &mut session,
            pid,
            "diagnostic",
            CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        ),
        Operation::Turn(direction) => request_command(
            &mut session,
            pid,
            "turn",
            CommandOperation::Submit {
                kind: CommandKind::Turn(direction),
                timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        ),
        Operation::Walk(target) => request_command(
            &mut session,
            pid,
            "walk",
            CommandOperation::Submit {
                kind: CommandKind::Walk(target),
                timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        ),
        Operation::UseSkill(slot) => request_command(
            &mut session,
            pid,
            "skill-use",
            CommandOperation::Submit {
                kind: CommandKind::UseSkill(slot),
                timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        ),
        Operation::CastSpell(cast) => request_command(
            &mut session,
            pid,
            "spell-cast",
            CommandOperation::Submit {
                kind: CommandKind::CastSpell(cast),
                timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        ),
        Operation::UseItem(slot) => {
            request_action(&mut session, pid, "item-use", CommandKind::UseItem(slot))
        }
        Operation::DropItem(transfer) => request_action(
            &mut session,
            pid,
            "item-drop",
            CommandKind::DropItem(transfer),
        ),
        Operation::GiveItem(transfer) => request_action(
            &mut session,
            pid,
            "item-give",
            CommandKind::GiveItem(transfer),
        ),
        Operation::DropGold(transfer) => request_action(
            &mut session,
            pid,
            "gold-drop",
            CommandKind::DropGold(transfer),
        ),
        Operation::GiveGold(transfer) => request_action(
            &mut session,
            pid,
            "gold-give",
            CommandKind::GiveGold(transfer),
        ),
        Operation::PickupItem(position) => request_action(
            &mut session,
            pid,
            "item-pickup",
            CommandKind::PickupItem(position),
        ),
        Operation::Unequip(slot) => {
            request_action(&mut session, pid, "unequip", CommandKind::Unequip(slot))
        }
        Operation::Emote(code) => {
            request_action(&mut session, pid, "emote", CommandKind::Emote(code))
        }
        Operation::Group(command) => {
            let action = match command {
                darpc_protocol::GroupCommand::Toggle => "group-toggle",
                darpc_protocol::GroupCommand::Invite(_) => "group-invite",
                darpc_protocol::GroupCommand::Respond {
                    action: darpc_protocol::GroupInvitationAction::Accept,
                    ..
                } => "group-accept",
                darpc_protocol::GroupCommand::Respond {
                    action: darpc_protocol::GroupInvitationAction::Decline,
                    ..
                } => "group-decline",
            };
            request_action(&mut session, pid, action, CommandKind::Group(command))
        }
        Operation::Who => request_who(&mut session, pid),
        Operation::CommandStatus(command_id) => request_command(
            &mut session,
            pid,
            "command-status",
            CommandOperation::Query {
                command_id,
                wait_ms: 0,
            },
        ),
        Operation::CommandCancel(command_id) => request_command(
            &mut session,
            pid,
            "command-cancel",
            CommandOperation::Cancel { command_id },
        ),
    }
}

fn request_who(session: &mut ControllerSession, pid: u32) -> Result<CommandResult> {
    const WHO_TIMEOUT_MS: u16 = 3_000;
    let deadline = Instant::now() + Duration::from_millis(u64::from(WHO_TIMEOUT_MS));
    let mut response = request_command(
        session,
        pid,
        "who",
        CommandOperation::Submit {
            kind: CommandKind::Who,
            timeout_ms: WHO_TIMEOUT_MS,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )?;
    loop {
        let command_id = match &response {
            CommandResult::Command {
                result: darpc_protocol::CommandResult::Who { .. },
                ..
            } => return Ok(response),
            CommandResult::Command {
                result: darpc_protocol::CommandResult::Status(status),
                ..
            } if status.state == darpc_protocol::CommandState::Accepted => status.command_id,
            _ => return Ok(response),
        };
        if Instant::now() >= deadline {
            return Ok(response);
        }
        response = request_command(
            session,
            pid,
            "who",
            CommandOperation::Query {
                command_id,
                wait_ms: MAX_COMMAND_WAIT_MS,
            },
        )?;
    }
}

fn request_action(
    session: &mut ControllerSession,
    pid: u32,
    action: &'static str,
    kind: CommandKind,
) -> Result<CommandResult> {
    request_command(
        session,
        pid,
        action,
        CommandOperation::Submit {
            kind,
            timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
}

fn request_command(
    session: &mut ControllerSession,
    pid: u32,
    action: &'static str,
    operation: CommandOperation,
) -> Result<CommandResult> {
    let request = session
        .send(Message::CommandRequest(CommandRequest {
            request_id: REQUEST_ID,
            operation,
        }))
        .map_err(|error| controller_error(pid, error))?;
    let response = session
        .receive()
        .map_err(|error| controller_error(pid, error))?;
    let received_tick_ms = sender_tick_ms();
    match response.message {
        Message::CommandResponse(message) if message.request_id == REQUEST_ID => {
            Ok(CommandResult::Command {
                pid,
                action,
                request_id: REQUEST_ID,
                result: message.result,
                round_trip_ms: elapsed_tick_ms(request.sender_tick_ms, received_tick_ms),
            })
        }
        Message::CommandResponse(message) => Err(protocol_error(
            pid,
            format!(
                "CommandResponse request ID {} does not match {REQUEST_ID}",
                message.request_id
            ),
        )),
        message => Err(protocol_error(
            pid,
            format!(
                "expected CommandResponse, received {:?}",
                message.message_type()
            ),
        )),
    }
}

fn snapshot_unavailable(pid: u32, reason: SnapshotUnavailableReason) -> ClientError {
    let (kind, message) = match reason {
        SnapshotUnavailableReason::HookUnavailable => (
            ErrorKind::Incompatible,
            "the client tick hook is unavailable",
        ),
        SnapshotUnavailableReason::CaptureTimedOut => (
            ErrorKind::Timeout,
            "the client did not capture a snapshot before the deadline",
        ),
        SnapshotUnavailableReason::CaptureFailed => (
            ErrorKind::Io,
            "the client rejected the snapshot memory walk",
        ),
    };
    ClientError::new(kind, message).with_pid(pid)
}

fn request_tick_health(
    session: &mut ControllerSession,
    pid: u32,
    request_id: u32,
) -> Result<(TickHealthResponse, u32)> {
    session
        .send(Message::TickHealthRequest(TickHealthRequest { request_id }))
        .map_err(|error| controller_error(pid, error))?;
    let response = session
        .receive()
        .map_err(|error| controller_error(pid, error))?;
    match response.message {
        Message::TickHealthResponse(message) if message.request_id == request_id => {
            Ok((message, response.sender_tick_ms))
        }
        Message::TickHealthResponse(message) => Err(protocol_error(
            pid,
            format!(
                "TickHealthResponse request ID {} does not match {request_id}",
                message.request_id
            ),
        )),
        message => Err(protocol_error(
            pid,
            format!(
                "expected TickHealthResponse, received {:?}",
                message.message_type()
            ),
        )),
    }
}

fn controller_error(pid: u32, error: ControllerError) -> ClientError {
    match error {
        ControllerError::Io { operation, source } => ClientError::from_io(pid, operation, source),
        ControllerError::Incompatible { message, .. } => {
            ClientError::new(ErrorKind::Incompatible, message).with_pid(pid)
        }
        ControllerError::Protocol(message) => protocol_error(pid, message),
    }
}

fn protocol_error(pid: u32, message: impl Into<String>) -> ClientError {
    ClientError::new(ErrorKind::Protocol, message).with_pid(pid)
}
