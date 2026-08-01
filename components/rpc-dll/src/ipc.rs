use darpc_protocol::{
    EchoResponse, EndpointRole, Frame, Handshake, Hello, Message, MessageDirection, Pong,
    SequenceCounter, TickHealthResponse,
};
use darpc_win32::pipe::{PipeServer, StopEvent, pipe_name, sender_tick_ms};
use std::{
    fs::File,
    io::{self, Write},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::tick_hook;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
                let health = tick_hook::health();
                let sample_tick_ms = sender_tick_ms();
                let _ = writeln!(
                    log,
                    concat!(
                        "event=hook_health hook={} installed={} relocated_bytes={} ",
                        "ticks={} sample_tick_ms={}"
                    ),
                    tick_hook::NAME,
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
