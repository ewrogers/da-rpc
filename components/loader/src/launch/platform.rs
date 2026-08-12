use super::*;
use crate::{lifecycle, patch, process::TargetProcess};
use std::{
    ffi::OsStr,
    fs, io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CREATE_SUSPENDED, CreateProcessW, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
        STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    },
};

const CHILD_TERMINATION_EXIT_CODE: u32 = 1;
const CHILD_TERMINATION_TIMEOUT_MS: u32 = 5_000;
const RESUME_FAILED: u32 = u32::MAX;

struct SuspendedChild {
    process: TargetProcess,
    primary_thread: OwnedHandle,
    terminate_on_drop: bool,
}

impl SuspendedChild {
    fn create(executable: &Path, current_directory: &Path, arguments: &[OsString]) -> Result<Self> {
        let application_name = wide_nul(executable.as_os_str(), "executable path")?;
        let current_directory = wide_nul(current_directory.as_os_str(), "current directory")?;
        let mut command_line = build_command_line(executable.as_os_str(), arguments)?;
        let startup_info = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).map_err(|_| {
                LoaderError::new(
                    ErrorKind::Internal,
                    "process startup information size does not fit u32",
                )
            })?,
            dwFlags: STARTF_USESTDHANDLES,
            ..Default::default()
        };
        let mut process_information = PROCESS_INFORMATION::default();

        // SAFETY: all pointers refer to live buffers for the duration of
        // the call. The command-line buffer is writable and NUL-terminated.
        // General handle inheritance is disabled, and STARTUPINFOW
        // explicitly supplies null standard handles so Windows does not
        // copy the loader's console handles as a special case. The process
        // starts suspended, and returned handles are transferred below.
        let succeeded = unsafe {
            CreateProcessW(
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED,
                ptr::null(),
                current_directory.as_ptr(),
                &startup_info,
                &mut process_information,
            )
        };

        if succeeded == 0 {
            return Err(LoaderError::from_io(
                ErrorKind::LaunchFailed,
                format!("failed to create `{}`", executable.display()),
                io::Error::last_os_error(),
            ));
        }

        if process_information.hProcess.is_null()
            || process_information.hThread.is_null()
            || process_information.dwProcessId == 0
        {
            close_incomplete_process(&process_information);
            return Err(LoaderError::new(
                ErrorKind::Internal,
                "CreateProcessW returned incomplete process information",
            ));
        }

        // SAFETY: CreateProcessW succeeded and returned two non-null owned
        // handles. Each handle is transferred exactly once.
        let process_handle = unsafe { OwnedHandle::from_raw_handle(process_information.hProcess) };
        // SAFETY: same invariant as above, for the primary thread handle.
        let primary_thread = unsafe { OwnedHandle::from_raw_handle(process_information.hThread) };

        Ok(Self {
            process: TargetProcess::from_created(process_information.dwProcessId, process_handle),
            primary_thread,
            terminate_on_drop: true,
        })
    }

    fn pid(&self) -> u32 {
        self.process.pid()
    }

    fn process(&self) -> &TargetProcess {
        &self.process
    }

    fn resume(&mut self) -> Result<()> {
        // SAFETY: primary_thread owns the initial thread handle returned by
        // CreateProcessW, and this child has not previously been resumed.
        let previous_suspend_count = unsafe { ResumeThread(self.primary_thread.as_raw_handle()) };

        if previous_suspend_count == RESUME_FAILED {
            return Err(LoaderError::from_io(
                ErrorKind::LaunchFailed,
                format!("failed to resume process {}", self.pid()),
                io::Error::last_os_error(),
            ));
        }

        if previous_suspend_count != 1 {
            return Err(LoaderError::new(
                ErrorKind::LaunchFailed,
                format!(
                    "process {} primary thread had unexpected suspend count \
                    {previous_suspend_count}",
                    self.pid()
                ),
            ));
        }

        self.terminate_on_drop = false;
        Ok(())
    }

    fn terminate(&mut self) -> Result<()> {
        if !self.terminate_on_drop {
            return Ok(());
        }

        if self.process.is_running()? {
            // SAFETY: process owns the child process handle created by this
            // loader invocation. Termination is limited to that child.
            let succeeded = unsafe {
                TerminateProcess(
                    self.process.handle().as_raw_handle(),
                    CHILD_TERMINATION_EXIT_CODE,
                )
            };

            if succeeded == 0 && self.process.is_running()? {
                return Err(LoaderError::from_io(
                    ErrorKind::LaunchFailed,
                    format!("failed to terminate child process {}", self.pid()),
                    io::Error::last_os_error(),
                ));
            }
        }

        match wait_for_process(
            self.process.handle(),
            CHILD_TERMINATION_TIMEOUT_MS,
            self.pid(),
        )? {
            true => {
                self.terminate_on_drop = false;
                Ok(())
            }
            false => Err(LoaderError::new(
                ErrorKind::LaunchFailed,
                format!("timed out terminating child process {}", self.pid()),
            )),
        }
    }
}

