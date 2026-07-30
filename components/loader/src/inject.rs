#[cfg(all(windows, not(target_arch = "x86")))]
compile_error!("loader must be built for an x86 Windows target");

use std::path::Path;

#[cfg(windows)]
use std::{
    ffi::{CStr, c_void},
    io,
    mem::{forget, size_of, transmute},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::{null, null_mut},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0},
    System::{
        Diagnostics::Debug::WriteProcessMemory,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
        },
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{CreateRemoteThread, GetExitCodeThread, INFINITE, WaitForSingleObject},
    },
};

#[cfg(windows)]
use crate::process;

#[cfg(windows)]
use darpc_win32::lifecycle::{ABI_VERSION, Status};

#[cfg(windows)]
type ThreadStart = unsafe extern "system" fn(*mut c_void) -> u32;

#[cfg(windows)]
struct RemoteAllocation<'a> {
    process: &'a OwnedHandle,
    address: *mut c_void,
    size: usize,
}

#[cfg(windows)]
impl<'a> RemoteAllocation<'a> {
    fn new(process: &'a OwnedHandle, size: usize) -> Result<Self, String> {
        if size == 0 {
            return Err("remote allocation size must be nonzero".to_owned());
        }

        // SAFETY: `process` references a valid handle with VM operation
        // access. Windows chooses the address, and `size` is nonzero.
        let address = unsafe {
            VirtualAllocEx(
                process.as_raw_handle(),
                null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };

        if address.is_null() {
            return Err(format!(
                "failed to allocate target memory: {}",
                io::Error::last_os_error()
            ));
        }

        Ok(Self {
            process,
            address,
            size,
        })
    }

    fn write_wide(&self, value: &[u16]) -> Result<(), String> {
        let byte_count = value
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(|| "remote write size overflow".to_owned())?;

        if byte_count > self.size {
            return Err(format!(
                "remote write exceeds allocation: write={byte_count} allocation={}",
                self.size
            ));
        }

        let mut bytes_written: usize = 0;

        // SAFETY: `process` has VM write access, `address` identifies an
        // allocation of at least `self.size` bytes, `value` is readable for
        // `byte_count` bytes, and `bytes_written` is a writable output.
        let succeeded = unsafe {
            WriteProcessMemory(
                self.process.as_raw_handle(),
                self.address,
                value.as_ptr().cast(),
                byte_count,
                &mut bytes_written,
            )
        };

        if succeeded == 0 {
            return Err(format!(
                "failed to write target memory: {}",
                io::Error::last_os_error()
            ));
        }

        if bytes_written != byte_count {
            return Err(format!(
                "incomplete target memory write: expected={byte_count} actual={bytes_written}"
            ));
        }

        Ok(())
    }

    fn address(&self) -> *mut c_void {
        self.address
    }
}

#[cfg(windows)]
impl Drop for RemoteAllocation<'_> {
    fn drop(&mut self) {
        // SAFETY: `address` was returned by `VirtualAllocEx` for this
        // process and has not previously been released.
        let _ =
            unsafe { VirtualFreeEx(self.process.as_raw_handle(), self.address, 0, MEM_RELEASE) };
    }
}

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
fn run_remote_thread(
    process: &OwnedHandle,
    address: usize,
    argument: *mut c_void,
    operation: &str,
) -> Result<u32, String> {
    // SAFETY: the loader and validated target are x86, so `usize` and the
    // remote thread entry address have matching widths. The caller resolved
    // `address` as a compatible target entry point.
    let start_routine = unsafe { transmute::<usize, ThreadStart>(address) };

    // SAFETY: `process` has the required thread access. `start_routine` is
    // the validated target entry point, and `argument` has the representation
    // expected by that entry point.
    let thread = unsafe {
        CreateRemoteThread(
            process.as_raw_handle(),
            null(),
            0,
            Some(start_routine),
            argument,
            0,
            null_mut(),
        )
    };

    if thread.is_null() {
        return Err(format!(
            "failed to create remote {operation} thread: {}",
            io::Error::last_os_error()
        ));
    }

    // SAFETY: `thread` is a non-null owned handle returned by
    // `CreateRemoteThread`, and ownership is transferred exactly once.
    let thread = unsafe { OwnedHandle::from_raw_handle(thread) };

    // SAFETY: `thread` references a valid thread handle.
    let wait_result = unsafe { WaitForSingleObject(thread.as_raw_handle(), INFINITE) };

    if wait_result == WAIT_FAILED {
        return Err(format!(
            "failed waiting for remote {operation} thread: {}",
            io::Error::last_os_error()
        ));
    }

    if wait_result != WAIT_OBJECT_0 {
        return Err(format!(
            "unexpected remote {operation} wait result: 0x{wait_result:08X}"
        ));
    }

    let mut exit_code: u32 = 0;

    // SAFETY: the thread has completed and `exit_code` is writable.
    let succeeded = unsafe { GetExitCodeThread(thread.as_raw_handle(), &mut exit_code) };

    if succeeded == 0 {
        return Err(format!(
            "failed to read remote {operation} result: {}",
            io::Error::last_os_error()
        ));
    }

    Ok(exit_code)
}

