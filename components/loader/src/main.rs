//! daRPC client launcher and injector.

mod dll;
mod endpoint;
mod error;
mod launch;
mod lifecycle;
mod output;
mod patch;
mod pe;
mod process;
mod remote;

#[cfg(debug_assertions)]
use darpc_game_client::DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE;
use darpc_game_client::{CLIENT_VERSION, ClientExecutable, executable_sha256};
use darpc_win32::lifecycle::InitializeOptions;
use endpoint::ServerEndpoint;
use error::{ErrorKind, LoaderError, Result};
use output::{CommandResult, OutputFormat, render_error};
use patch::LaunchPatches;
use pe::DarpcDll;
use process::{ProcessInspection, TargetProcess, inspect};

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

#[cfg(debug_assertions)]
use std::fs;

struct ValidatedClient {
    path: PathBuf,
    apply_default_patches: bool,
}

const USAGE: &str = "\
usage:
    loader [--json] inspect <pid>
    loader [--json] attach [--diagnostics hook-timing] <pid> <dll-path>
    loader [--json] detach <pid> <dll-path>
    loader [--json] launch [--allow-multiple] [--diagnostics hook-timing] [--server <host[:port]>] \
        [--skip-intro] [--skip-notice] [--skip-exchange-alerts] \
        <executable-path> <dll-path> [-- <argument>...]";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Inspect {
        pid: u32,
    },
    Attach {
        pid: u32,
        dll_path: PathBuf,
        initialize_options: InitializeOptions,
    },
    Detach {
        pid: u32,
        dll_path: PathBuf,
    },
    Launch {
        executable_path: PathBuf,
        dll_path: PathBuf,
        arguments: Vec<OsString>,
        patches: LaunchPatches,
        server: Option<ServerEndpoint>,
        initialize_options: InitializeOptions,
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
        Some("attach") => return parse_attach(arguments),
        Some("detach") => Command::Detach {
            pid: parse_pid(arguments.next())?,
            dll_path: parse_dll_path(arguments.next())?,
        },
        Some("launch") => return parse_launch(arguments),
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

fn parse_attach(mut arguments: impl Iterator<Item = OsString>) -> Result<Command> {
    let mut initialize_options = InitializeOptions::new();
    let first = arguments.next().ok_or_else(|| invalid_arguments(USAGE))?;
    let pid = if first == "--diagnostics" {
        initialize_options = parse_diagnostics(arguments.next())?;
        parse_pid(arguments.next())?
    } else {
        parse_pid(Some(first))?
    };
    let dll_path = parse_dll_path(arguments.next())?;
    if arguments.next().is_some() {
        return Err(invalid_arguments(format!("too many arguments\n{USAGE}")));
    }
    Ok(Command::Attach {
        pid,
        dll_path,
        initialize_options,
    })
}

fn parse_diagnostics(argument: Option<OsString>) -> Result<InitializeOptions> {
    match argument.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("hook-timing") => Ok(InitializeOptions::new().with_hook_timing(true)),
        Some(value) => Err(invalid_arguments(format!(
            "unsupported diagnostics mode: `{value}`"
        ))),
        None => Err(invalid_arguments(
            "diagnostics option requires `hook-timing`",
        )),
    }
}

fn parse_launch(mut arguments: impl Iterator<Item = OsString>) -> Result<Command> {
    let mut patches = LaunchPatches::default();
    let mut server = None;
    let mut initialize_options = InitializeOptions::new();
    let mut executable_path = None;

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--allow-multiple") => patches.allow_multiple = true,
            Some("--diagnostics") => {
                if initialize_options.hook_timing() {
                    return Err(invalid_arguments(
                        "diagnostics option may be specified only once",
                    ));
                }
                initialize_options = parse_diagnostics(arguments.next())?;
            }
            Some("--server") => {
                if server.is_some() {
                    return Err(invalid_arguments(
                        "server option may be specified only once",
                    ));
                }

                let value = arguments
                    .next()
                    .ok_or_else(|| invalid_arguments("server option requires a value"))?;
                server = Some(ServerEndpoint::parse(&value)?);
                patches.command_line_endpoint = true;
            }
            Some("--skip-intro") => patches.skip_intro = true,
            Some("--skip-notice") => patches.skip_notice = true,
            Some("--skip-exchange-alerts") => patches.skip_exchange_alerts = true,
            Some(option) if option.starts_with("--") => {
                return Err(invalid_arguments(format!(
                    "unknown launch option: `{option}`\n{USAGE}"
                )));
            }
            _ => {
                executable_path = Some(PathBuf::from(argument));
                break;
            }
        }
    }

    let executable_path = executable_path.ok_or_else(|| invalid_arguments(USAGE))?;
    let dll_path = parse_dll_path(arguments.next())?;
    let Some(separator) = arguments.next() else {
        return Ok(Command::Launch {
            executable_path,
            dll_path,
            arguments: Vec::new(),
            patches,
            server,
            initialize_options,
        });
    };

    if separator != "--" {
        return Err(invalid_arguments(format!(
            "launch arguments must follow `--`\n{USAGE}"
        )));
    }

    Ok(Command::Launch {
        executable_path,
        dll_path,
        arguments: arguments.collect(),
        patches,
        server,
        initialize_options,
    })
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
        Command::Attach {
            pid,
            dll_path,
            initialize_options,
        } => {
            let dll = validate_dll(dll_path)?;
            let process = TargetProcess::open(pid)?;
            let _ = validate_client(process.executable_path()?)?;
            let outcome = lifecycle::attach(&process, &dll, initialize_options)?;

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
            let outcome = lifecycle::detach(&process, &dll)?;

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
            patches,
            server,
            initialize_options,
        } => {
            let dll = validate_dll(dll_path)?;
            let client = validate_client(executable_path)?;
            let arguments = match server {
                Some(server) => server.prepend_to(&client.path, arguments)?,
                None => arguments,
            };
            let outcome = launch::launch(
                &client.path,
                &arguments,
                &dll,
                patches,
                client.apply_default_patches,
                initialize_options,
            )?;

            Ok(command_result(
                "launch",
                outcome.pid,
                outcome.inspection,
                outcome.changed,
            ))
        }
    }
}

