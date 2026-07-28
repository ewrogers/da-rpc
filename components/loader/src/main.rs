//! daRPC client launcher and injector.

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

#[cfg(windows)]
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            TH32CS_SNAPMODULE,
        },
        SystemInformation::{IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN},
        Threading::{
            GetProcessTimes, IsWow64Process2, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

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
        Command::Inspect { pid } => inspect(pid),
        Command::Attach { pid, dll_path } => Err(format!(
            "attach is not implemented yet: pid={pid}, dll={}",
            dll_path.display()
        )),
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

    let mut creation_time = FILETIME::default();
    let mut exit_time = FILETIME::default();
    let mut kernel_time = FILETIME::default();
    let mut user_time = FILETIME::default();

    // SAFETY: `process` owns a valid process handle, and all four output
    // pointers refer to live, writable `FILETIME` values.
    let succeeded = unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation_time,
            &mut exit_time,
            &mut kernel_time,
            &mut user_time,
        )
    };

    if succeeded == 0 {
        return Err(format!(
            "failed to inspect process {pid} creation time: {}",
            io::Error::last_os_error()
        ));
    }

    let creation_time =
        (u64::from(creation_time.dwHighDateTime) << 32) | u64::from(creation_time.dwLowDateTime);

    println!("Process identity: pid={pid} creation_time={creation_time}");

    let darpc_loaded = is_module_loaded(pid, "darpc.dll")?;

    println!(
        "daRPC module: {}",
        if darpc_loaded { "loaded" } else { "not loaded" }
    );

    Ok(())
}

#[cfg(windows)]
fn is_module_loaded(pid: u32, expected_name: &str) -> Result<bool, String> {
    // SAFETY: the validated PID identifies the target process, and the
    // requested snapshot contains read-only module information.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) };

    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "failed to capture modules for process {pid}: {}",
            io::Error::last_os_error()
        ));
    }

    // SAFETY: `snapshot` is a valid owned snapshot handle, and ownership
    // is transferred exactly once.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };

    let mut module = MODULEENTRY32W::default();
    module.dwSize = u32::try_from(size_of::<MODULEENTRY32W>())
        .map_err(|_| "module entry size does not fit in u32".to_owned())?;

    // SAFETY: `snapshot` owns a valid module snapshot handle. `module`
    // is writable, and `dwSize` describes its complete initialized size.
    let found = unsafe { Module32FirstW(snapshot.as_raw_handle(), &mut module) };

    if found == 0 {
        return Err(format!(
            "failed to read modules for process {pid}: {}",
            io::Error::last_os_error()
        ));
    }

    loop {
        let module_name = decode_module_name(&module);

        if module_name.eq_ignore_ascii_case(expected_name) {
            return Ok(true);
        }

        // SAFETY: the snapshot and writable entry remain valid.
        let found = unsafe { Module32NextW(snapshot.as_raw_handle(), &mut module) };

        if found != 0 {
            continue;
        }

        let error = io::Error::last_os_error();

        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            return Ok(false);
        }

        return Err(format!(
            "failed while reading modules for process {pid}: {error}",
        ));
    }
}

#[cfg(windows)]
fn decode_module_name(module: &MODULEENTRY32W) -> String {
    let name_length = module
        .szModule
        .iter()
        .position(|&code_unit| code_unit == 0)
        .unwrap_or(module.szModule.len());

    String::from_utf16_lossy(&module.szModule[..name_length])
}

#[cfg(not(windows))]
fn inspect(_pid: u32) -> Result<(), String> {
    Err("loader requires Windows".to_owned())
}
