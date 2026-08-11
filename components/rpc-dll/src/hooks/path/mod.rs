use darpc_game_client::{BUILD_BREADTH_FIRST_PATH_ENTRY, BUILD_BREADTH_FIRST_PATH_RVA};
use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use std::{
    io, panic,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub(crate) const NAME: &str = "breadth_first_path_builder";

const DETOUR_RANGE_LEN: usize = 192;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[unsafe(no_mangle)]
static PATH_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static PATH_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct PathHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

impl PathHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = target_address()?;
        validate_entry(target)?;
        let detour = NonNull::new(path_builder_detour as *mut u8)
            .ok_or_else(|| io::Error::other("path-builder detour address is null"))?;
        let detour_range = CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)
            .map_err(InstallError::from)?;
        let spec = DetourSpec::new(target, detour, detour_range, &PATH_HOOK_ACTIVITY)
            .map_err(InstallError::from)?;

        // SAFETY: client fingerprint and exact entry bytes were validated. The
        // detour preserves all eleven thiscall stack arguments and observes
        // the queue only after the original builder returns successfully.
        let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
        let relocated_bytes = u8::try_from(prepared.relocated_len())
            .map_err(|_| io::Error::other("path-builder relocated length does not fit in u8"))?;
        PATH_TRAMPOLINE.store(
            prepared.trampoline_address().map_err(InstallError::from)?,
            Ordering::Release,
        );

        let deadline = Instant::now() + INSTALL_TIMEOUT;
        let mut detour = loop {
            match prepared.install() {
                Ok(detour) => break detour,
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::sleep(COMMIT_RETRY_INTERVAL);
                }
                Err(error) => {
                    PATH_TRAMPOLINE.store(0, Ordering::Release);
                    return Err(InstallError::from(error));
                }
            }
        };
        let install_warning = detour.take_resume_warning().map(detour_error);
        Ok(Self {
            detour,
            relocated_bytes,
            install_warning,
        })
    }

    pub(crate) const fn relocated_bytes(&self) -> u8 {
        self.relocated_bytes
    }

    pub(crate) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(crate) fn uninstall(&mut self) -> io::Result<bool> {
        let deadline = Instant::now() + UNINSTALL_TIMEOUT;
        loop {
            match self.detour.uninstall() {
                Ok(changed) => {
                    PATH_TRAMPOLINE.store(0, Ordering::Release);
                    return Ok(changed);
                }
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::sleep(COMMIT_RETRY_INTERVAL);
                }
                Err(error) => return Err(detour_error(error)),
            }
        }
    }
}

fn target_address() -> io::Result<NonNull<u8>> {
    // SAFETY: a null module name requests the current executable module.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(BUILD_BREADTH_FIRST_PATH_RVA)
        .ok_or_else(|| io::Error::other("path-builder target address overflow"))?;
    NonNull::new(address as *mut u8)
        .ok_or_else(|| io::Error::other("path-builder target address is null"))
}

fn validate_entry(target: NonNull<u8>) -> io::Result<()> {
    // SAFETY: the supported executable fingerprint establishes readable code
    // spanning the fixed entry contract.
    let actual =
        unsafe { slice::from_raw_parts(target.as_ptr(), BUILD_BREADTH_FIRST_PATH_ENTRY.len()) };
    if actual == BUILD_BREADTH_FIRST_PATH_ENTRY {
        Ok(())
    } else {
        Err(io::Error::other("path-builder entry bytes do not match"))
    }
}

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn path_builder_detour(
    _world: *mut core::ffi::c_void,
    _start_x: i32,
    _start_y: i32,
    _goal_1_x: i32,
    _goal_1_y: i32,
    _goal_2_x: i32,
    _goal_2_y: i32,
    _goal_3_x: i32,
    _goal_3_y: i32,
    _goal_4_x: i32,
    _goal_4_y: i32,
    _allow_occupied: u32,
) -> usize {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push ecx",
        "mov edx, esp",
        "push dword ptr [edx + 48]",
        "push dword ptr [edx + 44]",
        "push dword ptr [edx + 40]",
        "push dword ptr [edx + 36]",
        "push dword ptr [edx + 32]",
        "push dword ptr [edx + 28]",
        "push dword ptr [edx + 24]",
        "push dword ptr [edx + 20]",
        "push dword ptr [edx + 16]",
        "push dword ptr [edx + 12]",
        "push dword ptr [edx + 8]",
        "mov ecx, dword ptr [edx]",
        "call dword ptr [{trampoline}]",
        "push eax",
        "push eax",
        "push dword ptr [esp + 8]",
        "call {observe}",
        "add esp, 8",
        "pop eax",
        "pop ecx",
        "lock dec dword ptr [{activity}]",
        "ret 44",
        activity = sym PATH_HOOK_ACTIVITY,
        trampoline = sym PATH_TRAMPOLINE,
        observe = sym observe_path,
    );
}

extern "C" fn observe_path(world: *const core::ffi::c_void, result: usize) {
    let _ = panic::catch_unwind(|| {
        if result != 0 {
            crate::route::observe(world, darpc_win32::pipe::sender_tick_ms());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::validate_entry;
    use darpc_game_client::BUILD_BREADTH_FIRST_PATH_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_path_builder_entry() {
        let mut entry = BUILD_BREADTH_FIRST_PATH_ENTRY;
        validate_entry(NonNull::new(entry.as_mut_ptr()).unwrap()).unwrap();
        entry[0] ^= 0xFF;
        assert!(validate_entry(NonNull::new(entry.as_mut_ptr()).unwrap()).is_err());
    }
}
