//! Shared Windows integration boundaries for daRPC.

pub mod lifecycle;

#[cfg(windows)]
pub mod pipe;
