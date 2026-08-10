use darpc_protocol::{
    CommandResponse, CommandResult, EchoResponse, EndpointRole, EventPollResponse, Frame,
    Handshake, Hello, MAX_EVENT_POLL_WAIT_MS, MAX_EVENTS_PER_POLL, Message, MessageDirection, Pong,
    SequenceCounter, SnapshotResponse, SnapshotResult, SnapshotUnavailableReason,
    TickHealthResponse,
};
use darpc_win32::pipe::{PipeServer, StopEvent, pipe_name, sender_tick_ms};
use std::{
    fs::File,
    io::{self, Write},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{commands, hooks::tick, snapshot, state};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct IpcWorker {
    stop: StopEvent,
    worker: Option<JoinHandle<()>>,
}

impl IpcWorker {
    pub(crate) fn start(hello: Hello, log: File) -> io::Result<Self> {
        let stop = StopEvent::new()?;
        let worker_stop = stop.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("darpc-ipc".into())
            .spawn(move || run(worker_stop, hello, log, ready_sender))?;

        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(io::Error::other("IPC worker stopped during startup"))
            }
        }
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        let Some(worker) = self.worker.as_ref() else {
            return Ok(());
        };

        self.stop.signal()?;
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC worker did not stop within five seconds",
                ));
            }
            thread::sleep(SHUTDOWN_POLL_INTERVAL);
        }

        let worker = self.worker.take().expect("worker was present");
        worker
            .join()
            .map_err(|_| io::Error::other("IPC worker panicked"))
    }
}

fn run(stop: StopEvent, hello: Hello, mut log: File, ready: mpsc::SyncSender<io::Result<()>>) {
    let server = match PipeServer::bind(hello.process_id, stop.clone()) {
        Ok(server) => {
            if ready.send(Ok(())).is_err() {
                return;
            }
            server
        }
        Err(error) => {
            let startup_error = io::Error::new(error.kind(), error.to_string());
            let _ = ready.send(Err(startup_error));
            return;
        }
    };

    let _ = writeln!(
        log,
        "event=ipc_listening pid={} pipe={}",
        hello.process_id,
        pipe_name(hello.process_id)
    );

    while !stop.is_signaled() {
        match server.accept() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
            Err(error) => {
                let _ = writeln!(log, "event=ipc_accept_failed error={error}");
                break;
            }
        }

        let _ = writeln!(log, "event=ipc_connected pid={}", hello.process_id);
        let result = serve_connection(&server, &hello, &mut log);
        let _ = server.disconnect();

        match result {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
            Err(error) => {
                let _ = writeln!(log, "event=ipc_disconnected error={error}");
            }
        }
    }

    let _ = writeln!(log, "event=ipc_stopped pid={}", hello.process_id);
}

