//! daRPC client launcher and injector.

mod inject;
mod pe;
mod process;
mod remote;
mod remote_dll;

use pe::DarpcDll;
use process::{TargetProcess, inspect};

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

const USAGE: &str = "\
usage:
    loader inspect <pid>
    loader attach <pid> <dll-path>";

enum Command {
    Inspect { pid: u32 },
    Attach { pid: u32, dll_path: PathBuf },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("loader: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    match parse_command()? {
        Command::Inspect { pid } => {
            inspect(pid)?;
            Ok(())
        }
        Command::Attach { pid, dll_path } => attach(pid, dll_path),
    }
}

fn parse_command() -> Result<Command, String> {
    let mut arguments = env::args_os().skip(1);

    let command = arguments.next().ok_or_else(|| USAGE.to_owned())?;

    let command = match command.to_str() {
        Some("inspect") => Command::Inspect {
            pid: parse_pid(arguments.next())?,
        },
        Some("attach") => Command::Attach {
            pid: parse_pid(arguments.next())?,
            dll_path: arguments
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| USAGE.to_owned())?,
        },
        Some(command) => return Err(format!("unknown command: `{command}`\n{USAGE}")),
        None => {
            return Err(format!("command must be valid Unicode\n{USAGE}"));
        }
    };

    if arguments.next().is_some() {
        return Err(format!("too many arguments\n{USAGE}"));
    }

    Ok(command)
}

fn parse_pid(argument: Option<OsString>) -> Result<u32, String> {
    let argument = argument.ok_or_else(|| USAGE.to_owned())?;

    let argument = argument
        .to_str()
        .ok_or_else(|| "PID must be valid Unicode".to_owned())?;

    let pid = argument
        .parse::<u32>()
        .map_err(|_| "PID must be an unsigned 32-bit integer".to_owned())?;

    if pid == 0 {
        return Err("PID must be greater than zero".to_owned());
    }

    Ok(pid)
}

fn attach(pid: u32, dll_path: PathBuf) -> Result<(), String> {
    let dll = DarpcDll::validate(dll_path)?;

    println!(
        "Validated x86 DLL: {} initialize_rva=0x{:08X} shutdown_rva=0x{:08X}",
        dll.path.display(),
        dll.initialize_rva,
        dll.shutdown_rva
    );

    let process = TargetProcess::open(pid)?;
    inject::attach(&process, &dll)
}
