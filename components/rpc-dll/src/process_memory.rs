#![cfg_attr(test, allow(dead_code))]

use darpc_game_client::MemoryReader;
use std::{mem, slice};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess,
};

/// A plain value for which every initialized bit pattern is valid.
///
/// # Safety
///
/// Implementors must permit every possible bit pattern and contain no
/// references whose validity cannot be established from copied process bytes.
pub(crate) unsafe trait ProcessValue: Copy {}

// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for u8 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for i8 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for u16 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for i16 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for u32 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for i32 {}
// SAFETY: every bit pattern is valid for these integer values.
unsafe impl ProcessValue for usize {}
// SAFETY: every bit pattern is valid for an array of bytes.
unsafe impl<const N: usize> ProcessValue for [u8; N] {}

pub(crate) fn read<T: ProcessValue>(address: usize) -> Option<T> {
    let mut value = mem::MaybeUninit::<T>::uninit();
    // SAFETY: the byte slice covers the uninitialized destination exactly.
    // ProcessValue's unsafe contract permits every possible bit pattern.
    let output =
        unsafe { slice::from_raw_parts_mut(value.as_mut_ptr().cast::<u8>(), mem::size_of::<T>()) };
    read_exact(address, output).then(|| {
        // SAFETY: read_exact initialized every byte, and ProcessValue only
        // permits types for which every bit pattern is valid.
        unsafe { value.assume_init() }
    })
}

pub(crate) fn read_exact(address: usize, output: &mut [u8]) -> bool {
    if output.is_empty() {
        return true;
    }
    let mut read = 0_usize;
    // SAFETY: output is valid for its length. ReadProcessMemory validates the
    // current-process source range and reports failure without creating a Rust
    // reference to that memory.
    let succeeded = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const core::ffi::c_void,
            output.as_mut_ptr().cast(),
            output.len(),
            &mut read,
        )
    };
    succeeded != 0 && read == output.len()
}

pub(crate) struct ProcessMemory;

impl MemoryReader for ProcessMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> bool {
        read_exact(address as usize, output)
    }
}
