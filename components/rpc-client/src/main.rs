//! daRPC command-line client.

mod error;
#[cfg(windows)]
mod ipc;
mod output;
mod snapshot_output;

use darpc_protocol::MAX_ECHO_TEXT_LEN;
use error::{ClientError, ErrorKind, Result};
use output::{CommandResult, OutputFormat, render_error};
use std::{env, ffi::OsString, process::ExitCode};

const USAGE: &str = "\
usage:
    darpc [--output <table|json>] hello --pid <pid>
    darpc [--output <table|json>] ping --pid <pid>
    darpc [--output <table|json>] tick-health --pid <pid>
    darpc [--output <table|json>] snapshot --pid <pid>
    darpc [--output <table|json>] echo --pid <pid> <text>";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Command {
    pid: u32,
    operation: Operation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Operation {
    Hello,
    Ping,
    TickHealth,
    Snapshot,
    Echo(String),
}

impl Command {
    const fn name(&self) -> &'static str {
        match &self.operation {
            Operation::Hello => "hello",
            Operation::Ping => "ping",
            Operation::TickHealth => "tick-health",
            Operation::Snapshot => "snapshot",
            Operation::Echo(_) => "echo",
        }
    }
}

fn main() -> ExitCode {
    let mut arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let format = match parse_output_format(&mut arguments) {
        Ok(format) => format,
        Err(error) => {
            eprintln!("{}", render_error(OutputFormat::Table, None, &error));
            return ExitCode::from(error.kind().exit_code());
        }
    };
    let command = match parse_command(arguments) {
        Ok(command) => command,
        Err(error) => {
            let rendered = render_error(format, None, &error);
            match format {
                OutputFormat::Table => eprintln!("{rendered}"),
                OutputFormat::Json => println!("{rendered}"),
            }
            return ExitCode::from(error.kind().exit_code());
        }
    };
    let command_name = command.name();

    match execute(command) {
        Ok(result) => {
            println!("{}", result.render(format));
            ExitCode::SUCCESS
        }
        Err(error) => {
            let rendered = render_error(format, Some(command_name), &error);
            match format {
                OutputFormat::Table => eprintln!("{rendered}"),
                OutputFormat::Json => println!("{rendered}"),
            }
            ExitCode::from(error.kind().exit_code())
        }
    }
}

#[cfg(test)]
fn parse(mut arguments: Vec<OsString>) -> Result<(OutputFormat, Command)> {
    let format = parse_output_format(&mut arguments)?;
    let command = parse_command(arguments)?;
    Ok((format, command))
}

fn parse_output_format(arguments: &mut Vec<OsString>) -> Result<OutputFormat> {
    let format = if arguments.first().and_then(|value| value.to_str()) == Some("--output") {
        if arguments.len() < 2 {
            return Err(invalid_arguments("--output requires `table` or `json`"));
        }
        let value = arguments.remove(1);
        arguments.remove(0);
        match value.to_str() {
            Some("table") => OutputFormat::Table,
            Some("json") => OutputFormat::Json,
            _ => return Err(invalid_arguments("--output must be `table` or `json`")),
        }
    } else {
        OutputFormat::Table
    };
    Ok(format)
}

fn parse_command(arguments: Vec<OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();
    let action = arguments
        .next()
        .and_then(|value| value.to_str().map(str::to_owned))
        .ok_or_else(|| ClientError::new(ErrorKind::InvalidArguments, USAGE))?;
    let pid_option = arguments
        .next()
        .and_then(|value| value.to_str().map(str::to_owned));
    if pid_option.as_deref() != Some("--pid") {
        return Err(invalid_arguments("commands require `--pid <pid>`"));
    }
    let pid = parse_pid(arguments.next())?;

    let operation = match action.as_str() {
        "hello" => Operation::Hello,
        "ping" => Operation::Ping,
        "tick-health" => Operation::TickHealth,
        "snapshot" => Operation::Snapshot,
        "echo" => {
            let text = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| invalid_arguments("echo requires UTF-8 text"))?;
            if text.len() > MAX_ECHO_TEXT_LEN {
                return Err(invalid_arguments(format!(
                    "echo text is {} bytes; maximum is {MAX_ECHO_TEXT_LEN}",
                    text.len()
                )));
            }
            Operation::Echo(text)
        }
        _ => return Err(invalid_arguments(format!("unknown command `{action}`"))),
    };

    if arguments.next().is_some() {
        return Err(invalid_arguments("too many arguments"));
    }

    Ok(Command { pid, operation })
}

fn parse_pid(argument: Option<OsString>) -> Result<u32> {
    let argument = argument.ok_or_else(|| invalid_arguments("--pid requires a value"))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("PID must be valid Unicode"))?;
    let pid = argument
        .parse()
        .map_err(|_| invalid_arguments("PID must be an unsigned 32-bit integer"))?;
    if pid == 0 {
        return Err(invalid_arguments("PID must be greater than zero"));
    }
    Ok(pid)
}

fn invalid_arguments(message: impl Into<String>) -> ClientError {
    ClientError::new(
        ErrorKind::InvalidArguments,
        format!("{}\n{USAGE}", message.into()),
    )
}

#[cfg(windows)]
fn execute(command: Command) -> Result<CommandResult> {
    ipc::execute(command.pid, command.operation)
}

#[cfg(not(windows))]
fn execute(_command: Command) -> Result<CommandResult> {
    Err(ClientError::new(
        ErrorKind::UnsupportedPlatform,
        "direct IPC diagnostics require Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::{Command, Operation, OutputFormat, parse};
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_direct_commands() {
        assert_eq!(
            parse(arguments(&["hello", "--pid", "42"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 42,
                    operation: Operation::Hello,
                }
            )
        );
        assert_eq!(
            parse(arguments(&[
                "--output", "json", "echo", "--pid", "7", "hello"
            ]))
            .unwrap(),
            (
                OutputFormat::Json,
                Command {
                    pid: 7,
                    operation: Operation::Echo("hello".into()),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["tick-health", "--pid", "9"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 9,
                    operation: Operation::TickHealth,
                }
            )
        );
        assert_eq!(
            parse(arguments(&["snapshot", "--pid", "10"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Snapshot,
                }
            )
        );
    }

    #[test]
    fn rejects_invalid_pid_and_extra_arguments() {
        assert!(parse(arguments(&["ping", "--pid", "0"])).is_err());
        assert!(parse(arguments(&["hello", "--pid", "1", "extra"])).is_err());
    }

    #[test]
    fn prints_usage_once_for_incomplete_commands() {
        let error = parse(arguments(&["hello"])).unwrap_err();
        assert_eq!(error.message().matches("usage:").count(), 1);
    }

    #[test]
    fn rejects_echo_over_the_wire_limit() {
        let text = "a".repeat(darpc_protocol::MAX_ECHO_TEXT_LEN + 1);
        assert!(parse(vec!["echo".into(), "--pid".into(), "1".into(), text.into(),]).is_err());
    }
}
