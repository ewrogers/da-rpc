use crate::{
    commands::{CommandCall, CommandReply, WORKER_CAPACITY},
    event::DaemonEvent,
    registry::{ClientIdentity, ConnectionEvent},
};
#[cfg(debug_assertions)]
use darpc_game_client::DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE;
use darpc_game_client::{CLIENT_VERSION_CODE, EXECUTABLE_SHA256};
use darpc_protocol::{
    Architecture, CommandRequest, EventPollRequest, EventPollResult, Hello, MAX_EVENTS_PER_POLL,
    Message, Ping, Pong, SnapshotRequest, SnapshotResult, SnapshotUnavailableReason,
};
use darpc_win32::controller::{ControllerError, ControllerSession};
#[cfg(debug_assertions)]
use std::env;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};

const RETRY_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_INTERVAL: Duration = Duration::from_secs(1);
const INITIALIZATION_GRACE: Duration = Duration::from_secs(1);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const EVENT_POLL_WAIT_MS: u16 = 50;
const SNAPSHOT_RETRY_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct Worker {
    stop: Arc<AtomicBool>,
    commands: SyncSender<CommandCall>,
    _handle: JoinHandle<()>,
}

impl Worker {
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub(crate) fn route_command(&self, call: CommandCall) {
        match self.commands.try_send(call) {
            Ok(()) => {}
            Err(TrySendError::Full(call)) => {
                let _ = call.reply.send(CommandReply::Busy);
            }
            Err(TrySendError::Disconnected(call)) => {
                let _ = call.reply.send(CommandReply::Unavailable);
            }
        }
    }
}

pub(crate) fn spawn(pid: u32, events: Sender<DaemonEvent>) -> io::Result<Worker> {
    let stop = Arc::new(AtomicBool::new(false));
    let (commands, command_receiver) = mpsc::sync_channel(WORKER_CAPACITY);
    let worker_stop = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name(format!("darpcd-client-{pid}"))
        .spawn(move || run(pid, events, &command_receiver, &worker_stop))?;
    Ok(Worker {
        stop,
        commands,
        _handle: handle,
    })
}

fn run(pid: u32, events: Sender<DaemonEvent>, commands: &Receiver<CommandCall>, stop: &AtomicBool) {
    if !emit(&events, ConnectionEvent::Connecting { pid }) {
        return;
    }
    let discovered_at = Instant::now();

    while !stop.load(Ordering::Acquire) {
        reject_pending_commands(commands);
        match ControllerSession::connect(pid) {
            Ok(mut session) => {
                let hello = session.hello();
                let identity = ClientIdentity::from_hello(hello);
                if let Err(reason) = validate_identity(hello) {
                    if !emit(
                        &events,
                        ConnectionEvent::Incompatible {
                            pid,
                            identity: Some(identity),
                            reason,
                        },
                    ) {
                        return;
                    }
                    if wait_for_stop(stop, RETRY_INTERVAL) {
                        return;
                    }
                    continue;
                }
                if !emit(
                    &events,
                    ConnectionEvent::Connected {
                        pid,
                        hello,
                        selected_version: session.selected_version(),
                    },
                ) {
                    return;
                }

                if let Err(error) = monitor(&mut session, commands, stop, &events, pid, identity)
                    && !stop.load(Ordering::Acquire)
                    && !emit(
                        &events,
                        ConnectionEvent::Disconnected {
                            pid,
                            identity: Some(identity),
                            reason: error.to_string(),
                        },
                    )
                {
                    return;
                }
            }
            Err(error) => {
                let event = connect_failure(pid, error);
                let within_grace = matches!(event, ConnectionEvent::NotLoaded { .. })
                    && discovered_at.elapsed() < INITIALIZATION_GRACE;
                if !within_grace && !emit(&events, event) {
                    return;
                }
            }
        }

        if wait_for_stop(stop, RETRY_INTERVAL) {
            reject_pending_commands(commands);
            return;
        }
    }
    reject_pending_commands(commands);
}

fn validate_identity(hello: Hello) -> Result<(), String> {
    if hello.architecture != Architecture::X86 {
        return Err(format!(
            "unsupported client architecture {:?}; expected x86",
            hello.architecture
        ));
    }

    #[cfg(debug_assertions)]
    if env::var_os(DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE).as_deref()
        == Some(std::ffi::OsStr::new("1"))
        && hello.executable_fingerprint == [0; 32]
        && hello.client_version == 0
    {
        return Ok(());
    }

    if hello.executable_fingerprint != EXECUTABLE_SHA256 {
        return Err("unsupported client executable fingerprint".into());
    }
    if hello.client_version != CLIENT_VERSION_CODE {
        return Err(format!(
            "unsupported client version {}; expected {CLIENT_VERSION_CODE}",
            hello.client_version
        ));
    }
    Ok(())
}