impl Drop for SuspendedChild {
    fn drop(&mut self) {
        if self.terminate_on_drop {
            // SAFETY: this handle belongs to the child created by this
            // object. Drop is only a last-resort cleanup path.
            unsafe {
                TerminateProcess(
                    self.process.handle().as_raw_handle(),
                    CHILD_TERMINATION_EXIT_CODE,
                );
                WaitForSingleObject(
                    self.process.handle().as_raw_handle(),
                    CHILD_TERMINATION_TIMEOUT_MS,
                );
            }
        }
    }
}

fn close_incomplete_process(process_information: &PROCESS_INFORMATION) {
    let process = if process_information.hProcess.is_null() {
        None
    } else {
        // SAFETY: CreateProcessW succeeded and returned this non-null owned
        // handle, which is transferred exactly once for cleanup.
        Some(unsafe { OwnedHandle::from_raw_handle(process_information.hProcess) })
    };
    let _thread = if process_information.hThread.is_null() {
        None
    } else {
        // SAFETY: same invariant as above, for the primary thread handle.
        Some(unsafe { OwnedHandle::from_raw_handle(process_information.hThread) })
    };

    if let Some(process) = process {
        // SAFETY: the handle belongs to the incomplete child returned by
        // CreateProcessW. Best-effort termination prevents a leaked child.
        unsafe {
            TerminateProcess(process.as_raw_handle(), CHILD_TERMINATION_EXIT_CODE);
            WaitForSingleObject(process.as_raw_handle(), CHILD_TERMINATION_TIMEOUT_MS);
        }
    }
}

fn wait_for_process(process: &OwnedHandle, timeout_ms: u32, pid: u32) -> Result<bool> {
    // SAFETY: process owns a valid process handle and the timeout is finite.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), timeout_ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(LoaderError::from_io(
            ErrorKind::LaunchFailed,
            format!("failed while waiting for child process {pid}"),
            io::Error::last_os_error(),
        )),
        result => Err(LoaderError::new(
            ErrorKind::LaunchFailed,
            format!("unexpected wait result {result} for child process {pid}"),
        )),
    }
}

fn validate_executable(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let executable = fs::canonicalize(path).map_err(|error| {
        LoaderError::from_io(
            ErrorKind::LaunchFailed,
            format!("failed to resolve executable `{}`", path.display()),
            error,
        )
    })?;

    if !executable.is_file() {
        return Err(LoaderError::new(
            ErrorKind::LaunchFailed,
            format!("executable is not a file: `{}`", executable.display()),
        ));
    }

    let current_directory = executable.parent().ok_or_else(|| {
        LoaderError::new(
            ErrorKind::LaunchFailed,
            format!(
                "executable has no parent directory: `{}`",
                executable.display()
            ),
        )
    })?;

    Ok((executable.clone(), current_directory.to_owned()))
}

fn wide_nul(value: &OsStr, description: &str) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();

    if value.contains(&0) {
        return Err(LoaderError::new(
            ErrorKind::InvalidArguments,
            format!("{description} contains a NUL character"),
        ));
    }

    value.push(0);
    Ok(value)
}

fn build_command_line(executable: &OsStr, arguments: &[OsString]) -> Result<Vec<u16>> {
    let mut command_line = Vec::new();
    append_argument(
        &mut command_line,
        &executable.encode_wide().collect::<Vec<_>>(),
    )?;

    for argument in arguments {
        command_line.push(u16::from(b' '));
        append_argument(
            &mut command_line,
            &argument.as_os_str().encode_wide().collect::<Vec<_>>(),
        )?;
    }

    command_line.push(0);
    Ok(command_line)
}

pub(super) fn run(
    executable_path: &Path,
    arguments: &[OsString],
    dll: &DarpcDll,
    patches: LaunchPatches,
    apply_default_patches: bool,
) -> Result<LaunchOutcome> {
    let (executable, current_directory) = validate_executable(executable_path)?;
    let mut child = SuspendedChild::create(&executable, &current_directory, arguments)?;
    let pid = child.pid();

    if let Err(error) = patch::apply(child.process(), patches, apply_default_patches) {
        return Err(cleanup_launch_error(&mut child, error));
    }

    let outcome = match lifecycle::attach_created(child.process(), dll) {
        Ok(outcome) => outcome,
        Err(error) => return Err(cleanup_launch_error(&mut child, error)),
    };

    if let Err(error) = child.resume() {
        return Err(cleanup_launch_error(&mut child, error));
    }

    eprintln!("Resumed child process {pid}");
    Ok(LaunchOutcome {
        pid,
        inspection: outcome.inspection,
        changed: outcome.changed,
    })
}

