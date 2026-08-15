use darpc_game_client::{EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use std::{
    io, panic,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};

use super::support;
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
        let target = support::target_address(
            support::module_base()?,
            EVENT_DISPATCHER_TICK_RVA,
            "tick target",
        )?;
        support::validate_bytes(target, &EVENT_DISPATCHER_TICK_ENTRY, "tick entry")?;

        // SAFETY: the supported executable fingerprint and exact target entry
        // bytes were validated. The detour preserves the target's thiscall ABI,
        // stays loaded with this DLL, and brackets its full execution with the
        // supplied activity counter.
        let mut prepared = unsafe {
            support::prepare_detour(
                target,
                event_dispatcher_tick_detour as *mut u8,
                DETOUR_RANGE_LEN,
                &TICK_HOOK_ACTIVITY,
                "tick detour",
            )
        }?;
        let relocated_bytes = support::relocated_bytes(&prepared, "tick prologue")?;

        commands::reset();
        snapshot::reset();
        TICK_COUNT.store(0, Ordering::Release);
        TICK_RELOCATED_BYTES.store(u32::from(relocated_bytes), Ordering::Release);
        TICK_TRAMPOLINE.store(
            prepared.trampoline_address().map_err(InstallError::from)?,
            Ordering::Release,
        );

        let mut detour = match support::install_prepared(
            &mut prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                TICK_TRAMPOLINE.store(0, Ordering::Release);
                TICK_RELOCATED_BYTES.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        };
        let install_warning = detour.take_resume_warning().map(support::detour_error);
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
        let changed =
            support::uninstall_detour(&mut self.detour, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
                .map_err(support::detour_error)?;
        TICK_HOOK_INSTALLED.store(false, Ordering::Release);
        TICK_TRAMPOLINE.store(0, Ordering::Release);
        TICK_RELOCATED_BYTES.store(0, Ordering::Release);
        Ok(changed)
    }
}

pub(crate) fn health() -> TickHealth {
    TickHealth {
        installed: TICK_HOOK_INSTALLED.load(Ordering::Acquire),
        relocated_bytes: TICK_RELOCATED_BYTES.load(Ordering::Acquire) as u8,
        tick_count: TICK_COUNT.load(Ordering::Acquire),
    }
}

#[unsafe(naked)]
unsafe extern "thiscall" fn event_dispatcher_tick_detour(_dispatcher: *mut core::ffi::c_void) {
    // The original dispatcher can remain active for the client lifetime. End
    // DLL activity before tail-jumping so uninstall only waits for observe_tick.
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push ecx",
        "call {observe}",
        "pop ecx",
        "lock dec dword ptr [{activity}]",
        "jmp dword ptr [{trampoline}]",
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
    use super::support;
    use darpc_game_client::EVENT_DISPATCHER_TICK_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_tick_entry() {
        let mut entry = EVENT_DISPATCHER_TICK_ENTRY;
        let target = NonNull::new(entry.as_mut_ptr()).unwrap();
        support::validate_bytes(target, &EVENT_DISPATCHER_TICK_ENTRY, "tick entry").unwrap();

        let mut wrong_entry = EVENT_DISPATCHER_TICK_ENTRY;
        wrong_entry[0] ^= 0xFF;
        let wrong_target = NonNull::new(wrong_entry.as_mut_ptr()).unwrap();
        let error =
            support::validate_bytes(wrong_target, &EVENT_DISPATCHER_TICK_ENTRY, "tick entry")
                .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("bytes do not match"));
    }
}
