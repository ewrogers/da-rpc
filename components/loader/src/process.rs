use crate::error::{ErrorKind, LoaderError, Result};
use std::path::PathBuf;

#[cfg(windows)]
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{
        ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            TH32CS_SNAPMODULE,
        },
        SystemInformation::{IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN},
        Threading::{
            GetExitCodeProcess, GetProcessTimes, IsWow64Process2, OpenProcess,
            PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
        },
    },
};

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessModule {
    pub(crate) base: usize,
    pub(crate) path: PathBuf,
}

pub(crate) struct ProcessInspection {
    pub(crate) creation_time: u64,
    pub(crate) darpc_module: Option<ProcessModule>,
}

#[cfg(windows)]
pub(crate) struct TargetProcess {
    pid: u32,
    handle: OwnedHandle,
}

#[cfg(windows)]
fn open_process(pid: u32, access: u32) -> Result<OwnedHandle> {
    // SAFETY: `pid` has been validated as nonzero, the caller supplies
    // the desired access mask, and handle inheritance is disabled.
    let handle = unsafe { OpenProcess(access, 0, pid) };

    if handle.is_null() {
        let error = io::Error::last_os_error();
        let kind = if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            ErrorKind::ProcessMissing
        } else {
            ErrorKind::RemoteOperationFailed
        };

        return Err(LoaderError::from_io(
            kind,
            format!("failed to open process {pid}"),
            error,
        ));
    }

    // SAFETY: `handle` is a non-null owned handle returned by
    // `OpenProcess`, and ownership is transferred exactly once.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    ensure_running(pid, &handle)?;
    Ok(handle)
}

#[cfg(windows)]
fn is_running(pid: u32, process: &impl AsRawHandle) -> Result<bool> {
    let mut exit_code = 0;

    // SAFETY: `process` references a valid process handle and `exit_code`
    // is a live, writable `u32`.
    let succeeded = unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit_code) };

    if succeeded == 0 {
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to query process {pid} state"),
            io::Error::last_os_error(),
        ));
    }

    Ok(exit_code == STILL_ACTIVE as u32)
}

#[cfg(windows)]
fn ensure_running(pid: u32, process: &impl AsRawHandle) -> Result<()> {
    if is_running(pid, process)? {
        Ok(())
    } else {
        Err(LoaderError::new(
            ErrorKind::ProcessExited,
            format!("process {pid} has exited"),
        ))
    }
}

#[cfg(windows)]
fn find_module(
    pid: u32,
    process: &OwnedHandle,
    expected_name: &str,
) -> Result<Option<ProcessModule>> {
    ensure_running(pid, process)?;

    // SAFETY: the validated PID identifies the target process, and the
    // requested snapshot contains read-only module information.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) };

    if snapshot == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();

        if !is_running(pid, process)?
            || error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
        {
            return Err(LoaderError::new(
                ErrorKind::ProcessExited,
                format!("process {pid} exited while capturing its modules"),
            ));
        }

        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to capture modules for process {pid}"),
            error,
        ));
    }

    // SAFETY: `snapshot` is a valid owned snapshot handle, and ownership
    // is transferred exactly once.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };

    let mut module = MODULEENTRY32W {
        dwSize: u32::try_from(size_of::<MODULEENTRY32W>()).map_err(|_| {
            LoaderError::new(ErrorKind::Internal, "module entry size does not fit in u32")
        })?,
        ..Default::default()
    };

    // SAFETY: `snapshot` owns a valid module snapshot handle. `module`
    // is writable, and `dwSize` describes its complete initialized size.
    let found = unsafe { Module32FirstW(snapshot.as_raw_handle(), &mut module) };

    if found == 0 {
        let error = io::Error::last_os_error();
        ensure_running(pid, process)?;
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to read modules for process {pid}"),
            error,
        ));
    }

    loop {
        let module_name = decode_wide(&module.szModule);

        if module_name.eq_ignore_ascii_case(expected_name) {
            return Ok(Some(ProcessModule {
                base: module.modBaseAddr as usize,
                path: PathBuf::from(decode_wide(&module.szExePath)),
            }));
        }

        // SAFETY: the snapshot and writable entry remain valid.
        let found = unsafe { Module32NextW(snapshot.as_raw_handle(), &mut module) };

        if found != 0 {
            continue;
        }

        let error = io::Error::last_os_error();

        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            return Ok(None);
        }

        ensure_running(pid, process)?;
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed while reading modules for process {pid}"),
            error,
        ));
    }
}

