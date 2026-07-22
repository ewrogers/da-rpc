//! Injected daRPC client component.

#[cfg(windows)]
use windows_sys::{
    Win32::Foundation::{HINSTANCE, TRUE},
    core::BOOL,
};

#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _module: HINSTANCE,
    _reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> BOOL {
    TRUE
}
