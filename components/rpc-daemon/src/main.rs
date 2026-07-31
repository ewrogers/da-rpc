//! daRPC daemon.

#[cfg(windows)]
mod connection;
#[cfg(windows)]
mod registry;

use std::{collections::BTreeSet, env, ffi::OsString, process::ExitCode};

const USAGE: &str = "usage: darpcd --pid <pid> [--pid <pid> ...]";

fn main() -> ExitCode {
    let pids = match parse_pids(env::args_os().skip(1)) {
        Ok(pids) => pids,
        Err(error) => {
            eprintln!("darpcd: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(pids) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("darpcd: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse_pids(arguments: impl IntoIterator<Item = OsString>) -> Result<Vec<u32>, String> {
    let mut arguments = arguments.into_iter();
    let mut pids = Vec::new();
    let mut unique = BTreeSet::new();

    while let Some(option) = arguments.next() {
        if option != "--pid" {
            return Err(format!("unknown option `{}`", option.to_string_lossy()));
        }
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
    }

    if pids.is_empty() {
        return Err("at least one --pid target is required".into());
    }
    Ok(pids)
}

#[cfg(windows)]
fn run(pids: Vec<u32>) -> Result<(), String> {
    use registry::Registry;
    use std::{io::Write as _, sync::mpsc};

    let (sender, receiver) = mpsc::channel();
    let mut workers = Vec::with_capacity(pids.len());
    for pid in pids {
        workers.push(connection::spawn(pid, sender.clone()).map_err(|error| {
            format!("failed to start connection worker for PID {pid}: {error}")
        })?);
    }
    drop(sender);

    let mut registry = Registry::new();
    for event in receiver {
        if registry.apply(&event) {
            println!("{}", registry::render_event(&event));
            let _ = std::io::stdout().flush();
        }
    }
    Err("all client connection workers stopped".into())
}

#[cfg(not(windows))]
fn run(_pids: Vec<u32>) -> Result<(), String> {
    Err("the daRPC daemon requires Windows".into())
}

#[cfg(test)]
mod tests {
    use super::parse_pids;
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_repeated_pid_options_in_order() {
        assert_eq!(
            parse_pids(arguments(&["--pid", "42", "--pid", "7"])).unwrap(),
            vec![42, 7]
        );
    }

    #[test]
    fn rejects_missing_zero_duplicate_and_unknown_targets() {
        assert!(parse_pids(Vec::<OsString>::new()).is_err());
        assert!(parse_pids(arguments(&["--pid", "0"])).is_err());
        assert!(parse_pids(arguments(&["--pid", "7", "--pid", "7"])).is_err());
        assert!(parse_pids(arguments(&["--pids", "7,8"])).is_err());
    }
}