#[cfg(windows)]
pub(crate) fn attach(pid: u32, dll_path: &Path, initialize_rva: u32) -> Result<(), String> {
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

    let (module_name, offset) = local_kernel_export(c"LoadLibraryW")?;

    println!("Local LoadLibraryW: module={module_name} offset=0x{offset:X}");

    let remote_module_base = process::module_base(pid, &module_name)?.ok_or_else(|| {
        format!("target module containing LoadLibraryW is not loaded: {module_name}")
    })?;

    let remote_load_library = remote_module_base
        .checked_add(offset)
        .ok_or_else(|| "target LoadLibraryW address overflow".to_owned())?;

    println!(
        "Target LoadLibraryW: module={module_name} base=0x{remote_module_base:X} \
        offset=0x{offset:X} address=0x{remote_load_library:X}"
    );

    let dll_path_size = dll_path_wide
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| "DLL path size overflow".to_owned())?;

    let remote_path = RemoteAllocation::new(&process, dll_path_size)?;

    println!("Allocated remote DLL path buffer: {dll_path_size} bytes");

    remote_path.write_wide(&dll_path_wide)?;

    println!("Wrote remote DLL path buffer: {dll_path_size} bytes");

    let exit_code = match run_remote_thread(
        &process,
        remote_load_library,
        remote_path.address(),
        "LoadLibraryW",
    ) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            forget(remote_path);
            return Err(error);
        }
    };

    if exit_code == 0 {
        return Err("remote LoadLibraryW failed".to_owned());
    }

    let loaded_module = process::module_base(pid, "darpc.dll")?.ok_or_else(|| {
        "LoadLibraryW succeeded, but darpc.dll is absent from the target module list".to_owned()
    })?;

    if loaded_module != exit_code as usize {
        return Err(format!(
            "loaded module address mismatch: thread=0x{exit_code:08X} \
            snapshot=0x{loaded_module:08X}"
        ));
    }

    println!("Verified loaded module: 0x{loaded_module:08X}");

    let initialize_offset = usize::try_from(initialize_rva)
        .map_err(|_| "darpc_initialize RVA does not fit usize".to_owned())?;

    let remote_initialize = loaded_module
        .checked_add(initialize_offset)
        .ok_or_else(|| "target darpc_initialize address overflow".to_owned())?;

    println!(
        "Target darpc_initialize: module=0x{loaded_module:08X} \
        rva=0x{initialize_rva:08X} address=0x{remote_initialize:08X}"
    );

    let initialize_argument =
        usize::try_from(ABI_VERSION).map_err(|_| "ABI version does not fit usize".to_owned())?;

    let initialize_status = run_remote_thread(
        &process,
        remote_initialize,
        initialize_argument as *mut c_void,
        "darpc_initialize",
    )?;

    if initialize_status != Status::OK.as_u32() {
        return Err(format!(
            "darpc_initialize failed: status={initialize_status}"
        ));
    }

    println!("darpc_initialize succeeded: status={initialize_status}");
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn attach(_pid: u32, _dll_path: &Path, _initialize_rva: u32) -> Result<(), String> {
    Err("loader requires Windows".to_owned())
}
