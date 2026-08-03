//! Injected daRPC client component.

#[cfg(windows)]
mod client_text;
#[cfg(any(windows, test))]
mod collections;
#[cfg(any(windows, test))]
mod commands;
#[cfg(any(windows, test))]
mod event_queue;
#[cfg(windows)]
mod hooks;
#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod ipc;
#[cfg(windows)]
mod lifecycle;
#[cfg(windows)]
mod map_name;
#[cfg(windows)]
mod movement;
#[cfg(any(windows, test))]
mod objects;
#[cfg(any(windows, test))]
mod packet;
#[cfg(windows)]
mod snapshot;
#[cfg(any(windows, test))]
mod state_events;

#[cfg(windows)]
use windows_sys::{
    Win32::Foundation::{HINSTANCE, TRUE},
    core::BOOL,
};

#[cfg(windows)]
use darpc_win32::lifecycle::{ABI_VERSION, InitializeFn, ShutdownFn, Status};

#[cfg(windows)]
#[unsafe(no_mangle)]
/// Minimal Windows DLL entry point.
///
/// # Safety
///
/// Windows must call this function using the documented DLL entry-point ABI
/// and provide the loader-supplied argument values.
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

        if let Err(error) = lifecycle::initialize() {
            return if error.unload_is_safe() {
                Status::INTERNAL_ERROR
            } else {
                Status::UNLOAD_UNSAFE
            };
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

        if lifecycle::shutdown().is_err() {
            return Status::INTERNAL_ERROR;
        }

        Status::OK
    })
    .unwrap_or(Status::INTERNAL_ERROR)
}

#[cfg(windows)]
const _: InitializeFn = darpc_initialize;

#[cfg(windows)]
const _: ShutdownFn = darpc_shutdown;
