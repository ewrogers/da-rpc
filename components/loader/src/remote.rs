#[cfg(windows)]
use std::{
    ffi::c_void,
    io,
    mem::{size_of, transmute},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::{null, null_mut},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0},
    System::{
        Diagnostics::Debug::WriteProcessMemory,
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{CreateRemoteThread, GetExitCodeThread, INFINITE, WaitForSingleObject},
    },
};

#[cfg(windows)]
use crate::process::TargetProcess;

#[cfg(windows)]
type ThreadStart = unsafe extern "system" fn(*mut c_void) -> u32;

#[cfg(windows)]
pub(crate) struct RemoteAllocation<'a> {
    process: &'a TargetProcess,
    address: *mut c_void,
    size: usize,
}

#[cfg(windows)]
impl<'a> RemoteAllocation<'a> {
    pub(crate) fn new(process: &'a TargetProcess, size: usize) -> Result<Self, String> {
        if size == 0 {
            return Err("remote allocation size must be nonzero".to_owned());
        }

        // SAFETY: `process` references a valid handle with VM operation
        // access. Windows chooses the address, and `size` is nonzero.
        let address = unsafe {
            VirtualAllocEx(
                process.handle().as_raw_handle(),
                null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };

        if address.is_null() {
            return Err(format!(
                "failed to allocate target memory: {}",
                io::Error::last_os_error()
            ));
        }

        Ok(Self {
            process,
            address,
            size,
        })
    }

    pub(crate) fn write_wide(&self, value: &[u16]) -> Result<(), String> {
        let byte_count = value
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(|| "remote write size overflow".to_owned())?;

        if byte_count > self.size {
            return Err(format!(
                "remote write exceeds allocation: write={byte_count} allocation={}",
                self.size
            ));
        }

        let mut bytes_written: usize = 0;

        // SAFETY: `process` has VM write access, `address` identifies an
        // allocation of at least `self.size` bytes, `value` is readable for
        // `byte_count` bytes, and `bytes_written` is a writable output.
        let succeeded = unsafe {
            WriteProcessMemory(
                self.process.handle().as_raw_handle(),
                self.address,
                value.as_ptr().cast(),
                byte_count,
                &mut bytes_written,
            )
        };

        if succeeded == 0 {
            return Err(format!(
                "failed to write target memory: {}",
                io::Error::last_os_error()
            ));
        }

        if bytes_written != byte_count {
            return Err(format!(
                "incomplete target memory write: expected={byte_count} actual={bytes_written}"
            ));
        }

        Ok(())
    }

    pub(crate) fn address(&self) -> *mut c_void {
        self.address
    }
}

#[cfg(windows)]
impl Drop for RemoteAllocation<'_> {
    fn drop(&mut self) {
        // SAFETY: `address` was returned by `VirtualAllocEx` for this
        // process and has not previously been released.
        let _ = unsafe {
            VirtualFreeEx(
                self.process.handle().as_raw_handle(),
                self.address,
                0,
                MEM_RELEASE,
            )
        };
    }
}

#[cfg(windows)]
pub(crate) fn run_thread(
    process: &TargetProcess,
    address: usize,
    argument: *mut c_void,
    operation: &str,
) -> Result<u32, String> {
    // SAFETY: the loader and validated target are x86, so `usize` and the
    // remote thread entry address have matching widths. The caller resolved
    // `address` as a compatible target entry point.
    let start_routine = unsafe { transmute::<usize, ThreadStart>(address) };

    // SAFETY: `process` has the required thread access. `start_routine` is
    // the validated target entry point, and `argument` has the representation
    // expected by that entry point.
    let thread = unsafe {
        CreateRemoteThread(
            process.handle().as_raw_handle(),
            null(),
            0,
            Some(start_routine),
            argument,
            0,
            null_mut(),
        )
    };

    if thread.is_null() {
        return Err(format!(
            "failed to create remote {operation} thread: {}",
            io::Error::last_os_error()
        ));
    }

    // SAFETY: `thread` is a non-null owned handle returned by
    // `CreateRemoteThread`, and ownership is transferred exactly once.
    let thread = unsafe { OwnedHandle::from_raw_handle(thread) };

    // SAFETY: `thread` references a valid thread handle.
    let wait_result = unsafe { WaitForSingleObject(thread.as_raw_handle(), INFINITE) };

    if wait_result == WAIT_FAILED {
        return Err(format!(
            "failed waiting for remote {operation} thread: {}",
            io::Error::last_os_error()
        ));
    }

    if wait_result != WAIT_OBJECT_0 {
        return Err(format!(
            "unexpected remote {operation} wait result: 0x{wait_result:08X}"
        ));
    }

    let mut exit_code: u32 = 0;

    // SAFETY: the thread has completed and `exit_code` is writable.
    let succeeded = unsafe { GetExitCodeThread(thread.as_raw_handle(), &mut exit_code) };

    if succeeded == 0 {
        return Err(format!(
            "failed to read remote {operation} result: {}",
            io::Error::last_os_error()
        ));
    }

    Ok(exit_code)
}
