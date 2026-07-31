//! daRPC daemon.

#[cfg(any(windows, test))]
mod api;
#[cfg(windows)]
mod connection;
#[cfg(any(windows, test))]
mod registry;

use std::{collections::BTreeSet, env, ffi::OsString, process::ExitCode};

const DEFAULT_PORT: u16 = 2626;
const USAGE: &str = "usage: darpcd --pid <pid> [--pid <pid> ...] [--port <port>]";

#[derive(Debug, Eq, PartialEq)]
struct Options {
    pids: Vec<u32>,
    port: u16,
}

fn main() -> ExitCode {
    let options = match parse_options(env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("darpcd: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("darpcd: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse_options(arguments: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut arguments = arguments.into_iter();
    let mut pids = Vec::new();
    let mut unique = BTreeSet::new();
    let mut port = None;

    while let Some(option) = arguments.next() {
        if option == "--pid" {
            let value = arguments
                .next()
                .ok_or_else(|| "--pid requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "PID must be valid Unicode".to_owned())?;
            let pid: u32 = value
                .parse()
                .map_err(|_| "PID must be an unsigned 32-bit integer".to_owned())?;
            if pid == 0 {
                return Err("PID must be greater than zero".into());
            }
            if !unique.insert(pid) {
                return Err(format!("PID {pid} was provided more than once"));
            }
            pids.push(pid);
        } else if option == "--port" {
            if port.is_some() {
                return Err("--port may be provided only once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| "--port requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "port must be valid Unicode".to_owned())?;
            let parsed: u16 = value
                .parse()
                .map_err(|_| "port must be an integer from 1 through 65535".to_owned())?;
            if parsed == 0 {
                return Err("port must be greater than zero".into());
            }
            port = Some(parsed);
        } else {
            return Err(format!("unknown option `{}`", option.to_string_lossy()));
        }
    }

    if pids.is_empty() {
        return Err("at least one --pid target is required".into());
    }
    Ok(Options {
        pids,
        port: port.unwrap_or(DEFAULT_PORT),
    })
}

#[cfg(windows)]
fn run(options: Options) -> Result<(), String> {
    use api::ApiState;
    use registry::{ConnectionEvent, Registry};
    use std::{io::Write as _, sync::mpsc};

    let mut registry = Registry::new();
    for &pid in &options.pids {
        let event = ConnectionEvent::Connecting { pid };
        registry.apply(&event);
    }

    let api_state = ApiState::new(registry.snapshot());
    let _api_worker = api::start(options.port, api_state.clone())
        .map_err(|error| format!("failed to listen on 127.0.0.1:{}: {error}", options.port))?;
    println!("HTTP API listening on http://127.0.0.1:{}", options.port);
    for &pid in &options.pids {
        println!(
            "{}",
            registry::render_event(&ConnectionEvent::Connecting { pid })
        );
    }
    let _ = std::io::stdout().flush();

    let (sender, receiver) = mpsc::channel();
    let mut workers = Vec::with_capacity(options.pids.len());
    for pid in options.pids {
        workers.push(connection::spawn(pid, sender.clone()).map_err(|error| {
            format!("failed to start connection worker for PID {pid}: {error}")
        })?);
    }
    drop(sender);

    for event in receiver {
        if registry.apply(&event) {
            api_state.publish(registry.snapshot());
            println!("{}", registry::render_event(&event));
            let _ = std::io::stdout().flush();
        }
    }
    Err("all client connection workers stopped".into())
}

#[cfg(not(windows))]
fn run(_options: Options) -> Result<(), String> {
    Err("the daRPC daemon requires Windows".into())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PORT, Options, parse_options};
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_repeated_pid_options_in_order() {
        assert_eq!(
            parse_options(arguments(&["--pid", "42", "--pid", "7"])).unwrap(),
            Options {
                pids: vec![42, 7],
                port: DEFAULT_PORT,
            }
        );
        assert_eq!(
            parse_options(arguments(&["--port", "3000", "--pid", "42"])).unwrap(),
            Options {
                pids: vec![42],
                port: 3000,
            }
        );
    }

    #[test]
    fn rejects_invalid_targets_and_ports() {
        assert!(parse_options(Vec::<OsString>::new()).is_err());
        assert!(parse_options(arguments(&["--pid", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--pid", "7"])).is_err());
        assert!(parse_options(arguments(&["--pids", "7,8"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "65536"])).is_err());
        assert!(
            parse_options(arguments(&[
                "--pid", "7", "--port", "2626", "--port", "2627"
            ]))
            .is_err()
        );
    }
}
