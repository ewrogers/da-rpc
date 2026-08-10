use darpc_game_client::WINDOW_CLASS;
use std::{
    collections::BTreeSet, ffi::OsString, io, os::windows::ffi::OsStringExt as _, path::PathBuf,
};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId},
    },
    core::BOOL,
};

const MAX_CLASS_NAME: usize = 256;
const MAX_EXECUTABLE_PATH: usize = 32_768;

pub(crate) fn client_pids() -> io::Result<BTreeSet<u32>> {
    let mut pids = BTreeSet::new();
    let context = &raw mut pids;

    // SAFETY: context points to a live BTreeSet for the duration of the
    // synchronous enumeration. enum_window restores that exact pointer and
    // never retains it.
    let result = unsafe { EnumWindows(Some(enum_window), context as LPARAM) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pids)
}

pub(crate) fn client_maps_directory(pid: u32) -> io::Result<PathBuf> {
    let mut path = vec![0_u16; MAX_EXECUTABLE_PATH];
    let mut length = u32::try_from(path.len())
        .map_err(|_| io::Error::other("executable path buffer length does not fit u32"))?;
    // SAFETY: the requested access is read-only and pid is supplied by Windows
    // discovery or an explicitly configured process identifier.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: process is a live query handle and path is writable for length
    // UTF-16 code units. The handle is closed immediately after this call.
    let result =
        unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &raw mut length) };
    let query_error = (result == 0).then(io::Error::last_os_error);
    // SAFETY: process was returned by OpenProcess and has not been closed yet.
    unsafe { CloseHandle(process) };
    if let Some(error) = query_error {
        return Err(error);
    }
    let length = usize::try_from(length)
        .map_err(|_| io::Error::other("executable path length does not fit usize"))?;
    let executable = PathBuf::from(OsString::from_wide(&path[..length]));
    executable
        .parent()
        .map(|directory| directory.join("Maps"))
        .ok_or_else(|| io::Error::other("client executable has no parent directory"))
}

unsafe extern "system" fn enum_window(window: HWND, context: LPARAM) -> BOOL {
    let mut class_name = [0_u16; MAX_CLASS_NAME];

    // SAFETY: class_name is writable for MAX_CLASS_NAME UTF-16 code units and
    // window is supplied by EnumWindows for the duration of this callback.
    let length = unsafe { GetClassNameW(window, class_name.as_mut_ptr(), MAX_CLASS_NAME as i32) };
    if length <= 0 || !class_matches(&class_name[..length as usize]) {
        return 1;
    }

    let mut pid = 0_u32;
    // SAFETY: pid points to writable storage and window remains valid for this
    // callback. A zero PID is ignored below.
    unsafe { GetWindowThreadProcessId(window, &raw mut pid) };
    if pid != 0 {
        // SAFETY: context was created from a unique mutable BTreeSet reference
        // in client_pids and EnumWindows invokes callbacks synchronously.
        unsafe { &mut *(context as *mut BTreeSet<u32>) }.insert(pid);
    }
    1
}

fn class_matches(class_name: &[u16]) -> bool {
    class_name.iter().copied().eq(WINDOW_CLASS.encode_utf16())
}

#[cfg(test)]
mod tests {
    use super::class_matches;

    #[test]
    fn matches_only_the_supported_window_class() {
        assert!(class_matches(
            &"Darkages".encode_utf16().collect::<Vec<_>>()
        ));
        assert!(!class_matches(
            &"darkages".encode_utf16().collect::<Vec<_>>()
        ));
        assert!(!class_matches(
            &"Darkages - Character".encode_utf16().collect::<Vec<_>>()
        ));
    }
}
