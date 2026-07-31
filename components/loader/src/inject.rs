#[cfg(all(windows, not(target_arch = "x86")))]
compile_error!("loader must be built for an x86 Windows target");

use crate::{pe::DarpcDll, process::TargetProcess};

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
use crate::{remote, remote_dll};

#[cfg(windows)]
use darpc_win32::lifecycle::{ABI_VERSION, Status};

#[cfg(windows)]
const DARPC_MODULE_NAME: &str = "darpc.dll";

#[cfg(windows)]
fn initialize(process: &TargetProcess, module: usize, initialize_rva: u32) -> Result<u32, String> {
    let initialize_offset = usize::try_from(initialize_rva)
        .map_err(|_| "darpc_initialize RVA does not fit usize".to_owned())?;

    let initialize = module
        .checked_add(initialize_offset)
        .ok_or_else(|| "target darpc_initialize address overflow".to_owned())?;

    println!(
        "Target darpc_initialize: module=0x{module:08X} \
        rva=0x{initialize_rva:08X} address=0x{initialize:08X}"
    );

    let argument =
        usize::try_from(ABI_VERSION).map_err(|_| "ABI version does not fit usize".to_owned())?;

    remote::run_thread(
        process,
        initialize,
        argument as *mut c_void,
        "darpc_initialize",
    )
}

#[cfg(windows)]
pub(crate) fn attach(process: &TargetProcess, dll: &DarpcDll) -> Result<(), String> {
    let inspection = process.inspect()?;
    let pid = process.pid();

    if inspection.darpc_loaded {
        return Err(format!(
            "{DARPC_MODULE_NAME} is already loaded in process {pid}"
        ));
    }

    println!(
        "Attach preflight complete: pid={pid} creation_time={}",
        inspection.creation_time
    );

    let module = remote_dll::load(process, &dll.path, DARPC_MODULE_NAME)?;
    let status = initialize(process, module, dll.initialize_rva)?;

    if status != Status::OK.as_u32() {
        let initialize_error = format!("darpc_initialize failed: status={status}");

        if let Err(error) = remote_dll::unload(process, module) {
            return Err(format!(
                "{initialize_error}; rollback FreeLibrary failed: {error}"
            ));
        }

        println!("Rollback FreeLibrary succeeded");
        return Err(initialize_error);
    }

    println!("darpc_initialize succeeded: status={status}");
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn attach(_process: &TargetProcess, _dll: &DarpcDll) -> Result<(), String> {
    Err("loader requires Windows".to_owned())
}
