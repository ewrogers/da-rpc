//! daRPC client launcher and injector.

mod error;
mod inject;
mod launch;
mod output;
mod pe;
mod process;
mod remote;
mod remote_dll;

use darpc_client_741::{CLIENT_VERSION, ClientExecutable, executable_sha256};
use error::{ErrorKind, LoaderError, Result};
use output::{CommandResult, OutputFormat, render_error};
use pe::DarpcDll;
use process::{ProcessInspection, TargetProcess, inspect};

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

#[cfg(debug_assertions)]
use std::fs;

#[cfg(debug_assertions)]
const TEST_CLIENT_BYPASS_ENVIRONMENT_VARIABLE: &str = "DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT";

const USAGE: &str = "\
usage:
    loader [--json] inspect <pid>
    loader [--json] attach <pid> <dll-path>
    loader [--json] detach <pid> <dll-path>
    loader [--json] launch <executable-path> <dll-path> [-- <argument>...]";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Inspect {
        pid: u32,
    },
    Attach {
        pid: u32,
        dll_path: PathBuf,
    },
    Detach {
        pid: u32,
        dll_path: PathBuf,
    },
    Launch {
        executable_path: PathBuf,
        dll_path: PathBuf,
        arguments: Vec<OsString>,
    },
}

impl Command {
    const fn name(&self) -> &'static str {
        match self {
            Self::Inspect { .. } => "inspect",
            Self::Attach { .. } => "attach",
            Self::Detach { .. } => "detach",
            Self::Launch { .. } => "launch",
        }
    }
}

fn main() -> ExitCode {
    let mut arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let format = parse_output_format(&mut arguments);

    let command = match parse_command(arguments) {
        Ok(command) => command,
        Err(error) => {
            print_error(format, None, &error);
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
            print_error(format, Some(command_name), &error);
            ExitCode::from(error.kind().exit_code())
        }
    }
}

fn print_error(format: OutputFormat, command: Option<&str>, error: &LoaderError) {
    let rendered = render_error(format, command, error);

    match format {
        OutputFormat::Human => eprintln!("{rendered}"),
        OutputFormat::Json => println!("{rendered}"),
    }
}

fn parse_output_format(arguments: &mut Vec<OsString>) -> OutputFormat {
    if arguments.first().and_then(|argument| argument.to_str()) == Some("--json") {
        arguments.remove(0);
        OutputFormat::Json
    } else {
        OutputFormat::Human
    }
}

fn parse_command(arguments: Vec<OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();

    let command = arguments.next().ok_or_else(|| invalid_arguments(USAGE))?;

    let command = match command.to_str() {
        Some("inspect") => Command::Inspect {
            pid: parse_pid(arguments.next())?,
        },
        Some("attach") => Command::Attach {
            pid: parse_pid(arguments.next())?,
            dll_path: parse_dll_path(arguments.next())?,
        },
        Some("detach") => Command::Detach {
            pid: parse_pid(arguments.next())?,
            dll_path: parse_dll_path(arguments.next())?,
        },
        Some("launch") => {
            let executable_path = parse_path(arguments.next())?;
            let dll_path = parse_dll_path(arguments.next())?;
            let Some(separator) = arguments.next() else {
                return Ok(Command::Launch {
                    executable_path,
                    dll_path,
                    arguments: Vec::new(),
                });
            };

            if separator != "--" {
                return Err(invalid_arguments(format!(
                    "launch arguments must follow `--`\n{USAGE}"
                )));
            }

            return Ok(Command::Launch {
                executable_path,
                dll_path,
                arguments: arguments.collect(),
            });
        }
        Some(command) => {
            return Err(invalid_arguments(format!(
                "unknown command: `{command}`\n{USAGE}"
            )));
        }
        None => {
            return Err(invalid_arguments(format!(
                "command must be valid Unicode\n{USAGE}"
            )));
        }
    };

    if arguments.next().is_some() {
        return Err(invalid_arguments(format!("too many arguments\n{USAGE}")));
    }

    Ok(command)
}

fn parse_pid(argument: Option<OsString>) -> Result<u32> {
    let argument = argument.ok_or_else(|| invalid_arguments(USAGE))?;

    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("PID must be valid Unicode"))?;

    let pid = argument
        .parse::<u32>()
        .map_err(|_| invalid_arguments("PID must be an unsigned 32-bit integer"))?;

    if pid == 0 {
        return Err(invalid_arguments("PID must be greater than zero"));
    }

    Ok(pid)
}

fn parse_dll_path(argument: Option<OsString>) -> Result<PathBuf> {
    parse_path(argument)
}

fn parse_path(argument: Option<OsString>) -> Result<PathBuf> {
    argument
        .map(PathBuf::from)
        .ok_or_else(|| invalid_arguments(USAGE))
}

fn invalid_arguments(message: impl Into<String>) -> LoaderError {
    LoaderError::new(ErrorKind::InvalidArguments, message)
}