fn cleanup_launch_error(child: &mut SuspendedChild, error: LoaderError) -> LoaderError {
    let pid = child.pid();

    match child.terminate() {
        Ok(()) => error.with_pid(pid),
        Err(cleanup_error) => LoaderError::new(
            error.kind(),
            format!("{error}; child process {pid} cleanup failed: {cleanup_error}"),
        )
        .with_pid(pid),
    }
}

#[cfg(test)]
mod tests {
    use super::{SuspendedChild, wait_for_process};
    use std::{
        ffi::OsString,
        fs,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr,
        time::{SystemTime, UNIX_EPOCH},
    };
    use windows_sys::Win32::{
        Foundation::WAIT_TIMEOUT,
        Security::SECURITY_ATTRIBUTES,
        System::Threading::{CreateEventW, GetExitCodeProcess, WaitForSingleObject},
    };

    #[test]
    fn process_creation_does_not_inherit_handles() {
        let security_attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>())
                .expect("security attributes size should fit u32"),
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };

        // SAFETY: security_attributes is live and explicitly marks the
        // returned unnamed event handle inheritable.
        let event = unsafe { CreateEventW(&security_attributes, 1, 0, ptr::null()) };
        assert!(!event.is_null(), "failed to create inheritable event");

        // SAFETY: CreateEventW returned a non-null owned handle, which is
        // transferred exactly once.
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let test_directory = std::env::temp_dir().join(format!(
            "darpc-launch-handle-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&test_directory).expect("failed to create handle test directory");
        fs::write(
            test_directory.join("handle.txt"),
            (event.as_raw_handle() as usize).to_string(),
        )
        .expect("failed to write inherited handle probe");

        let executable = std::env::current_exe().expect("failed to locate test executable");
        let arguments = [
            OsString::from("--ignored"),
            OsString::from("--exact"),
            OsString::from("launch::platform::tests::inheritable_handle_is_unavailable_in_child"),
        ];
        let mut child = SuspendedChild::create(&executable, &test_directory, &arguments)
            .expect("failed to create suspended handle probe");
        let pid = child.pid();
        child.resume().expect("failed to resume handle probe");

        assert!(
            wait_for_process(child.process().handle(), 10_000, pid)
                .expect("failed to wait for handle probe"),
            "handle probe timed out"
        );

        let mut exit_code = u32::MAX;
        // SAFETY: child owns a valid process handle and exit_code is
        // writable. The wait above established that the child exited.
        let succeeded =
            unsafe { GetExitCodeProcess(child.process().handle().as_raw_handle(), &mut exit_code) };
        assert_ne!(succeeded, 0, "failed to read handle probe exit code");
        assert_eq!(exit_code, 0, "inheritable handle was visible in child");
        // SAFETY: event owns a live waitable handle and a zero timeout does
        // not block. A signaled event proves the child received this same
        // kernel object, rather than merely reusing its numeric value.
        assert_eq!(
            unsafe { WaitForSingleObject(event.as_raw_handle(), 0) },
            WAIT_TIMEOUT,
            "inheritable event reached the child"
        );

        fs::remove_file(test_directory.join("handle.txt")).expect("failed to remove handle probe");
        fs::remove_dir(test_directory).expect("failed to remove handle test directory");
    }

    #[test]
    #[ignore = "runs only as a child of process_creation_does_not_inherit_handles"]
    fn inheritable_handle_is_unavailable_in_child() {
        use windows_sys::Win32::Foundation::GetHandleInformation;
        use windows_sys::Win32::System::Threading::SetEvent;

        let handle = fs::read_to_string("handle.txt")
            .expect("failed to read handle probe")
            .parse::<usize>()
            .expect("handle probe was not a usize") as *mut core::ffi::c_void;
        let mut flags = 0;

        // SAFETY: handle is treated only as an opaque candidate value and
        // flags is writable. Failure is the expected result.
        let succeeded = unsafe { GetHandleInformation(handle, &mut flags) };
        if succeeded != 0 {
            // SAFETY: a successful handle query established a live child
            // handle. If it is the inherited event, signaling it is
            // observable through the parent's handle.
            unsafe { SetEvent(handle) };
        }
    }
}
