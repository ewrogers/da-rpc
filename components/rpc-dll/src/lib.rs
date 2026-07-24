//! Injected daRPC client component.

#[cfg(windows)]
use windows_sys::{
    Win32::Foundation::{HINSTANCE, TRUE},
    core::BOOL,
};

#[cfg(windows)]
use darpc_win32::lifecycle::{ABI_VERSION, InitializeFn, ShutdownFn, Status};

#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _module: HINSTANCE,
    _reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> BOOL {
    TRUE
}

#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn darpc_initialize(abi_version: u32) -> Status {
    std::panic::catch_unwind(|| {
        if abi_version != ABI_VERSION {
            return Status::UNSUPPORTED_ABI_VERSION;
        }

        Status::OK
    })
    .unwrap_or(Status::INTERNAL_ERROR)
}

#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn darpc_shutdown(reserved: u32) -> Status {
    std::panic::catch_unwind(|| {
        if reserved != 0 {
            return Status::INVALID_ARGUMENT;
        }

        Status::OK
    })
    .unwrap_or(Status::INTERNAL_ERROR)
}

#[cfg(windows)]
const _: InitializeFn = darpc_initialize;

#[cfg(windows)]
const _: ShutdownFn = darpc_shutdown;
