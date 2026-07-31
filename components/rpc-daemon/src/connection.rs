use crate::{
    event::DaemonEvent,
    registry::{ClientIdentity, ConnectionEvent},
};
#[cfg(debug_assertions)]
use darpc_game_client::DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE;
use darpc_game_client::{EXECUTABLE_SHA256, LAYOUT_ID};
use darpc_protocol::{Architecture, Hello, Message, Ping, Pong};
use darpc_win32::controller::{ControllerError, ControllerSession};
#[cfg(debug_assertions)]
use std::env;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};

const RETRY_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_INTERVAL: Duration = Duration::from_secs(1);
const INITIALIZATION_GRACE: Duration = Duration::from_secs(1);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) struct Worker {
    stop: Arc<AtomicBool>,
    _handle: JoinHandle<()>,
}

impl Worker {
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

pub(crate) fn spawn(pid: u32, events: Sender<DaemonEvent>) -> io::Result<Worker> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name(format!("darpcd-client-{pid}"))
        .spawn(move || run(pid, events, &worker_stop))?;
    Ok(Worker {
        stop,
        _handle: handle,
    })
}

fn run(pid: u32, events: Sender<DaemonEvent>, stop: &AtomicBool) {
    if !emit(&events, ConnectionEvent::Connecting { pid }) {
        return;
    }
    let discovered_at = Instant::now();

    while !stop.load(Ordering::Acquire) {
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

                if let Err(error) = monitor(&mut session, stop)
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
            return;
        }
    }
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
        && hello.layout_id == 0
    {
        return Ok(());
    }

    if hello.executable_fingerprint != EXECUTABLE_SHA256 {
        return Err("unsupported client executable fingerprint".into());
    }
    if hello.layout_id != LAYOUT_ID {
        return Err(format!(
            "unsupported client layout {}; expected {LAYOUT_ID}",
            hello.layout_id
        ));
    }
    Ok(())
}

fn monitor(session: &mut ControllerSession, stop: &AtomicBool) -> Result<(), ControllerError> {
    let mut request_id = 1_u32;
    while !wait_for_stop(stop, HEALTH_INTERVAL) {
        session.send(Message::Ping(Ping { request_id }))?;
        let response = session.receive()?;
        match response.message {
            Message::Pong(Pong {
                request_id: response_id,
            }) if response_id == request_id => {}
            Message::Pong(Pong {
                request_id: response_id,
            }) => {
                return Err(ControllerError::Protocol(format!(
                    "Pong request ID {response_id} does not match {request_id}"
                )));
            }
            message => {
                return Err(ControllerError::Protocol(format!(
                    "expected Pong, received {:?}",
                    message.message_type()
                )));
            }
        }
        request_id = request_id.wrapping_add(1);
    }
    Ok(())
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
    use super::validate_identity;
    use darpc_game_client::{EXECUTABLE_SHA256, LAYOUT_ID};
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
            layout_id: LAYOUT_ID,
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

        let mut wrong_layout = hello();
        wrong_layout.layout_id = LAYOUT_ID + 1;
        assert!(validate_identity(wrong_layout).is_err());
    }
}
