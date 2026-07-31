#[cfg(all(windows, not(target_arch = "x86")))]
compile_error!("loader must be built for an x86 Windows target");

use crate::{
    error::{ErrorKind, LoaderError, Result},
    pe::DarpcDll,
    process::{ProcessInspection, TargetProcess},
};

#[cfg(windows)]
use crate::process::ProcessModule;

#[cfg(windows)]
use std::{ffi::c_void, fs};

#[cfg(windows)]
use crate::{remote, remote_dll};

#[cfg(windows)]
use darpc_win32::lifecycle::{ABI_VERSION, Status};

#[cfg(windows)]
const DARPC_MODULE_NAME: &str = "darpc.dll";

pub(crate) struct LifecycleOutcome {
    pub(crate) inspection: ProcessInspection,
    pub(crate) changed: bool,
}

#[cfg(windows)]
fn export_address(module: usize, rva: u32, export_name: &str) -> Result<usize> {
    let offset = usize::try_from(rva).map_err(|_| {
        LoaderError::new(
            ErrorKind::Internal,
            format!("{export_name} RVA does not fit usize"),
        )
    })?;

    module.checked_add(offset).ok_or_else(|| {
        LoaderError::new(
            ErrorKind::Internal,
            format!("target {export_name} address overflow"),
        )
    })
}

#[cfg(windows)]
fn initialize(process: &TargetProcess, module: usize, initialize_rva: u32) -> Result<u32> {
    let initialize = export_address(module, initialize_rva, "darpc_initialize")?;

    eprintln!(
        "Target darpc_initialize: module=0x{module:08X} \
        rva=0x{initialize_rva:08X} address=0x{initialize:08X}"
    );

    let argument = usize::try_from(ABI_VERSION)
        .map_err(|_| LoaderError::new(ErrorKind::Internal, "ABI version does not fit usize"))?;

    remote::run_thread(
        process,
        initialize,
        argument as *mut c_void,
        "darpc_initialize",
    )
}

#[cfg(windows)]
fn shutdown(process: &TargetProcess, module: usize, shutdown_rva: u32) -> Result<u32> {
    let shutdown = export_address(module, shutdown_rva, "darpc_shutdown")?;

    eprintln!(
        "Target darpc_shutdown: module=0x{module:08X} \
        rva=0x{shutdown_rva:08X} address=0x{shutdown:08X}"
    );

    remote::run_thread(process, shutdown, std::ptr::null_mut(), "darpc_shutdown")
}

#[cfg(windows)]
fn validate_loaded_dll(module: &ProcessModule, dll: &DarpcDll) -> Result<()> {
    let observed_path = fs::canonicalize(&module.path).map_err(|error| {
        LoaderError::from_io(
            ErrorKind::RemoteOperationFailed,
            format!(
                "failed to canonicalize loaded module path `{}`",
                module.path.display()
            ),
            error,
        )
    })?;

    if !observed_path
        .to_string_lossy()
        .eq_ignore_ascii_case(&dll.path.to_string_lossy())
    {
        return Err(LoaderError::new(
            ErrorKind::InvalidDll,
            format!(
                "loaded {DARPC_MODULE_NAME} path does not match the selected DLL: \
                loaded=`{}` selected=`{}`",
                observed_path.display(),
                dll.path.display()
            ),
        ));
    }

    Ok(())
}

#[cfg(windows)]
pub(crate) fn attach(process: &TargetProcess, dll: &DarpcDll) -> Result<LifecycleOutcome> {
    let inspection = process.inspect()?;
    let pid = process.pid();

    if inspection.darpc_module.is_some() {
        return Err(LoaderError::new(
            ErrorKind::AlreadyLoaded,
            format!("{DARPC_MODULE_NAME} is already loaded in process {pid}"),
        ));
    }

    eprintln!(
        "Attach preflight complete: pid={pid} creation_time={}",
        inspection.creation_time
    );

    let module = remote_dll::load(process, &dll.path, DARPC_MODULE_NAME)?;
    let status = initialize(process, module, dll.initialize_rva)?;

    if status != Status::OK.as_u32() {
        let initialize_error = format!("darpc_initialize failed: status={status}");

        if let Err(error) = remote_dll::unload(process, module) {
            return Err(LoaderError::new(
                ErrorKind::InitializationFailed,
                format!("{initialize_error}; rollback FreeLibrary failed: {error}"),
            ));
        }

        if process.module_base(DARPC_MODULE_NAME)?.is_some() {
            return Err(LoaderError::new(
                ErrorKind::InitializationFailed,
                format!("{initialize_error}; rollback left {DARPC_MODULE_NAME} loaded"),
            ));
        }

        eprintln!("Rollback FreeLibrary succeeded");
        return Err(LoaderError::new(
            ErrorKind::InitializationFailed,
            initialize_error,
        ));
    }

    eprintln!("darpc_initialize succeeded: status={status}");

    let inspection = process.inspect()?;
    let Some(observed_module) = inspection.darpc_module.as_ref() else {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!("attach completed, but {DARPC_MODULE_NAME} is absent"),
        ));
    };

    if observed_module.base != module {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!(
                "attach completed, but the observed {DARPC_MODULE_NAME} module does not match \
                0x{module:08X}"
            ),
        ));
    }

    validate_loaded_dll(observed_module, dll)?;

    Ok(LifecycleOutcome {
        inspection,
        changed: true,
    })
}

#[cfg(windows)]
pub(crate) fn detach(process: &TargetProcess, dll: &DarpcDll) -> Result<LifecycleOutcome> {
    let inspection = process.inspect()?;
    let pid = process.pid();

    let Some(observed_module) = inspection.darpc_module.as_ref() else {
        eprintln!("{DARPC_MODULE_NAME} is already absent from process {pid}");
        return Ok(LifecycleOutcome {
            inspection,
            changed: false,
        });
    };

    validate_loaded_dll(observed_module, dll)?;
    let module = observed_module.base;

    eprintln!(
        "Detach preflight complete: pid={pid} creation_time={} module=0x{module:08X}",
        inspection.creation_time
    );

    let status = shutdown(process, module, dll.shutdown_rva)?;

    if status != Status::OK.as_u32() {
        return Err(LoaderError::new(
            ErrorKind::ShutdownFailed,
            format!("darpc_shutdown failed: status={status}; {DARPC_MODULE_NAME} remains loaded"),
        ));
    }

    eprintln!("darpc_shutdown succeeded: status={status}");
    remote_dll::unload(process, module)?;

    let inspection = process.inspect()?;
    if inspection.darpc_module.is_some() {
        return Err(LoaderError::new(
            ErrorKind::RemoteOperationFailed,
            format!("FreeLibrary succeeded, but {DARPC_MODULE_NAME} remains loaded"),
        ));
    }

    eprintln!("Verified unloaded module");
    Ok(LifecycleOutcome {
        inspection,
        changed: true,
    })
}

#[cfg(not(windows))]
pub(crate) fn attach(_process: &TargetProcess, _dll: &DarpcDll) -> Result<LifecycleOutcome> {
    Err(LoaderError::new(
        ErrorKind::UnsupportedPlatform,
        "loader requires Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn detach(_process: &TargetProcess, _dll: &DarpcDll) -> Result<LifecycleOutcome> {
    Err(LoaderError::new(
        ErrorKind::UnsupportedPlatform,
        "loader requires Windows",
    ))
}
