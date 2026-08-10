use std::ptr;

use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar};

pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let length = i32::try_from(bytes.len()).ok()?;
    // SAFETY: `bytes` is valid for `length` bytes and this query requests only
    // the required UTF-16 length.
    let required =
        unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, ptr::null_mut(), 0) };
    if required <= 0 {
        return None;
    }
    let mut wide = vec![0_u16; required as usize];
    // SAFETY: `wide` has the capacity reported by the first call and both
    // buffers remain valid for the duration of the conversion.
    let written = unsafe {
        MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            length,
            wide.as_mut_ptr(),
            required,
        )
    };
    (written == required).then(|| String::from_utf16_lossy(&wide))
}
