//! daRPC command-line client.

mod args;
mod command_output;
mod error;
#[cfg(windows)]
mod ipc;
mod object_output;
mod output;
mod snapshot_output;

#[cfg(windows)]
use args::Operation;
use args::{Command, parse_command, parse_output_format};
use error::Result;
#[cfg(not(windows))]
use error::{ClientError, ErrorKind};
use output::{CommandResult, OutputFormat, render_error};
use std::{env, ffi::OsString, process::ExitCode};

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
