//! Transactional in-process hooks used by the injected daRPC library.

#[cfg(all(windows, target_arch = "x86"))]
mod x86;

#[cfg(all(windows, target_arch = "x86"))]
pub use x86::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstalledDetour, PreparedDetour,
};
