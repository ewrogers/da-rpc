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
            GetProcessTimes, IsWow64Process2, OpenProcess, PROCESS_CREATE_THREAD,
            PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION,
            PROCESS_VM_READ, PROCESS_VM_WRITE,
        },
    },
};

pub(crate) struct ProcessInspection {
    pub(crate) creation_time: u64,
    pub(crate) darpc_loaded: bool,
}

#[cfg(windows)]
fn open_process(pid: u32, access: u32) -> Result<OwnedHandle, String> {
    // SAFETY: `pid` has been validated as nonzero, the caller supplies
    // the desired access mask, and handle inheritance is disabled.
    let handle = unsafe { OpenProcess(access, 0, pid) };

    if handle.is_null() {
        return Err(format!(
            "failed to open process {pid}: {}",
            io::Error::last_os_error()
        ));
    }

    // SAFETY: `handle` is a non-null owned handle returned by
    // `OpenProcess`, and ownership is transferred exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

#[cfg(windows)]
pub(crate) fn open_for_injection(pid: u32) -> Result<OwnedHandle, String> {
    let access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_READ
        | PROCESS_VM_WRITE;

    open_process(pid, access)
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

#[cfg(windows)]
pub(crate) fn inspect(pid: u32) -> Result<ProcessInspection, String> {
    let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    inspect_process(pid, &process)
}

#[cfg(windows)]
pub(crate) fn inspect_process(
    pid: u32,
    process: &OwnedHandle,
) -> Result<ProcessInspection, String> {
    println!("Opened target process");

    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;

    // SAFETY: `process` references a valid process handle, and both output
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

    Ok(ProcessInspection {
        creation_time,
        darpc_loaded,
    })
}

#[cfg(not(windows))]
pub(crate) fn inspect(_pid: u32) -> Result<ProcessInspection, String> {
    Err("loader requires Windows".to_owned())
}