fn validate_client(executable_path: PathBuf) -> Result<ValidatedClient> {
    #[cfg(debug_assertions)]
    if env::var_os(DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE).as_deref()
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
        return Ok(ValidatedClient {
            path: executable_path,
            apply_default_patches: false,
        });
    }

    let executable = ClientExecutable::validate(&executable_path)
        .map_err(|error| LoaderError::new(ErrorKind::UnsupportedClient, error))?;

    eprintln!(
        "Validated Dark Ages client {CLIENT_VERSION}: {} sha256={}",
        executable.path().display(),
        executable_sha256()
    );

    Ok(ValidatedClient {
        path: executable.path().to_owned(),
        apply_default_patches: true,
    })
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
    use crate::{endpoint::ServerEndpoint, patch::LaunchPatches};
    use darpc_win32::lifecycle::InitializeOptions;
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
                initialize_options: InitializeOptions::new(),
            }
        );
        assert_eq!(
            parse_command(arguments(&[
                "attach",
                "--diagnostics",
                "hook-timing",
                "42",
                "darpc.dll",
            ]))
            .unwrap(),
            Command::Attach {
                pid: 42,
                dll_path: PathBuf::from("darpc.dll"),
                initialize_options: InitializeOptions::new().with_hook_timing(true),
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
                patches: LaunchPatches::default(),
                server: None,
                initialize_options: InitializeOptions::new(),
            }
        );

        assert_eq!(
            parse_command(arguments(&["launch", "target.exe", "darpc.dll"])).unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: Vec::new(),
                patches: LaunchPatches::default(),
                server: None,
                initialize_options: InitializeOptions::new(),
            }
        );
    }

    #[test]
    fn parses_launch_patch_options() {
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "--diagnostics",
                "hook-timing",
                "target.exe",
                "darpc.dll",
            ]))
            .unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: Vec::new(),
                patches: LaunchPatches::default(),
                server: None,
                initialize_options: InitializeOptions::new().with_hook_timing(true),
            }
        );
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "--allow-multiple",
                "--skip-intro",
                "--skip-notice",
                "--skip-exchange-alerts",
                "target.exe",
                "darpc.dll",
            ]))
            .unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: Vec::new(),
                patches: LaunchPatches {
                    allow_multiple: true,
                    command_line_endpoint: false,
                    skip_exchange_alerts: true,
                    skip_intro: true,
                    skip_notice: true,
                },
                server: None,
                initialize_options: InitializeOptions::new(),
            }
        );

        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "target.exe",
                "darpc.dll",
                "--",
                "--skip-intro",
            ]))
            .unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: arguments(&["--skip-intro"]),
                patches: LaunchPatches::default(),
                server: None,
                initialize_options: InitializeOptions::new(),
            }
        );
    }

    #[test]
    fn parses_server_option() {
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "--server",
                "da0.kru.com",
                "target.exe",
                "darpc.dll",
            ]))
            .unwrap(),
            Command::Launch {
                executable_path: PathBuf::from("target.exe"),
                dll_path: PathBuf::from("darpc.dll"),
                arguments: Vec::new(),
                patches: LaunchPatches {
                    command_line_endpoint: true,
                    ..Default::default()
                },
                server: Some(ServerEndpoint::parse("da0.kru.com".as_ref()).unwrap()),
                initialize_options: InitializeOptions::new(),
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
        assert_eq!(
            parse_command(arguments(&[
                "launch",
                "--unknown",
                "target.exe",
                "darpc.dll",
            ]))
            .unwrap_err()
            .kind(),
            crate::error::ErrorKind::InvalidArguments
        );
    }
}