fn monitor(
    session: &mut ControllerSession,
    commands: &Receiver<CommandCall>,
    stop: &AtomicBool,
    events: &Sender<DaemonEvent>,
    pid: u32,
    identity: ClientIdentity,
) -> Result<(), ControllerError> {
    let mut request_id = 1_u32;
    let mut boundary = None;
    let mut last_health = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if let Some(call) = next_command(commands) {
            if call.identity != identity {
                let _ = call.reply.send(CommandReply::Unavailable);
            } else {
                let result = request_command(session, request_id, call.operation);
                request_id = next_nonzero(request_id);
                match result {
                    Ok(result) => {
                        let _ = call.reply.send(CommandReply::Result(result));
                    }
                    Err(error) => {
                        let _ = call.reply.send(CommandReply::Unavailable);
                        reject_pending_commands(commands);
                        return Err(error);
                    }
                }
            }
        }

        if boundary.is_none() {
            match request_snapshot(session, request_id)? {
                SnapshotOutcome::Ready(snapshot) => {
                    boundary = Some((snapshot.event_sequence, snapshot.revision));
                    send_event(
                        events,
                        ConnectionEvent::Snapshot {
                            pid,
                            identity,
                            snapshot,
                        },
                    )?;
                }
                SnapshotOutcome::Unavailable(reason) => {
                    send_event(
                        events,
                        ConnectionEvent::SnapshotUnavailable {
                            pid,
                            identity,
                            reason: format!("snapshot unavailable: {reason:?}"),
                        },
                    )?;
                    if wait_for_stop(stop, SNAPSHOT_RETRY_INTERVAL) {
                        return Ok(());
                    }
                }
            }
            request_id = request_id.wrapping_add(1);
            continue;
        }

        let (after_sequence, after_revision) = boundary.expect("snapshot boundary is present");
        match poll_events(session, request_id, after_sequence)? {
            EventPollResult::Events(state_events) => {
                if !state_events.is_empty() {
                    if let Some(next_boundary) =
                        validate_event_batch(after_sequence, after_revision, &state_events)
                    {
                        boundary = Some(next_boundary);
                        send_event(
                            events,
                            ConnectionEvent::StateEvents {
                                pid,
                                identity,
                                events: state_events,
                            },
                        )?;
                    } else {
                        boundary = None;
                    }
                }
            }
            EventPollResult::ResyncRequired { .. } => boundary = None,
        }
        request_id = request_id.wrapping_add(1);

        if last_health.elapsed() >= HEALTH_INTERVAL {
            ping(session, request_id)?;
            request_id = request_id.wrapping_add(1);
            last_health = Instant::now();
        }
    }
    reject_pending_commands(commands);
    Ok(())
}

fn request_command(
    session: &mut ControllerSession,
    request_id: u32,
    operation: darpc_protocol::CommandOperation,
) -> Result<darpc_protocol::CommandResult, ControllerError> {
    session.send(Message::CommandRequest(CommandRequest {
        request_id,
        operation,
    }))?;
    let response = session.receive()?;
    match response.message {
        Message::CommandResponse(response) if response.request_id == request_id => {
            Ok(response.result)
        }
        Message::CommandResponse(response) => Err(ControllerError::Protocol(format!(
            "CommandResponse request ID {} does not match {request_id}",
            response.request_id
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected CommandResponse, received {:?}",
            message.message_type()
        ))),
    }
}

fn next_command(commands: &Receiver<CommandCall>) -> Option<CommandCall> {
    commands.try_recv().ok()
}

fn reject_pending_commands(commands: &Receiver<CommandCall>) {
    while let Ok(call) = commands.try_recv() {
        let _ = call.reply.send(CommandReply::Unavailable);
    }
}

fn request_snapshot(
    session: &mut ControllerSession,
    request_id: u32,
) -> Result<SnapshotOutcome, ControllerError> {
    session.send(Message::SnapshotRequest(SnapshotRequest { request_id }))?;
    let response = session.receive()?;
    match response.message {
        Message::SnapshotResponse(response) if response.request_id == request_id => {
            match response.result {
                SnapshotResult::Ready(snapshot) => Ok(SnapshotOutcome::Ready(snapshot)),
                SnapshotResult::Unavailable(reason) => Ok(SnapshotOutcome::Unavailable(reason)),
            }
        }
        Message::SnapshotResponse(response) => Err(ControllerError::Protocol(format!(
            "SnapshotResponse request ID {} does not match {request_id}",
            response.request_id,
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected SnapshotResponse, received {:?}",
            message.message_type()
        ))),
    }
}

fn poll_events(
    session: &mut ControllerSession,
    request_id: u32,
    after_sequence: u32,
) -> Result<EventPollResult, ControllerError> {
    session.send(Message::EventPollRequest(EventPollRequest {
        request_id,
        after_sequence,
        max_events: MAX_EVENTS_PER_POLL,
        wait_ms: EVENT_POLL_WAIT_MS,
    }))?;
    let response = session.receive()?;
    match response.message {
        Message::EventPollResponse(response) if response.request_id == request_id => {
            Ok(response.result)
        }
        Message::EventPollResponse(response) => Err(ControllerError::Protocol(format!(
            "EventPollResponse request ID {} does not match {request_id}",
            response.request_id
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected EventPollResponse, received {:?}",
            message.message_type()
        ))),
    }
}

