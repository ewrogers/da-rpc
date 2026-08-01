use super::DetourError;
use std::{ffi::c_void, io, ptr, ptr::NonNull, slice};
use windows_sys::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
    },
    Threading::GetCurrentProcess,
};

pub(super) const ALLOCATION_SIZE: usize = 4096;

pub(super) struct ExecutableMemory {
    address: NonNull<u8>,
}

impl ExecutableMemory {
    pub(super) fn allocate() -> Result<Self, DetourError> {
        // SAFETY: a null preferred address requests a new private allocation.
        // The returned region is committed and writable until `seal` changes it.
        let address = unsafe {
            VirtualAlloc(
                ptr::null(),
                ALLOCATION_SIZE,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let address = NonNull::new(address.cast::<u8>())
            .ok_or_else(|| DetourError::windows("VirtualAlloc for trampoline failed"))?;
        Ok(Self { address })
    }

    pub(super) fn address(&self) -> NonNull<u8> {
        self.address
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> Result<(), DetourError> {
        if bytes.len() > ALLOCATION_SIZE {
            return Err(DetourError::PrologueTooLong);
        }

        // SAFETY: the allocation is live, writable, and at least
        // `ALLOCATION_SIZE` bytes long. The source does not overlap it.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), self.address.as_ptr(), bytes.len());
        }
        Ok(())
    }

    pub(super) fn seal(&mut self, used: usize) -> Result<(), DetourError> {
        let mut old_protection = 0;
        // SAFETY: the allocation is live and `used` was bounded by `write`.
        if unsafe {
            VirtualProtect(
                self.address.as_ptr().cast::<c_void>(),
                used,
                PAGE_EXECUTE_READ,
                &mut old_protection,
            )
        } == 0
        {
            return Err(DetourError::windows(
                "VirtualProtect sealing trampoline failed",
            ));
        }

        // SAFETY: the current-process pseudo-handle is always valid and the
        // range is the newly executable portion of the live allocation.
        if unsafe { FlushInstructionCache(GetCurrentProcess(), self.address.as_ptr().cast(), used) }
            == 0
        {
            return Err(DetourError::windows(
                "FlushInstructionCache for trampoline failed",
            ));
        }
        Ok(())
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        // SAFETY: `address` is the base returned by VirtualAlloc and this
        // object owns the single matching release.
        unsafe {
            VirtualFree(self.address.as_ptr().cast(), 0, MEM_RELEASE);
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum CommitFault {
    None,
    #[cfg(test)]
    AfterWrite,
}

pub(super) unsafe fn replace_code(
    target: NonNull<u8>,
    expected: &[u8],
    replacement: &[u8],
    fault: CommitFault,
) -> Result<(), DetourError> {
    if expected.len() != replacement.len() || expected.is_empty() {
        return Err(DetourError::InvalidState);
    }

    // SAFETY: the caller guarantees the target range is readable and remains
    // stable while all other process threads are enlisted.
    let current = unsafe { slice::from_raw_parts(target.as_ptr(), expected.len()) };
    if current != expected {
        return Err(DetourError::TargetChanged);
    }

    let mut old_protection: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: the caller guarantees that target identifies live executable
    // code spanning `replacement.len()` bytes.
    if unsafe {
        VirtualProtect(
            target.as_ptr().cast(),
            replacement.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old_protection,
        )
    } == 0
    {
        return Err(DetourError::windows(
            "VirtualProtect opening target code failed",
        ));
    }

    // SAFETY: VirtualProtect made the complete target range writable and the
    // source cannot overlap it.
    unsafe {
        ptr::copy_nonoverlapping(replacement.as_ptr(), target.as_ptr(), replacement.len());
    }

    #[cfg(test)]
    if fault == CommitFault::AfterWrite {
        return rollback(
            target,
            expected,
            old_protection,
            "injected commit failure",
            io::Error::other("injected failure after target write"),
        );
    }
    let _ = fault;

    // SAFETY: the current-process pseudo-handle and modified range are valid.
    if unsafe {
        FlushInstructionCache(
            GetCurrentProcess(),
            target.as_ptr().cast(),
            replacement.len(),
        )
    } == 0
    {
        return rollback(
            target,
            expected,
            old_protection,
            "FlushInstructionCache for target code failed",
            io::Error::last_os_error(),
        );
    }

    let mut ignored = 0;
    // SAFETY: the same live target range is currently writable and
    // `old_protection` came from the successful opening call above.
    if unsafe {
        VirtualProtect(
            target.as_ptr().cast(),
            replacement.len(),
            old_protection,
            &mut ignored,
        )
    } == 0
    {
        return rollback(
            target,
            expected,
            old_protection,
            "VirtualProtect restoring target code failed",
            io::Error::last_os_error(),
        );
    }

    Ok(())
}

fn rollback(
    target: NonNull<u8>,
    original: &[u8],
    old_protection: PAGE_PROTECTION_FLAGS,
    operation: &'static str,
    source: io::Error,
) -> Result<(), DetourError> {
    // SAFETY: rollback is called only while the target range remains writable
    // after the corresponding commit failed.
    unsafe {
        ptr::copy_nonoverlapping(original.as_ptr(), target.as_ptr(), original.len());
    }

    let flush_error =
        // SAFETY: the current-process pseudo-handle and restored range are valid.
        (unsafe {
            FlushInstructionCache(
                GetCurrentProcess(),
                target.as_ptr().cast(),
                original.len(),
            )
        } == 0)
            .then(io::Error::last_os_error);

    let mut ignored = 0;
    let protect_error =
        // SAFETY: the live target range remains writable and old_protection
        // came from its successful opening call.
        (unsafe {
            VirtualProtect(
                target.as_ptr().cast(),
                original.len(),
                old_protection,
                &mut ignored,
            )
        } == 0)
            .then(io::Error::last_os_error);

    Err(DetourError::CommitFailed {
        operation,
        source,
        rollback: flush_error.or(protect_error),
    })
}
