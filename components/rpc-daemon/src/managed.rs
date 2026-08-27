use crate::event::DaemonEvent;
use std::{
    io::{self, Read},
    sync::mpsc::Sender,
    thread::{self, JoinHandle},
};

pub(crate) struct ManagedLifetime {
    _worker: JoinHandle<()>,
}

impl ManagedLifetime {
    #[cfg(windows)]
    pub(crate) fn start(events: Sender<DaemonEvent>) -> io::Result<Self> {
        Self::start_with_reader(io::stdin(), events)
    }

    fn start_with_reader(
        mut reader: impl Read + Send + 'static,
        events: Sender<DaemonEvent>,
    ) -> io::Result<Self> {
        let worker = thread::Builder::new()
            .name("darpcd-managed-lifetime".into())
            .spawn(move || {
                let result = wait_for_eof(&mut reader);
                let _ = events.send(DaemonEvent::ManagedShutdown(result));
            })?;
        Ok(Self { _worker: worker })
    }
}

fn wait_for_eof(reader: &mut impl Read) -> io::Result<()> {
    let mut buffer = [0_u8; 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ManagedLifetime;
    use crate::event::DaemonEvent;
    use std::{
        io::{self, Read},
        sync::mpsc,
        time::Duration,
    };

    struct LifetimePipe {
        receiver: mpsc::Receiver<io::Result<Vec<u8>>>,
    }

    impl Read for LifetimePipe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self.receiver.recv() {
                Ok(Ok(bytes)) => {
                    let length = bytes.len().min(buffer.len());
                    buffer[..length].copy_from_slice(&bytes[..length]);
                    Ok(length)
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Ok(0),
            }
        }
    }

    #[test]
    fn lifetime_pipe_requests_shutdown_only_after_eof() {
        let (input, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        let lifetime = ManagedLifetime::start_with_reader(LifetimePipe { receiver }, events)
            .expect("lifetime worker should start");

        input.send(Ok(b"not a command".to_vec())).unwrap();
        assert!(
            event_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );

        drop(input);
        let DaemonEvent::ManagedShutdown(result) = event_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("EOF should request shutdown")
        else {
            panic!("expected managed shutdown event");
        };
        result.expect("EOF should be a normal shutdown");
        lifetime._worker.join().unwrap();
    }

    #[test]
    fn lifetime_pipe_reports_read_failure() {
        let (input, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        let lifetime = ManagedLifetime::start_with_reader(LifetimePipe { receiver }, events)
            .expect("lifetime worker should start");

        input
            .send(Err(io::Error::other("lifetime pipe failed")))
            .unwrap();
        let DaemonEvent::ManagedShutdown(result) = event_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("read failure should request shutdown")
        else {
            panic!("expected managed shutdown event");
        };
        assert_eq!(result.unwrap_err().to_string(), "lifetime pipe failed");
        lifetime._worker.join().unwrap();
    }
}
