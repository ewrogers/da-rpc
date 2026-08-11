use darpc_game_client::{EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA};
use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use std::{
    io, panic,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::{commands, snapshot};

pub(crate) const NAME: &str = "event_dispatcher_tick";

const DETOUR_RANGE_LEN: usize = 64;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[unsafe(no_mangle)]
static TICK_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static TICK_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static TICK_COUNT: AtomicU32 = AtomicU32::new(0);
static TICK_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static TICK_RELOCATED_BYTES: AtomicU32 = AtomicU32::new(0);

pub(crate) struct TickHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TickHealth {
    pub(crate) installed: bool,
    pub(crate) relocated_bytes: u8,
    pub(crate) tick_count: u32,
}

impl TickHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = target_address()?;
        validate_entry(target)?;
        let detour = NonNull::new(event_dispatcher_tick_detour as *mut u8)
            .ok_or_else(|| io::Error::other("tick detour address is null"))?;
        let detour_range = CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)
            .map_err(InstallError::from)?;
        let spec = DetourSpec::new(target, detour, detour_range, &TICK_HOOK_ACTIVITY)
            .map_err(InstallError::from)?;

        // SAFETY: the supported executable fingerprint and exact target entry
        // bytes were validated. The detour preserves the target's thiscall ABI,
        // stays loaded with this DLL, and brackets its full execution with the
        // supplied activity counter.
        let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
        let relocated_bytes = u8::try_from(prepared.relocated_len())
            .map_err(|_| io::Error::other("relocated tick prologue exceeds u8"))?;

        commands::reset();
        snapshot::reset();
        TICK_COUNT.store(0, Ordering::Release);
        TICK_RELOCATED_BYTES.store(u32::from(relocated_bytes), Ordering::Release);
        TICK_TRAMPOLINE.store(
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
                    TICK_TRAMPOLINE.store(0, Ordering::Release);
                    TICK_RELOCATED_BYTES.store(0, Ordering::Release);
                    return Err(InstallError::from(error));
                }
            }
        };
        let install_warning = detour.take_resume_warning().map(detour_error);
        TICK_HOOK_INSTALLED.store(true, Ordering::Release);

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
                    TICK_HOOK_INSTALLED.store(false, Ordering::Release);
                    TICK_TRAMPOLINE.store(0, Ordering::Release);
                    TICK_RELOCATED_BYTES.store(0, Ordering::Release);
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

pub(crate) fn health() -> TickHealth {
    TickHealth {
        installed: TICK_HOOK_INSTALLED.load(Ordering::Acquire),
        relocated_bytes: TICK_RELOCATED_BYTES.load(Ordering::Acquire) as u8,
        tick_count: TICK_COUNT.load(Ordering::Acquire),
    }
}

fn target_address() -> io::Result<NonNull<u8>> {
    // SAFETY: a null module name requests the executable module for the current
    // process and has no lifetime transfer.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(EVENT_DISPATCHER_TICK_RVA)
        .ok_or_else(|| io::Error::other("tick target address overflow"))?;
    NonNull::new(address as *mut u8).ok_or_else(|| io::Error::other("tick target address is null"))
}

fn validate_entry(target: NonNull<u8>) -> io::Result<()> {
    // SAFETY: the supported executable fingerprint establishes that the target
    // RVA names readable executable memory spanning this fixed entry contract.
    let actual =
        unsafe { slice::from_raw_parts(target.as_ptr(), EVENT_DISPATCHER_TICK_ENTRY.len()) };
    if actual != EVENT_DISPATCHER_TICK_ENTRY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{NAME} entry mismatch at RVA 0x{EVENT_DISPATCHER_TICK_RVA:08X}: expected={:02X?} actual={actual:02X?}",
                EVENT_DISPATCHER_TICK_ENTRY
            ),
        ));
    }
    Ok(())
}

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn event_dispatcher_tick_detour(_dispatcher: *mut core::ffi::c_void) {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push ecx",
        "call {observe}",
        "pop ecx",
        "call dword ptr [{trampoline}]",
        "lock dec dword ptr [{activity}]",
        "ret",
        activity = sym TICK_HOOK_ACTIVITY,
        observe = sym observe_tick,
        trampoline = sym TICK_TRAMPOLINE,
    );
}

extern "C" fn observe_tick() {
    let _ = panic::catch_unwind(|| {
        TICK_COUNT.fetch_add(1, Ordering::Relaxed);
        #[cfg(not(test))]
        crate::actions::movement::observe_tick();
        commands::observe_tick();
        crate::player::observe_tick(darpc_win32::pipe::sender_tick_ms());
        crate::state::observe_tick();
        snapshot::observe_tick();
    });
}

#[cfg(test)]
mod tests {
    use super::validate_entry;
    use darpc_game_client::EVENT_DISPATCHER_TICK_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_tick_entry() {
        let mut entry = EVENT_DISPATCHER_TICK_ENTRY;
        let target = NonNull::new(entry.as_mut_ptr()).unwrap();
        validate_entry(target).unwrap();

        let mut wrong_entry = EVENT_DISPATCHER_TICK_ENTRY;
        wrong_entry[0] ^= 0xFF;
        let wrong_target = NonNull::new(wrong_entry.as_mut_ptr()).unwrap();
        let error = validate_entry(wrong_target).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("entry mismatch"));
    }
}
