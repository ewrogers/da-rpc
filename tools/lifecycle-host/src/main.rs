//! Local test host for validating the `darpc.dll` lifecycle.

use std::process::ExitCode;

#[cfg(windows)]
use darpc_win32::lifecycle::{
    ABI_VERSION, INITIALIZE_EXPORT, InitializeFn, SHUTDOWN_EXPORT, ShutdownFn, Status,
};
#[cfg(windows)]
use std::{env, fs, io, mem, os::windows::ffi::OsStrExt, path::Path};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
};

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("lifecycle-host requires Windows");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lifecycle-host: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn run() -> io::Result<()> {
    let mut arguments = env::args_os().skip(1);

    let dll_path = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: lifecycle-host <path-to-darpc.dll>",
        )
    })?;

    if arguments.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly one DLL path",
        ));
    }

    let dll_path = fs::canonicalize(dll_path)?;

    if !dll_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DLL path is not a file",
        ));
    }

    const CYCLE_COUNT: usize = 3;

    for cycle in 1..=CYCLE_COUNT {
        println!("Cycle {cycle}/{CYCLE_COUNT}");
        run_cycle(&dll_path)?;
    }

    Ok(())
}

#[cfg(windows)]
fn run_cycle(dll_path: &Path) -> io::Result<()> {
    let dll_name = dll_path
        .file_name()
        .ok_or_else(|| io::Error::other("DLL path has no file name"))?;

    let mut dll_path_wide: Vec<u16> = dll_path.as_os_str().encode_wide().collect();
    dll_path_wide.push(0);

    debug_assert_eq!(dll_path_wide.last(), Some(&0));

    let mut dll_name_wide: Vec<u16> = dll_name.encode_wide().collect();
    dll_name_wide.push(0);

    debug_assert_eq!(dll_name_wide.last(), Some(&0));

    // SAFETY: `dll_path_wide` is a live, null-terminated UTF-16
    // buffer whose pointer remains valid for the duration of the call.
    let module = unsafe { LoadLibraryW(dll_path_wide.as_ptr()) };

    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    println!("Loaded module: {module:p}");

    let resolve_result = (|| -> io::Result<()> {
        // SAFETY: `dll_name_wide` is a live, null-terminated UTF-16
        // module name, and this call does not increase the reference count.
        let observed_module = unsafe { GetModuleHandleW(dll_name_wide.as_ptr()) };

        if observed_module != module {
            return Err(io::Error::other(
                "loaded DLL was not found in the module list",
            ));
        }

        println!("Verified module is loaded");

        // SAFETY: `module` is a valid loaded-module handle and
        // `INITIALIZE_EXPORT` is a null-terminated ASCII name.
        let initialize = unsafe { GetProcAddress(module, INITIALIZE_EXPORT.as_ptr()) }
            .ok_or_else(io::Error::last_os_error)?;

        println!("Resolved darpc_initialize: {:p}", initialize as *const ());

        // SAFETY: `module` is a valid loaded-module handle and
        // `SHUTDOWN_EXPORT` is a null-terminated ASCII name.
        let shutdown = unsafe { GetProcAddress(module, SHUTDOWN_EXPORT.as_ptr()) }
            .ok_or_else(io::Error::last_os_error)?;

        println!("Resolved darpc_shutdown: {:p}", shutdown as *const ());

        // SAFETY: `initialize` was resolved from the expected export name,
        // and `InitializeFn` is the shared definition of that export's ABI.
        let initialize: InitializeFn = unsafe { mem::transmute(initialize) };

        // SAFETY: `shutdown` was resolved from the expected export name,
        // and `ShutdownFn` is the shared definition of that export's ABI.
        let shutdown: ShutdownFn = unsafe { mem::transmute(shutdown) };

        let unsupported_version = ABI_VERSION + 1;

        // SAFETY: the function pointer was resolved from the loaded module,
        // and the module remains loaded for this call.
        let status = unsafe { initialize(unsupported_version) };

        if status != Status::UNSUPPORTED_ABI_VERSION {
            return Err(io::Error::other(format!(
                "darpc_initialize({unsupported_version}) returns status {}, expected {}",
                status.as_u32(),
                Status::UNSUPPORTED_ABI_VERSION.as_u32()
            )));
        }

        println!("Rejected unsupported ABI version");

        // SAFETY: the function pointer was resolved from the loaded module,
        // the module remains loaded, and `ABI_VERSION` is a valid argument.
        let status = unsafe { initialize(ABI_VERSION) };

        if status != Status::OK {
            return Err(io::Error::other(format!(
                "darpc_initialize returned status {}",
                status.as_u32()
            )));
        }

        println!("Initialized");

        // SAFETY: the function pointer was resolved from the loaded module,
        // the module remains loaded, and initialization is safe to repeat.
        let status = unsafe { initialize(ABI_VERSION) };

        if status != Status::OK {
            return Err(io::Error::other(format!(
                "repeated darpc_initialize returned status {}",
                status.as_u32()
            )));
        }

        println!("Repeated initialization succeeded");

        // SAFETY: the function pointer was resolved from the loaded module,
        // and the module remains loaded for this call.
        let status = unsafe { shutdown(1) };

        if status != Status::INVALID_ARGUMENT {
            return Err(io::Error::other(format!(
                "darpc_shutdown(1) returned status {}, expected {}",
                status.as_u32(),
                Status::INVALID_ARGUMENT.as_u32()
            )));
        }

        println!("Rejected invalid shutdown argument");

        // SAFETY: the function pointer was resolved from the loaded module,
        // the module remains loaded, and zero is the required reserved value.
        let status = unsafe { shutdown(0) };

        if status != Status::OK {
            return Err(io::Error::other(format!(
                "darpc_shutdown returned status {}",
                status.as_u32()
            )));
        }

        println!("Shut down");

        // SAFETY: the function pointer was resolved from the loaded module,
        // the module remains loaded, and zero is the required reserved value.
        let status = unsafe { shutdown(0) };

        if status != Status::OK {
            return Err(io::Error::other(format!(
                "repeated darpc_shutdown returned status {}",
                status.as_u32()
            )));
        }

        println!("Repeated shutdown succeeded");

        Ok(())
    })();

    // SAFETY: `module` is a non-null handle returned by a successful
    // `LoadLibraryW` call, and its owned reference has not been released.
    let unloaded = unsafe { FreeLibrary(module) };

    if unloaded == 0 {
        return Err(io::Error::last_os_error());
    }
    println!("Unloaded module");

    // SAFETY: `dll_name_wide` remains a live, null-terminated UTF-16
    // module name, and this call does not increase the reference count.
    let observed_module = unsafe { GetModuleHandleW(dll_name_wide.as_ptr()) };

    if !observed_module.is_null() {
        return Err(io::Error::other("DLL remains loaded after FreeLibrary"));
    }

    println!("Verified module is unloaded");

    resolve_result?;
    Ok(())
}
