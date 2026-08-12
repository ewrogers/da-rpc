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
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{CreateRemoteThread, GetExitCodeThread, WaitForSingleObject},
    },
};

#[cfg(windows)]
use crate::{
    error::{ErrorKind, LoaderError, Result},
    process::TargetProcess,
};

#[cfg(windows)]
type ThreadStart = unsafe extern "system" fn(*mut c_void) -> u32;

#[cfg(windows)]
pub(crate) const REMOTE_THREAD_TIMEOUT_MS: u32 = 10_000;

#[cfg(windows)]
pub(crate) fn read(process: &TargetProcess, address: usize, size: usize) -> Result<Vec<u8>> {
    if size == 0 {
        return Err(LoaderError::new(
            ErrorKind::Internal,
            "remote read size must be nonzero",
        ));
    }

    let mut bytes = vec![0; size];
    let mut bytes_read = 0;

    // SAFETY: `process` has VM read access, `address` is supplied by the
    // caller, `bytes` is writable for `size` bytes, and `bytes_read` is a
    // writable output. Windows validates the remote address range.
    let succeeded = unsafe {
        ReadProcessMemory(
            process.handle().as_raw_handle(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            size,
            &mut bytes_read,
        )
    };

    if succeeded == 0 {
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to read target memory at 0x{address:08X}"),
            io::Error::last_os_error(),
        ));
    }

    if bytes_read != size {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!(
                "incomplete target memory read at 0x{address:08X}: expected={size} actual={bytes_read}"
            ),
        ));
    }

    Ok(bytes)
}

#[cfg(windows)]
pub(crate) fn write(process: &TargetProcess, address: usize, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(LoaderError::new(
            ErrorKind::Internal,
            "remote write size must be nonzero",
        ));
    }

    let mut bytes_written = 0;

    // SAFETY: `process` has VM write access, `address` is supplied by the
    // caller, `bytes` is readable for its complete length, and
    // `bytes_written` is a writable output. Windows validates the remote
    // address range and its current protection.
    let succeeded = unsafe {
        WriteProcessMemory(
            process.handle().as_raw_handle(),
            address as *mut c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut bytes_written,
        )
    };

    if succeeded == 0 {
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to write target memory at 0x{address:08X}"),
            io::Error::last_os_error(),
        ));
    }

    if bytes_written != bytes.len() {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!(
                "incomplete target memory write at 0x{address:08X}: expected={} actual={bytes_written}",
                bytes.len()
            ),
        ));
    }

    Ok(())
}

#[cfg(windows)]
pub(crate) struct RemoteAllocation<'a> {
    process: &'a TargetProcess,
    address: *mut c_void,
    size: usize,
}

#[cfg(windows)]
impl<'a> RemoteAllocation<'a> {
    pub(crate) fn new(process: &'a TargetProcess, size: usize) -> Result<Self> {
        if size == 0 {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                "remote allocation size must be nonzero",
            ));
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
            return Err(LoaderError::from_io(
                ErrorKind::RemoteOperationFailed,
                "failed to allocate target memory",
                io::Error::last_os_error(),
            ));
        }

        Ok(Self {
            process,
            address,
            size,
        })
    }

    pub(crate) fn write_wide(&self, value: &[u16]) -> Result<()> {
        let byte_count = value
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "remote write size overflow"))?;

        if byte_count > self.size {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                format!(
                    "remote write exceeds allocation: write={byte_count} allocation={}",
                    self.size
                ),
            ));
        }

        // SAFETY: a `u16` slice is contiguous and may be viewed as bytes for
        // the duration of this write.
        let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr().cast(), byte_count) };
        write(self.process, self.address as usize, bytes)
    }

    pub(crate) fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.size {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                format!(
                    "remote write exceeds allocation: write={} allocation={}",
                    bytes.len(),
                    self.size
                ),
            ));
        }

        write(self.process, self.address as usize, bytes)
    }

    pub(crate) fn address(&self) -> *mut c_void {
        self.address
    }

    pub(crate) fn persist(self) -> usize {
        let address = self.address as usize;
        std::mem::forget(self);
        address
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
) -> Result<u32> {
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
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to create remote {operation} thread"),
            io::Error::last_os_error(),
        ));
    }

    // SAFETY: `thread` is a non-null owned handle returned by
    // `CreateRemoteThread`, and ownership is transferred exactly once.
    let thread = unsafe { OwnedHandle::from_raw_handle(thread) };

    // SAFETY: `thread` references a valid thread handle.
    let wait_result =
        unsafe { WaitForSingleObject(thread.as_raw_handle(), REMOTE_THREAD_TIMEOUT_MS) };

    if wait_result == WAIT_TIMEOUT {
        return Err(LoaderError::new(
            ErrorKind::Timeout,
            format!(
                "remote {operation} thread did not complete within {REMOTE_THREAD_TIMEOUT_MS} ms"
            ),
        ));
    }

    if wait_result == WAIT_FAILED {
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed waiting for remote {operation} thread"),
            io::Error::last_os_error(),
        ));
    }

    if wait_result != WAIT_OBJECT_0 {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!("unexpected remote {operation} wait result: 0x{wait_result:08X}"),
        ));
    }

    let mut exit_code: u32 = 0;

    // SAFETY: the thread has completed and `exit_code` is writable.
    let succeeded = unsafe { GetExitCodeThread(thread.as_raw_handle(), &mut exit_code) };

    if succeeded == 0 {
        return Err(LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!("failed to read remote {operation} result"),
            io::Error::last_os_error(),
        ));
    }

    Ok(exit_code)
}
