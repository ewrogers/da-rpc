#![cfg(windows)]

use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

struct TestChild(Child);

impl TestChild {
    fn spawn(address: SocketAddrV4, managed: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_darpcd"));
        command
            .arg("--listen")
            .arg(address.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if managed {
            command.arg("--managed");
        }
        Self(command.spawn().expect("darpcd.exe should start"))
    }

    fn close_stdin(&mut self) {
        drop(self.0.stdin.take());
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn output(&mut self) -> String {
        let mut output = String::new();
        if let Some(stdout) = self.0.stdout.as_mut() {
            let _ = stdout.read_to_string(&mut output);
        }
        if let Some(stderr) = self.0.stderr.as_mut() {
            let _ = stderr.read_to_string(&mut output);
        }
        output
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn available_address() -> SocketAddrV4 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let SocketAddr::V4(address) = listener.local_addr().unwrap() else {
        unreachable!("IPv4 bind returned an IPv6 address");
    };
    address
}

fn wait_for_health(child: &mut TestChild, address: SocketAddrV4) {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            panic!(
                "darpcd.exe exited before becoming healthy ({status}): {}",
                child.output()
            );
        }
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&SocketAddr::V4(address), Duration::from_millis(100))
        {
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            if response.starts_with("HTTP/1.1 200 OK") {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "darpcd.exe did not become healthy"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn managed_daemon_exits_successfully_when_parent_closes_stdin() {
    let address = available_address();
    let mut child = TestChild::spawn(address, true);
    wait_for_health(&mut child, address);

    child.close_stdin();

    let status = child
        .wait_for_exit(SHUTDOWN_TIMEOUT)
        .expect("managed daemon did not stop");
    assert!(
        status.success(),
        "managed daemon failed ({status}): {}",
        child.output()
    );
    TcpListener::bind(address).expect("managed shutdown should release the HTTP listener");
}

#[test]
fn unmanaged_daemon_ignores_stdin_eof() {
    let address = available_address();
    let mut child = TestChild::spawn(address, false);
    wait_for_health(&mut child, address);

    child.close_stdin();
    thread::sleep(Duration::from_millis(250));

    assert!(
        child.0.try_wait().unwrap().is_none(),
        "unmanaged daemon unexpectedly stopped: {}",
        child.output()
    );
}
