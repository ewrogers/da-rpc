use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use std::{
    io,
    ptr::{self, NonNull},
    slice, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub(super) fn module_base() -> io::Result<usize> {
    // SAFETY: a null module name requests the current executable module and
    // does not transfer ownership.
    let module = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    (module != 0)
        .then_some(module)
        .ok_or_else(io::Error::last_os_error)
}

pub(super) fn target_address(module: usize, rva: usize, label: &str) -> io::Result<NonNull<u8>> {
    let address = module
        .checked_add(rva)
        .ok_or_else(|| io::Error::other(format!("{label} address overflow")))?;
    NonNull::new(address as *mut u8)
        .ok_or_else(|| io::Error::other(format!("{label} address is null")))
}

/// Validates a fixed code contract at a previously resolved client address.
///
/// # Safety
///
/// `target` must be readable for `expected.len()` bytes for this call.
pub(super) unsafe fn validate_bytes(
    target: NonNull<u8>,
    expected: &[u8],
    label: &str,
) -> io::Result<()> {
    // SAFETY: the caller guarantees that target is readable for the complete
    // expected byte contract.
    let actual = unsafe { slice::from_raw_parts(target.as_ptr(), expected.len()) };
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} bytes do not match: expected={expected:02X?} actual={actual:02X?}"),
        ))
    }
}

/// Prepares an inline detour after its executable identity, entry bytes, and
/// ABI-specific implementation have been validated by the caller.
///
/// # Safety
///
/// `target`, `detour`, `activity`, and the detour ABI must satisfy
/// [`DetourSpec`] and [`PreparedDetour::prepare`] for the full installed lifetime.
pub(super) unsafe fn prepare_detour(
    target: NonNull<u8>,
    detour: *mut u8,
    detour_range_len: usize,
    activity: &'static DetourActivity,
    label: &str,
) -> Result<PreparedDetour, InstallError> {
    let detour =
        NonNull::new(detour).ok_or_else(|| io::Error::other(format!("{label} address is null")))?;
    let range =
        CodeRange::new(detour.as_ptr() as usize, detour_range_len).map_err(InstallError::from)?;
    let spec = DetourSpec::new(target, detour, range, activity).map_err(InstallError::from)?;
    // SAFETY: the caller upholds this helper's documented target, ABI,
    // lifetime, and activity-counter requirements.
    unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)
}

pub(super) fn relocated_bytes(prepared: &PreparedDetour, label: &str) -> Result<u8, InstallError> {
    u8::try_from(prepared.relocated_len()).map_err(|_| {
        io::Error::other(format!("{label} relocated length does not fit in u8")).into()
    })
}

pub(super) fn install_prepared(
    prepared: &mut PreparedDetour,
    timeout: Duration,
    retry_interval: Duration,
) -> Result<InstalledDetour, DetourError> {
    let deadline = Instant::now() + timeout;
    loop {
        match prepared.install() {
            Ok(detour) => return Ok(detour),
            Err(error) if error.is_transient() && Instant::now() < deadline => {
                thread::sleep(retry_interval);
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn uninstall_detour(
    detour: &mut InstalledDetour,
    timeout: Duration,
    retry_interval: Duration,
) -> Result<bool, DetourError> {
    let deadline = Instant::now() + timeout;
    loop {
        match detour.uninstall() {
            Ok(changed) => return Ok(changed),
            Err(error) if error.is_transient() && Instant::now() < deadline => {
                thread::sleep(retry_interval);
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}