#[cfg(windows)]
fn decode_wide(value: &[u16]) -> String {
    let value_length = value
        .iter()
        .position(|&code_unit| code_unit == 0)
        .unwrap_or(value.len());

    String::from_utf16_lossy(&value[..value_length])
}

#[cfg(windows)]
pub(crate) fn inspect(pid: u32) -> Result<ProcessInspection> {
    let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    inspect_handle(pid, &process)
}

#[cfg(windows)]
fn inspect_handle(pid: u32, process: &OwnedHandle) -> Result<ProcessInspection> {
    ensure_running(pid, process)?;
    eprintln!("Opened target process");

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
        let error = io::Error::last_os_error();
        ensure_running(pid, process)?;
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to inspect process {pid} architecture"),
            error,
        ));
    }

    let target_machine = if process_machine == IMAGE_FILE_MACHINE_UNKNOWN {
        native_machine
    } else {
        process_machine
    };

    if target_machine != IMAGE_FILE_MACHINE_I386 {
        return Err(LoaderError::new(
            ErrorKind::WrongArchitecture,
            format!("process {pid} is not x86: machine=0x{target_machine:04X}"),
        ));
    }

    eprintln!("Target architecture: x86");

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
        let error = io::Error::last_os_error();
        ensure_running(pid, process)?;
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to inspect process {pid} creation time"),
            error,
        ));
    }

    let creation_time =
        (u64::from(creation_time.dwHighDateTime) << 32) | u64::from(creation_time.dwLowDateTime);

    eprintln!("Process identity: pid={pid} creation_time={creation_time}");

    let darpc_module = find_module(pid, process, "darpc.dll")?;

    eprintln!(
        "daRPC module: {}",
        if darpc_module.is_some() {
            "loaded"
        } else {
            "not loaded"
        }
    );

    Ok(ProcessInspection {
        creation_time,
        darpc_module,
    })
}

#[cfg(windows)]
impl TargetProcess {
    pub(crate) fn open(pid: u32) -> Result<Self> {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_READ
            | PROCESS_VM_WRITE;

        Ok(Self {
            pid,
            handle: open_process(pid, access)?,
        })
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn handle(&self) -> &OwnedHandle {
        &self.handle
    }

    pub(crate) fn module_base(&self, expected_name: &str) -> Result<Option<usize>> {
        Ok(self
            .module(expected_name)?
            .map(|process_module| process_module.base))
    }

    pub(crate) fn module(&self, expected_name: &str) -> Result<Option<ProcessModule>> {
        find_module(self.pid, &self.handle, expected_name)
    }

    pub(crate) fn inspect(&self) -> Result<ProcessInspection> {
        inspect_handle(self.pid, &self.handle)
    }
}

#[cfg(not(windows))]
pub(crate) struct TargetProcess;

#[cfg(not(windows))]
impl TargetProcess {
    pub(crate) fn open(_pid: u32) -> Result<Self> {
        Err(LoaderError::new(
            ErrorKind::UnsupportedPlatform,
            "loader requires Windows",
        ))
    }
}

#[cfg(not(windows))]
pub(crate) fn inspect(_pid: u32) -> Result<ProcessInspection> {
    Err(LoaderError::new(
        ErrorKind::UnsupportedPlatform,
        "loader requires Windows",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::ensure_running;
    use crate::error::ErrorKind;
    use std::process::Command;

    #[test]
    fn terminated_process_has_an_exited_result() {
        let mut child = Command::new("cmd.exe")
            .args(["/C", "exit", "0"])
            .spawn()
            .expect("failed to start short-lived child process");
        let pid = child.id();
        child
            .wait()
            .expect("failed to wait for short-lived child process");

        let error = ensure_running(pid, &child).expect_err("terminated process reported running");

        assert_eq!(error.kind(), ErrorKind::ProcessExited);
    }
}