fn ping(session: &mut ControllerSession, request_id: u32) -> Result<(), ControllerError> {
    session.send(Message::Ping(Ping { request_id }))?;
    let response = session.receive()?;
    match response.message {
        Message::Pong(Pong {
            request_id: response_id,
        }) if response_id == request_id => Ok(()),
        Message::Pong(Pong {
            request_id: response_id,
        }) => Err(ControllerError::Protocol(format!(
            "Pong request ID {response_id} does not match {request_id}"
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected Pong, received {:?}",
            message.message_type()
        ))),
    }
}

fn send_event(events: &Sender<DaemonEvent>, event: ConnectionEvent) -> Result<(), ControllerError> {
    if emit(events, event) {
        Ok(())
    } else {
        Err(ControllerError::Protocol(
            "daemon event channel closed".into(),
        ))
    }
}

const fn next_nonzero(value: u32) -> u32 {
    let next = value.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

fn validate_event_batch(
    after_sequence: u32,
    after_revision: u32,
    events: &[darpc_model::StateEvent],
) -> Option<(u32, u32)> {
    let mut boundary = (after_sequence, after_revision);
    for event in events {
        if event.sequence != next_nonzero(boundary.0) || event.revision != next_nonzero(boundary.1)
        {
            return None;
        }
        boundary = (event.sequence, event.revision);
    }
    Some(boundary)
}

enum SnapshotOutcome {
    Ready(Box<darpc_model::ClientSnapshot>),
    Unavailable(SnapshotUnavailableReason),
}

fn wait_for_stop(stop: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stop.load(Ordering::Acquire) {
            return true;
        }
        thread::sleep(STOP_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
    stop.load(Ordering::Acquire)
}

fn connect_failure(pid: u32, error: ControllerError) -> ConnectionEvent {
    match error {
        ControllerError::Io { source, .. }
            if source.raw_os_error().map(|value| value as u32) == Some(ERROR_FILE_NOT_FOUND) =>
        {
            ConnectionEvent::NotLoaded { pid }
        }
        ControllerError::Io { source, .. }
            if source.raw_os_error().map(|value| value as u32) == Some(ERROR_PIPE_BUSY) =>
        {
            ConnectionEvent::Busy { pid }
        }
        ControllerError::Incompatible { hello, message } => ConnectionEvent::Incompatible {
            pid,
            identity: hello.map(ClientIdentity::from_hello),
            reason: message,
        },
        ControllerError::Protocol(reason) => ConnectionEvent::Incompatible {
            pid,
            identity: None,
            reason,
        },
        error => ConnectionEvent::Disconnected {
            pid,
            identity: None,
            reason: error.to_string(),
        },
    }
}

fn emit(events: &Sender<DaemonEvent>, event: ConnectionEvent) -> bool {
    events.send(DaemonEvent::Connection(event)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{validate_event_batch, validate_identity};
    use darpc_game_client::{CLIENT_VERSION_CODE, EXECUTABLE_SHA256};
    use darpc_model::{StateEvent, StateUpdate, StatusUpdate};
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};

    fn hello() -> Hello {
        Hello {
            protocol_versions: SUPPORTED_VERSIONS,
            dll_instance_id: [1; 16],
            process_id: 42,
            process_creation_time: 100,
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            executable_fingerprint: EXECUTABLE_SHA256,
            client_version: CLIENT_VERSION_CODE,
        }
    }

    #[test]
    fn accepts_only_the_supported_client_identity() {
        assert!(validate_identity(hello()).is_ok());

        let mut wrong_architecture = hello();
        wrong_architecture.architecture = Architecture::X86_64;
        assert!(validate_identity(wrong_architecture).is_err());

        let mut wrong_fingerprint = hello();
        wrong_fingerprint.executable_fingerprint[0] ^= 0xFF;
        assert!(validate_identity(wrong_fingerprint).is_err());

        let mut wrong_version = hello();
        wrong_version.client_version = CLIENT_VERSION_CODE + 1;
        assert!(validate_identity(wrong_version).is_err());
    }

    #[test]
    fn accepts_only_contiguous_event_batches() {
        let event = |sequence, revision| StateEvent {
            sequence,
            revision,
            tick_ms: 500,
            update: StateUpdate::Status(StatusUpdate::default()),
        };

        assert_eq!(
            validate_event_batch(9, 19, &[event(10, 20), event(11, 21)]),
            Some((11, 21))
        );
        assert_eq!(validate_event_batch(9, 19, &[event(11, 20)]), None);
        assert_eq!(validate_event_batch(9, 19, &[event(10, 21)]), None);
        assert_eq!(
            validate_event_batch(u32::MAX, u32::MAX, &[event(1, 1)]),
            Some((1, 1))
        );
    }
}
