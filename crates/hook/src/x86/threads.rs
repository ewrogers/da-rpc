use super::{CodeRange, DetourError};
use std::{io, mem};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
    },
    System::{
        Diagnostics::{
            Debug::{CONTEXT, CONTEXT_CONTROL_X86, GetThreadContext},
            ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
        },
        Threading::{
            GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
            THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

const MAX_SUSPENDED_THREADS: usize = 256;

pub(super) struct SuspendedThreads {
    threads: Vec<SuspendedThread>,
}

impl SuspendedThreads {
    pub(super) fn capture() -> Result<Self, DetourError> {
        let mut suspended = Self {
            threads: Vec::with_capacity(MAX_SUSPENDED_THREADS),
        };
        let mut thread_ids = Vec::with_capacity(MAX_SUSPENDED_THREADS);
        let process_id =
            // SAFETY: GetCurrentProcessId has no preconditions.
            unsafe { GetCurrentProcessId() };
        let current_thread_id =
            // SAFETY: GetCurrentThreadId has no preconditions.
            unsafe { GetCurrentThreadId() };

        loop {
            enumerate_thread_ids(process_id, &mut thread_ids)?;
            let mut added = false;

            for &thread_id in &thread_ids {
                if thread_id == current_thread_id
                    || suspended
                        .threads
                        .iter()
                        .any(|thread| thread.id == thread_id)
                {
                    continue;
                }
                if suspended.threads.len() == MAX_SUSPENDED_THREADS {
                    return Err(DetourError::TooManyThreads {
                        limit: MAX_SUSPENDED_THREADS,
                    });
                }
                if let Some(thread) = SuspendedThread::open(thread_id)? {
                    suspended.threads.push(thread);
                    added = true;
                }
            }

            if !added {
                return Ok(suspended);
            }
        }
    }

    pub(super) fn reject_instruction_pointers(
        &self,
        ranges: &[CodeRange],
    ) -> Result<(), DetourError> {
        for thread in &self.threads {
            let instruction_pointer = thread.instruction_pointer()?;
            if ranges
                .iter()
                .any(|range| range.contains(instruction_pointer))
            {
                return Err(DetourError::BusyInstructionPointer {
                    thread_id: thread.id,
                    instruction_pointer,
                });
            }
        }
        Ok(())
    }

    pub(super) fn resume(mut self) {
        for thread in self.threads.iter_mut().rev() {
            thread.resume();
        }
    }
}

struct SuspendedThread {
    id: u32,
    handle: HANDLE,
    suspended: bool,
}

impl SuspendedThread {
    fn open(id: u32) -> Result<Option<Self>, DetourError> {
        // SAFETY: id came from the system thread snapshot and the requested
        // access rights are the minimum needed for enlistment and inspection.
        let handle = unsafe {
            OpenThread(
                THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION | THREAD_SUSPEND_RESUME,
                0,
                id,
            )
        };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(None);
            }
            return Err(DetourError::Windows {
                operation: "OpenThread for detour enlistment failed",
                source: error,
            });
        }

        // SAFETY: handle grants THREAD_SUSPEND_RESUME and remains owned below.
        if unsafe { SuspendThread(handle) } == u32::MAX {
            let error = io::Error::last_os_error();
            // SAFETY: handle was returned by OpenThread and has not been closed.
            unsafe {
                CloseHandle(handle);
            }
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(None);
            }
            return Err(DetourError::Windows {
                operation: "SuspendThread for detour enlistment failed",
                source: error,
            });
        }

        Ok(Some(Self {
            id,
            handle,
            suspended: true,
        }))
    }

    fn instruction_pointer(&self) -> Result<usize, DetourError> {
        // SAFETY: an all-zero x86 CONTEXT is valid once ContextFlags is set.
        let mut context: CONTEXT = unsafe { mem::zeroed() };
        context.ContextFlags = CONTEXT_CONTROL_X86;
        // SAFETY: this object owns a suspended thread handle with
        // THREAD_GET_CONTEXT access and context points to writable storage.
        if unsafe { GetThreadContext(self.handle, &mut context) } == 0 {
            return Err(DetourError::windows(
                "GetThreadContext for detour enlistment failed",
            ));
        }
        Ok(context.Eip as usize)
    }

    fn resume(&mut self) {
        if !self.suspended {
            return;
        }

        for _ in 0..3 {
            // SAFETY: this object owns one successful suspension and the live
            // thread handle grants THREAD_SUSPEND_RESUME.
            if unsafe { ResumeThread(self.handle) } != u32::MAX {
                self.suspended = false;
                return;
            }
        }
    }
}

impl Drop for SuspendedThread {
    fn drop(&mut self) {
        self.resume();
        // SAFETY: this object owns the matching live thread handle. Explicit
        // resume is attempted before handle closure, including on error paths.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn enumerate_thread_ids(process_id: u32, output: &mut Vec<u32>) -> Result<(), DetourError> {
    output.clear();
    // SAFETY: TH32CS_SNAPTHREAD ignores the process identifier and returns an
    // owned snapshot handle on success.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(DetourError::windows(
            "CreateToolhelp32Snapshot for detour enlistment failed",
        ));
    }
    let snapshot = Snapshot(snapshot);

    let mut entry = THREADENTRY32 {
        dwSize: mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: snapshot is live and entry has the required size initialized.
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        return Err(DetourError::windows(
            "Thread32First for detour enlistment failed",
        ));
    }

    loop {
        if entry.th32OwnerProcessID == process_id {
            if output.len() == MAX_SUSPENDED_THREADS {
                return Err(DetourError::TooManyThreads {
                    limit: MAX_SUSPENDED_THREADS,
                });
            }
            output.push(entry.th32ThreadID);
        }

        entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
        // SAFETY: snapshot and entry remain valid for continued enumeration.
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(());
            }
            return Err(DetourError::Windows {
                operation: "Thread32Next for detour enlistment failed",
                source: error,
            });
        }
    }
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: this object owns the live snapshot handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
