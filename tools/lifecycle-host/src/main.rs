//! Local test host for validating the `darpc.dll` lifecycle.

use std::process::ExitCode;

#[cfg(windows)]
use darpc_win32::lifecycle::{
    ABI_VERSION, INITIALIZE_EXPORT, InitializeFn, SHUTDOWN_EXPORT, ShutdownFn, Status,
};
#[cfg(windows)]
use std::{env, fs, io, mem, os::windows::ffi::OsStrExt};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
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

    let mut dll_path_wide: Vec<u16> = dll_path.as_os_str().encode_wide().collect();
    dll_path_wide.push(0);

    debug_assert_eq!(dll_path_wide.last(), Some(&0));

    // SAFETY: `dll_path_wide` is a live, null-terminated UTF-16
    // buffer whose pointer remains valid for the duration of the call.
    let module = unsafe { LoadLibraryW(dll_path_wide.as_ptr()) };

    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    println!("Loaded module: {module:p}");

    let resolve_result = (|| -> io::Result<()> {
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
        // the module remains loaded, and zero is the required reserved value.
        let status = unsafe { shutdown(0) };

        if status != Status::OK {
            return Err(io::Error::other(format!(
                "darpc_shutdown returned status {}",
                status.as_u32()
            )));
        }

        println!("Shut down");

        Ok(())
    })();

    // SAFETY: `module` is a non-null handle returned by a successful
    // `LoadLibraryW` call, and its owned reference has not been released.
    let unloaded = unsafe { FreeLibrary(module) };

    if unloaded == 0 {
        return Err(io::Error::last_os_error());
    }
    println!("Unloaded module");

    resolve_result?;
    Ok(())
}
