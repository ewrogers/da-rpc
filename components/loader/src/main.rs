//! daRPC client launcher and injector.

use std::{env, process::ExitCode};

#[cfg(windows)]
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

#[cfg(windows)]
use windows_sys::Win32::System::{
    SystemInformation::{IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN},
    Threading::{IsWow64Process2, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
};

const USAGE: &str = "usage: loader inspect <pid>";

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
    let mut arguments = env::args().skip(1);

    let command = arguments.next().ok_or_else(|| USAGE.to_owned())?;

    if command != "inspect" {
        return Err(format!("unknown command `{command}`\n{USAGE}"));
    }

    let pid = arguments
        .next()
        .ok_or_else(|| USAGE.to_owned())?
        .parse::<u32>()
        .map_err(|_| "PID must be an unsigned 32-bit integer".to_owned())?;

    if arguments.next().is_some() {
        return Err(format!("too many arguments\n{USAGE}"));
    }

    if pid == 0 {
        return Err("PID must be greater than zero".to_owned());
    }

    inspect(pid)
}

#[cfg(windows)]
fn inspect(pid: u32) -> Result<(), String> {
    // SAFETY: `OpenProcess` accepts any `u32` process ID. Access is
    // query-only, and handle inheritance is disabled.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

    if handle.is_null() {
        return Err(format!(
            "failed to open process {pid}: {}",
            io::Error::last_os_error()
        ));
    }

    // SAFETY: `handle` is a non-null owned handle returned by
    // `OpenProcess`, and ownership is transferred exactly once.
    let process = unsafe { OwnedHandle::from_raw_handle(handle) };

    println!("Opened target process");

    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;

    // SAFETY: `process` owns a valid process handle, and both output
    // pointers refer to live, writable `u16` values.
    let succeeded = unsafe {
        IsWow64Process2(
            process.as_raw_handle(),
            &mut process_machine,
            &mut native_machine,
        )
    };

    if succeeded == 0 {
        return Err(format!(
            "failed to inspect process {pid} architecture: {}",
            io::Error::last_os_error()
        ));
    }

    let target_machine = if process_machine == IMAGE_FILE_MACHINE_UNKNOWN {
        native_machine
    } else {
        process_machine
    };

    if target_machine != IMAGE_FILE_MACHINE_I386 {
        return Err(format!(
            "process {pid} is not x86: machine=0x{target_machine:04X}"
        ));
    }

    println!("Target architecture: x86");

    Ok(())
}

#[cfg(not(windows))]
fn inspect(_pid: u32) -> Result<(), String> {
    Err("loader requires Windows".to_owned())
}
