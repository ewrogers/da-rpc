use std::path::Path;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use crate::process;

#[cfg(windows)]
fn encode_dll_path(path: &Path) -> Result<Vec<u16>, String> {
    let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();

    if encoded.contains(&0) {
        return Err("DLL path contains an interior null character".to_owned());
    }

    encoded.push(0);

    Ok(encoded)
}

#[cfg(windows)]
pub(crate) fn attach(pid: u32, dll_path: &Path) -> Result<(), String> {
    let dll_path_wide = encode_dll_path(dll_path)?;

    println!(
        "Encoded DLL path: {} UTF-16 code units",
        dll_path_wide.len()
    );

    let process = process::open_for_injection(pid)?;
    let inspection = process::inspect_process(pid, &process)?;

    if inspection.darpc_loaded {
        return Err(format!("darpc.dll is already loaded in process {pid}"));
    }

    println!(
        "Attach preflight complete: pid={pid} creation_time={}",
        inspection.creation_time
    );

    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn attach(_pid: u32, _dll_path: &Path) -> Result<(), String> {
    Err("loader requires Windows".to_owned())
}