fn execute(command: Command) -> Result<CommandResult> {
    match command {
        Command::Inspect { pid } => {
            let inspection = inspect(pid)?;
            Ok(command_result("inspect", pid, inspection, false))
        }
        Command::Attach { pid, dll_path } => {
            let dll = validate_dll(dll_path)?;
            let process = TargetProcess::open(pid)?;
            validate_client(process.executable_path()?)?;
            let outcome = inject::attach(&process, &dll)?;

            Ok(command_result(
                "attach",
                pid,
                outcome.inspection,
                outcome.changed,
            ))
        }
        Command::Detach { pid, dll_path } => {
            let dll = validate_dll(dll_path)?;
            let process = TargetProcess::open(pid)?;
            let outcome = inject::detach(&process, &dll)?;

            Ok(command_result(
                "detach",
                pid,
                outcome.inspection,
                outcome.changed,
            ))
        }
        Command::Launch {
            executable_path,
            dll_path,
            arguments,
        } => {
            let dll = validate_dll(dll_path)?;
            let executable_path = validate_client(executable_path)?;
            let outcome = launch::launch(&executable_path, &arguments, &dll)?;

            Ok(command_result(
                "launch",
                outcome.pid,
                outcome.inspection,
                outcome.changed,
            ))
        }
    }
}

fn validate_client(executable_path: PathBuf) -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    if env::var_os(TEST_CLIENT_BYPASS_ENVIRONMENT_VARIABLE).as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        let executable_path = fs::canonicalize(&executable_path).map_err(|error| {
            LoaderError::new(
                ErrorKind::UnsupportedClient,
                format!(
                    "failed to resolve test client `{}`: {error}",
                    executable_path.display(),
                ),
            )
        })?;

        eprintln!(
            "WARNING: debug-only unsupported-client test bypass enabled for {}",
            executable_path.display()
        );
        return Ok(executable_path);
    }

    let executable = ClientExecutable::validate(&executable_path)
        .map_err(|error| LoaderError::new(ErrorKind::UnsupportedClient, error))?;

    eprintln!(
        "Validated Dark Ages client {CLIENT_VERSION}: {} sha256={}",
        executable.path().display(),
        executable_sha256()
    );

    Ok(executable.path().to_owned())
}

fn validate_dll(dll_path: PathBuf) -> Result<DarpcDll> {
    let dll = DarpcDll::validate(dll_path)
        .map_err(|error| LoaderError::new(ErrorKind::InvalidDll, error))?;

    eprintln!(
        "Validated x86 DLL: {} initialize_rva=0x{:08X} shutdown_rva=0x{:08X}",
        dll.path.display(),
        dll.initialize_rva,
        dll.shutdown_rva
    );

    Ok(dll)
}

fn command_result(
    command: &'static str,
    pid: u32,
    inspection: ProcessInspection,
    changed: bool,
) -> CommandResult {
    CommandResult {
        command,
        pid,
        creation_time: inspection.creation_time,
        darpc_loaded: inspection.darpc_module.is_some(),
        module_base: inspection
            .darpc_module
            .map(|process_module| process_module.base),
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, OutputFormat, parse_command, parse_output_format};
    use std::{ffi::OsString, path::PathBuf};

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_process_commands() {
        assert_eq!(
            parse_command(arguments(&["inspect", "42"])).unwrap(),
            Command::Inspect { pid: 42 }
        );
        assert_eq!(
            parse_command(arguments(&["attach", "42", "darpc.dll"])).unwrap(),
            Command::Attach {
                pid: 42,
                dll_path: PathBuf::from("darpc.dll"),
            }
        );
        assert_eq!(
            parse_command(arguments(&["detach", "42", "darpc.dll"])).unwrap(),
            Command::Detach {
                pid: 42,
                dll_path: PathBuf::from("darpc.dll"),
            }
        );
    }

    #[test]
    fn parses_launch_with_forwarded_arguments() {
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "target.exe",
                "darpc.dll",
                "--",
                "--wait-ms",
                "10",
                "two words",
            ]))
            .unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: arguments(&["--wait-ms", "10", "two words"]),
            }
        );

        assert_eq!(
            parse_command(arguments(&["launch", "target.exe", "darpc.dll"])).unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: Vec::new(),
            }
        );
    }

    #[test]
    fn parses_leading_json_mode() {
        let mut values = arguments(&["--json", "inspect", "42"]);

        assert_eq!(parse_output_format(&mut values), OutputFormat::Json);
        assert_eq!(values, arguments(&["inspect", "42"]));
    }

    #[test]
    fn rejects_zero_pid_and_extra_arguments() {
        assert_eq!(
            parse_command(arguments(&["inspect", "0"]))
                .unwrap_err()
                .kind(),
            crate::error::ErrorKind::InvalidArguments
        );
        assert_eq!(
            parse_command(arguments(&["inspect", "42", "extra"]))
                .unwrap_err()
                .kind(),
            crate::error::ErrorKind::InvalidArguments
        );
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "target.exe",
                "darpc.dll",
                "--wait-ms",
                "10",
            ]))
            .unwrap_err()
            .kind(),
            crate::error::ErrorKind::InvalidArguments
        );
    }
}
