use crate::{
    commands::{ClientOperation, CommandCall, CommandReply, WORKER_CAPACITY},
    event::DaemonEvent,
    registry::{ClientIdentity, ConnectionEvent},
};
#[cfg(debug_assertions)]
use darpc_game_client::DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE;
use darpc_game_client::{CLIENT_VERSION_CODE, EXECUTABLE_SHA256};
use darpc_model::SequenceNumber;
use darpc_protocol::{
    Architecture, CommandRequest, DiagnosticsMode, DiagnosticsOperation, DiagnosticsRequest,
    DiagnosticsResponse, EventPollRequest, EventPollResult, HOOK_TIMING_STAGE_COUNT, Hello,
    MAX_EVENTS_PER_POLL, Message, SnapshotRequest, SnapshotResult, SnapshotUnavailableReason,
    TickHealthRequest, TickHealthResponse,
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
const MIN_TICK_RATE_HZ: u32 = 60;
const DEGRADED_SAMPLE_COUNT: u8 = 3;
const INITIALIZATION_GRACE: Duration = Duration::from_secs(1);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const EVENT_POLL_WAIT_MS: u16 = 50;
const SNAPSHOT_RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct TickRateMonitor {
    previous_tick_count: Option<u32>,
    slow_samples: u8,
    degraded: bool,
}

#[derive(Default)]
struct HookTimingMonitor {
    over_budget_counts: [u64; HOOK_TIMING_STAGE_COUNT],
}

impl HookTimingMonitor {
    fn observe(
        &mut self,
        pid: u32,
        response: &DiagnosticsResponse,
        events: &Sender<DaemonEvent>,
    ) -> Result<(), ControllerError> {
        if response.mode != DiagnosticsMode::HookTiming {
            self.over_budget_counts = [0; HOOK_TIMING_STAGE_COUNT];
            return Ok(());
        }
        for (index, timing) in response.hook_timings.iter().enumerate() {
            let previous = self.over_budget_counts[index];
            let delta = if timing.over_budget_count < previous {
                timing.over_budget_count
            } else {
                timing.over_budget_count - previous
            };
            self.over_budget_counts[index] = timing.over_budget_count;
            if delta != 0 {
                events.send(DaemonEvent::Timing(format!(
                    concat!(
                        "client pid={} timing=hook_budget_exceeded stage={:?} budget_us={} ",
                        "over_budget_delta={} over_budget_total={} maximum_duration_us={} last_duration_us={}"
                    ),
                    pid, timing.stage, timing.budget_us, delta, timing.over_budget_count,
                    timing.maximum_duration_us, timing.last_duration_us,
                ))).map_err(|_| ControllerError::Protocol("daemon event channel closed".into()))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TickRateChange {
    Degraded {
        tick_delta: u32,
        sample_ms: u64,
        rate_hz: u32,
    },
    Recovered {
        tick_delta: u32,
        sample_ms: u64,
        rate_hz: u32,
    },
}

impl TickRateMonitor {
    fn observe(&mut self, tick_count: u32, sample_ms: u64) -> Option<TickRateChange> {
        let previous = self.previous_tick_count.replace(tick_count)?;
        let sample_ms = sample_ms.max(1);
        let tick_delta = tick_count.wrapping_sub(previous);
        let rate_hz = u32::try_from(u64::from(tick_delta) * 1_000 / sample_ms).unwrap_or(u32::MAX);
        let slow = rate_hz < MIN_TICK_RATE_HZ;

        if slow {
            self.slow_samples = self.slow_samples.saturating_add(1);
            if !self.degraded && self.slow_samples >= DEGRADED_SAMPLE_COUNT {
                self.degraded = true;
                return Some(TickRateChange::Degraded {
                    tick_delta,
                    sample_ms,
                    rate_hz,
                });
            }
        } else {
            self.slow_samples = 0;
            if self.degraded {
                self.degraded = false;
                return Some(TickRateChange::Recovered {
                    tick_delta,
                    sample_ms,
                    rate_hz,
                });
            }
        }
        None
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

pub(crate) struct Worker {
    stop: Arc<AtomicBool>,
    refresh_observation: Arc<AtomicBool>,
    commands: SyncSender<CommandCall>,
    _handle: JoinHandle<()>,
}

impl Worker {
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub(crate) fn request_fresh_snapshot(&self) {
        // The worker consumes this flag between bounded pipe operations and
        // clears its event boundary before issuing the snapshot request.
        self.refresh_observation.store(true, Ordering::Release);
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
    let refresh_observation = Arc::new(AtomicBool::new(false));
    let (commands, command_receiver) = mpsc::sync_channel(WORKER_CAPACITY);
    let worker_stop = Arc::clone(&stop);
    let worker_refresh_observation = Arc::clone(&refresh_observation);
    let handle = thread::Builder::new()
        .name(format!("darpcd-client-{pid}"))
        .spawn(move || {
            run(
                pid,
                events,
                &command_receiver,
                &worker_stop,
                &worker_refresh_observation,
            );
        })?;
    Ok(Worker {
        stop,
        refresh_observation,
        commands,
        _handle: handle,
    })
}

fn run(
    pid: u32,
    events: Sender<DaemonEvent>,
    commands: &Receiver<CommandCall>,
    stop: &AtomicBool,
    refresh_observation: &AtomicBool,
) {
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

                let diagnostics_supported = supports_diagnostics(hello.dll_version);
                if let Err(error) = monitor(
                    &mut session,
                    commands,
                    stop,
                    &events,
                    pid,
                    identity,
                    diagnostics_supported,
                    refresh_observation,
                ) && !stop.load(Ordering::Acquire)
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

fn supports_diagnostics(version: darpc_protocol::ComponentVersion) -> bool {
    (version.major, version.minor, version.patch) >= (1, 5, 2)
}

fn monitor(
    session: &mut ControllerSession,
    commands: &Receiver<CommandCall>,
    stop: &AtomicBool,
    events: &Sender<DaemonEvent>,
    pid: u32,
    identity: ClientIdentity,
    diagnostics_supported: bool,
    refresh_observation: &AtomicBool,
) -> Result<(), ControllerError> {
    let mut request_id = 1_u32;
    let mut boundary = None;
    let mut last_health = Instant::now();
    let mut tick_rate = TickRateMonitor::default();
    let mut hook_timing = HookTimingMonitor::default();
    while !stop.load(Ordering::Acquire) {
        if refresh_observation.swap(false, Ordering::AcqRel) {
            boundary = None;
        }
        if let Some(call) = next_command(commands) {
            if call.identity != identity {
                let _ = call.reply.send(CommandReply::Unavailable);
            } else {
                let result = match call.operation {
                    ClientOperation::Command(operation) => {
                        request_command(session, request_id, operation).map(CommandReply::Result)
                    }
                    ClientOperation::Diagnostics(operation) if diagnostics_supported => {
                        request_diagnostics(session, request_id, operation)
                            .map(CommandReply::Diagnostics)
                    }
                    ClientOperation::Diagnostics(_) => Ok(CommandReply::Unavailable),
                    ClientOperation::Snapshot => match request_snapshot(session, request_id) {
                        Ok(SnapshotOutcome::Ready(snapshot)) => {
                            boundary = Some((snapshot.event_sequence, snapshot.revision));
                            let reply_snapshot = snapshot.clone();
                            send_event(
                                events,
                                ConnectionEvent::Snapshot {
                                    pid,
                                    identity,
                                    snapshot,
                                },
                            )?;
                            Ok(CommandReply::Snapshot(reply_snapshot))
                        }
                        Ok(SnapshotOutcome::Unavailable(_)) => Ok(CommandReply::Unavailable),
                        Err(error) => Err(error),
                    },
                };
                request_id = SequenceNumber::new(request_id).next().get();
                match result {
                    Ok(reply) => {
                        let _ = call.reply.send(reply);
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
            request_id = SequenceNumber::new(request_id).next().get();
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
        request_id = SequenceNumber::new(request_id).next().get();

        if last_health.elapsed() >= HEALTH_INTERVAL {
            let health = request_tick_health(session, request_id)?;
            let sample_ms = u64::try_from(last_health.elapsed().as_millis()).unwrap_or(u64::MAX);
            request_id = SequenceNumber::new(request_id).next().get();
            last_health = Instant::now();
            if health.installed {
                if let Some(change) = tick_rate.observe(health.tick_count, sample_ms) {
                    send_tick_rate_change(events, pid, change)?;
                }
            } else {
                tick_rate.reset();
            }
            if diagnostics_supported {
                let diagnostics =
                    request_diagnostics(session, request_id, DiagnosticsOperation::Query)?;
                request_id = SequenceNumber::new(request_id).next().get();
                hook_timing.observe(pid, &diagnostics, events)?;
            }
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

fn request_diagnostics(
    session: &mut ControllerSession,
    request_id: u32,
    operation: DiagnosticsOperation,
) -> Result<DiagnosticsResponse, ControllerError> {
    session.send(Message::DiagnosticsRequest(DiagnosticsRequest {
        request_id,
        operation,
    }))?;
    let response = session.receive()?;
    match response.message {
        Message::DiagnosticsResponse(response) if response.request_id == request_id => Ok(response),
        Message::DiagnosticsResponse(response) => Err(ControllerError::Protocol(format!(
            "DiagnosticsResponse request ID {} does not match {request_id}",
            response.request_id
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected DiagnosticsResponse, received {:?}",
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

fn request_tick_health(
    session: &mut ControllerSession,
    request_id: u32,
) -> Result<TickHealthResponse, ControllerError> {
    session.send(Message::TickHealthRequest(TickHealthRequest { request_id }))?;
    let response = session.receive()?;
    match response.message {
        Message::TickHealthResponse(response) if response.request_id == request_id => Ok(response),
        Message::TickHealthResponse(response) => Err(ControllerError::Protocol(format!(
            "TickHealthResponse request ID {} does not match {request_id}",
            response.request_id
        ))),
        message => Err(ControllerError::Protocol(format!(
            "expected TickHealthResponse, received {:?}",
            message.message_type()
        ))),
    }
}

fn send_tick_rate_change(
    events: &Sender<DaemonEvent>,
    pid: u32,
    change: TickRateChange,
) -> Result<(), ControllerError> {
    let (state, tick_delta, sample_ms, rate_hz) = match change {
        TickRateChange::Degraded {
            tick_delta,
            sample_ms,
            rate_hz,
        } => ("degraded", tick_delta, sample_ms, rate_hz),
        TickRateChange::Recovered {
            tick_delta,
            sample_ms,
            rate_hz,
        } => ("recovered", tick_delta, sample_ms, rate_hz),
    };
    events
        .send(DaemonEvent::Timing(format!(
            concat!(
                "client pid={} timing=tick_rate_{} rate_hz={} ",
                "threshold_hz={} tick_delta={} sample_ms={}"
            ),
            pid, state, rate_hz, MIN_TICK_RATE_HZ, tick_delta, sample_ms
        )))
        .map_err(|_| ControllerError::Protocol("daemon event channel closed".into()))
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

fn validate_event_batch(
    after_sequence: u32,
    after_revision: u32,
    events: &[darpc_model::StateEvent],
) -> Option<(u32, u32)> {
    let mut boundary = (after_sequence, after_revision);
    for event in events {
        if event.sequence != SequenceNumber::new(boundary.0).next().get()
            || event.revision != SequenceNumber::new(boundary.1).next().get()
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
    use super::{
        HookTimingMonitor, TickRateChange, TickRateMonitor, validate_event_batch, validate_identity,
    };
    use crate::event::DaemonEvent;
    use darpc_game_client::{CLIENT_VERSION_CODE, EXECUTABLE_SHA256};
    use darpc_model::{StateEvent, StateUpdate, StatusUpdate};
    use darpc_protocol::{
        Architecture, ComponentVersion, DiagnosticsMode, DiagnosticsResponse, Hello,
        HookTimingRecord, HookTimingStage, SUPPORTED_VERSIONS,
    };
    use std::sync::mpsc;

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

    #[test]
    fn reports_sustained_tick_rate_degradation_and_recovery() {
        let mut monitor = TickRateMonitor::default();
        assert_eq!(monitor.observe(100, 1_000), None);
        assert_eq!(monitor.observe(110, 1_000), None);
        assert_eq!(monitor.observe(120, 1_000), None);
        assert_eq!(
            monitor.observe(130, 1_000),
            Some(TickRateChange::Degraded {
                tick_delta: 10,
                sample_ms: 1_000,
                rate_hz: 10,
            })
        );
        assert_eq!(monitor.observe(140, 1_000), None);
        assert_eq!(
            monitor.observe(240, 1_000),
            Some(TickRateChange::Recovered {
                tick_delta: 100,
                sample_ms: 1_000,
                rate_hz: 100,
            })
        );
    }

    #[test]
    fn tick_rate_monitor_handles_counter_wraparound() {
        let mut monitor = TickRateMonitor::default();
        assert_eq!(monitor.observe(u32::MAX - 20, 1_000), None);
        assert_eq!(monitor.observe(79, 1_000), None);
    }

    #[test]
    fn hook_timing_monitor_reports_counts_after_counter_reset() {
        let response = |over_budget_count| DiagnosticsResponse {
            request_id: 1,
            mode: DiagnosticsMode::HookTiming,
            hook_timings: std::array::from_fn(|index| HookTimingRecord {
                stage: [
                    HookTimingStage::Tick,
                    HookTimingStage::Movement,
                    HookTimingStage::Commands,
                    HookTimingStage::Player,
                    HookTimingStage::State,
                    HookTimingStage::Snapshot,
                    HookTimingStage::Event,
                ][index],
                budget_us: 5_000,
                call_count: 0,
                total_duration_us: 0,
                maximum_duration_us: 6_000,
                over_budget_count: if index == 0 { over_budget_count } else { 0 },
                last_duration_us: 6_000,
            }),
        };
        let (events, received) = mpsc::channel();
        let mut monitor = HookTimingMonitor::default();

        monitor.observe(42, &response(10), &events).unwrap();
        monitor.observe(42, &response(2), &events).unwrap();

        let messages: Vec<_> = received.try_iter().collect();
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[1],
            DaemonEvent::Timing(message) if message.contains("over_budget_delta=2")
        ));
    }
}
