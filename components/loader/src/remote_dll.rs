#[cfg(windows)]
use std::{
    ffi::{CStr, c_void},
    io,
    mem::{forget, size_of},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::null_mut,
};

#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
};

#[cfg(windows)]
use crate::{
    process::TargetProcess,
    remote::{self, RemoteAllocation},
};

#[cfg(windows)]
fn encode_path(path: &Path) -> Result<Vec<u16>, String> {
    let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();

    if encoded.contains(&0) {
        return Err("DLL path contains an interior null character".to_owned());
    }

    encoded.push(0);
    Ok(encoded)
}

#[cfg(windows)]
fn local_kernel_export(export_name: &CStr) -> Result<(String, usize), String> {
    let kernel32: Vec<u16> = "kernel32.dll".encode_utf16().chain([0]).collect();

    // SAFETY: `kernel32` is a live, NUL-terminated UTF-16 string.
    let kernel32 = unsafe { GetModuleHandleW(kernel32.as_ptr()) };

    if kernel32.is_null() {
        return Err(format!(
            "failed to locate local kernel32.dll: {}",
            io::Error::last_os_error()
        ));
    }

    let function_name = export_name.to_string_lossy();

    // SAFETY: `kernel32` is a valid loaded module and the export name
    // is a live, NUL-terminated byte string.
    let function = unsafe { GetProcAddress(kernel32, export_name.as_ptr().cast()) }
        .ok_or_else(|| format!("failed to resolve local {function_name}"))?;

    let mut containing_module = null_mut();

    // SAFETY: with FROM_ADDRESS, the second parameter is interpreted as
    // an address. `function` is valid and the output is writable.
    let succeeded = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            function as *const () as *const u16,
            &mut containing_module,
        )
    };

    if succeeded == 0 {
        return Err(format!(
            "failed to locate module containing `{function_name}`: {}",
            io::Error::last_os_error()
        ));
    }

    let mut module_path = [0_u16; 1024];
    let capacity = u32::try_from(module_path.len())
        .map_err(|_| "module path capacity does not fit in u32".to_owned())?;

    // SAFETY: `containing_module` is valid and `module_path` is writable.
    let length =
        unsafe { GetModuleFileNameW(containing_module, module_path.as_mut_ptr(), capacity) };

    let length = usize::try_from(length)
        .map_err(|_| "module path length does not fit in usize".to_owned())?;

    if length == 0 || length >= module_path.len() {
        return Err(format!("failed to read module containing {function_name}"));
    }

    let path = String::from_utf16_lossy(&module_path[..length]);
    let module_name = path
        .rsplit(['\\', '/'])
        .next()
        .ok_or_else(|| "resolved module path has no file name".to_owned())?
        .to_owned();

    let offset = (function as usize)
        .checked_sub(containing_module as usize)
        .ok_or_else(|| format!("{function_name} address precedes its module base"))?;

    Ok((module_name, offset))
}

#[cfg(windows)]
fn remote_kernel_export(process: &TargetProcess, export_name: &CStr) -> Result<usize, String> {
    let function_name = export_name.to_string_lossy();
    let (module_name, offset) = local_kernel_export(export_name)?;

    println!("Local {function_name}: module={module_name} offset=0x{offset:X}");

    let module_base = process.module_base(&module_name)?.ok_or_else(|| {
        format!("target module containing {function_name} is not loaded: {module_name}")
    })?;

    let address = module_base
        .checked_add(offset)
        .ok_or_else(|| format!("target {function_name} address overflow"))?;

    println!(
        "Target {function_name}: module={module_name} base=0x{module_base:X} \
        offset=0x{offset:X} address=0x{address:X}"
    );

    Ok(address)
}

#[cfg(windows)]
pub(crate) fn load(
    process: &TargetProcess,
    path: &Path,
    expected_name: &str,
) -> Result<usize, String> {
    let encoded_path = encode_path(path)?;

    println!("Encoded DLL path: {} UTF-16 code units", encoded_path.len());

    let load_library = remote_kernel_export(process, c"LoadLibraryW")?;
    let path_size = encoded_path
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| "DLL path size overflow".to_owned())?;

    let remote_path = RemoteAllocation::new(process, path_size)?;
    println!("Allocated remote DLL path buffer: {path_size} bytes");

    remote_path.write_wide(&encoded_path)?;
    println!("Wrote remote DLL path buffer: {path_size} bytes");

    let exit_code =
        match remote::run_thread(process, load_library, remote_path.address(), "LoadLibraryW") {
            Ok(exit_code) => exit_code,
            Err(error) => {
                forget(remote_path);
                return Err(error);
            }
        };

    if exit_code == 0 {
        return Err("remote LoadLibraryW failed".to_owned());
    }

    let loaded_module = process.module_base(expected_name)?.ok_or_else(|| {
        format!("LoadLibraryW succeeded, but {expected_name} is absent from the target module list")
    })?;

    if loaded_module != exit_code as usize {
        return Err(format!(
            "loaded module address mismatch: thread=0x{exit_code:08X} \
            snapshot=0x{loaded_module:08X}"
        ));
    }

    println!("Verified loaded module: 0x{loaded_module:08X}");
    Ok(loaded_module)
}

#[cfg(windows)]
pub(crate) fn unload(process: &TargetProcess, module: usize) -> Result<(), String> {
    let free_library = remote_kernel_export(process, c"FreeLibrary")?;
    let exit_code =
        remote::run_thread(process, free_library, module as *mut c_void, "FreeLibrary")?;

    if exit_code == 0 {
        return Err("remote FreeLibrary returned failure".to_owned());
    }

    Ok(())
}