fn serve_connection(server: &PipeServer, hello: &Hello, log: &mut File) -> io::Result<()> {
    let mut handshake = Handshake::new(EndpointRole::Dll);
    let mut incoming_sequence = SequenceCounter::new();
    let mut outgoing_sequence = SequenceCounter::new();

    send(
        server,
        &mut handshake,
        &mut outgoing_sequence,
        Message::Hello(*hello),
    )?;

    let acknowledgement = receive(server, &mut handshake, &mut incoming_sequence)?;
    if !matches!(acknowledgement, Message::HelloAck(_)) || !handshake.is_ready() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "client did not complete the Hello handshake",
        ));
    }
    loop {
        let message = receive(server, &mut handshake, &mut incoming_sequence)?;
        let response = match message {
            Message::Ping(message) => Message::Pong(Pong {
                request_id: message.request_id,
            }),
            Message::EchoRequest(message) => Message::EchoResponse(EchoResponse {
                request_id: message.request_id,
                text: message.text,
            }),
            Message::TickHealthRequest(message) => {
                let health = tick::health();
                let sample_tick_ms = sender_tick_ms();
                let _ = writeln!(
                    log,
                    concat!(
                        "event=hook_health hook={} installed={} relocated_bytes={} ",
                        "ticks={} sample_tick_ms={}"
                    ),
                    tick::NAME,
                    health.installed,
                    health.relocated_bytes,
                    health.tick_count,
                    sample_tick_ms
                );
                Message::TickHealthResponse(TickHealthResponse {
                    request_id: message.request_id,
                    installed: health.installed,
                    relocated_bytes: health.relocated_bytes,
                    tick_count: health.tick_count,
                })
            }
            Message::SnapshotRequest(message) => {
                let result = if !tick::health().installed {
                    SnapshotResult::Unavailable(SnapshotUnavailableReason::HookUnavailable)
                } else {
                    let generation = snapshot::request();
                    match snapshot::wait(generation, SNAPSHOT_TIMEOUT) {
                        Ok(snapshot) => {
                            state::rebase(snapshot.event_sequence);
                            let _ = writeln!(
                                log,
                                concat!(
                                    "event=snapshot_captured revision={} event_sequence={} ",
                                    "lifecycle={:?} world_generation={} duration_us={}"
                                ),
                                snapshot.revision,
                                snapshot.event_sequence,
                                snapshot.lifecycle,
                                snapshot.world_generation,
                                snapshot.capture_duration_us
                            );
                            SnapshotResult::Ready(Box::new(snapshot))
                        }
                        Err(snapshot::WaitError::TimedOut) => {
                            let _ = writeln!(log, "event=snapshot_failed reason=capture_timed_out");
                            SnapshotResult::Unavailable(SnapshotUnavailableReason::CaptureTimedOut)
                        }
                        Err(snapshot::WaitError::Capture(error)) => {
                            let _ = writeln!(log, "event=snapshot_failed reason={error}");
                            SnapshotResult::Unavailable(SnapshotUnavailableReason::CaptureFailed)
                        }
                    }
                };
                Message::SnapshotResponse(SnapshotResponse {
                    request_id: message.request_id,
                    result,
                })
            }
            Message::EventPollRequest(message) => {
                if message.max_events == 0 || message.max_events > MAX_EVENTS_PER_POLL {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid event poll limit {}", message.max_events),
                    ));
                }
                if message.wait_ms > MAX_EVENT_POLL_WAIT_MS {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid event poll wait {} ms", message.wait_ms),
                    ));
                }
                Message::EventPollResponse(EventPollResponse {
                    request_id: message.request_id,
                    result: state::poll(
                        message.after_sequence,
                        message.max_events,
                        Duration::from_millis(u64::from(message.wait_ms)),
                    ),
                })
            }
            Message::CommandRequest(message) => {
                let result = if tick::health().installed {
                    commands::handle(message.operation)
                } else {
                    CommandResult::Unavailable
                };
                let _ = writeln!(
                    log,
                    "event=command request_id={} result={result:?}",
                    message.request_id
                );
                Message::CommandResponse(CommandResponse {
                    request_id: message.request_id,
                    result,
                })
            }
            message => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected request message {:?}", message.message_type()),
                ));
            }
        };

        send(server, &mut handshake, &mut outgoing_sequence, response)?;
    }
}

fn receive(
    server: &PipeServer,
    handshake: &mut Handshake,
    sequence: &mut SequenceCounter,
) -> io::Result<Message> {
    let frame = server.receive_frame()?;
    sequence
        .observe(frame.sequence)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    handshake
        .observe(MessageDirection::Inbound, &frame.message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(frame.message)
}

fn send(
    server: &PipeServer,
    handshake: &mut Handshake,
    sequence: &mut SequenceCounter,
    message: Message,
) -> io::Result<()> {
    handshake
        .observe(MessageDirection::Outbound, &message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let frame = Frame::new(sequence.take(), sender_tick_ms(), message);
    server.send_frame(&frame)
}
