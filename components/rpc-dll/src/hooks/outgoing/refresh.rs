use darpc_game_client::{
    PHYSICAL_REFRESH_CALLER_RETURN_RVAS, REFRESH_USER_ENTRY, REFRESH_USER_RVA,
};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use std::{
    io, panic,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};

use crate::hooks::support;

const DETOUR_RANGE_LEN: usize = 64;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[unsafe(no_mangle)]
static REFRESH_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static REFRESH_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static REFRESH_MODULE_BASE: AtomicUsize = AtomicUsize::new(0);
static PHYSICAL_REFRESH_DEFERRED_COUNT: AtomicU32 = AtomicU32::new(0);

pub(super) struct PhysicalRefreshHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

impl PhysicalRefreshHook {
    pub(super) fn install() -> Result<Self, InstallError> {
        let module_base = support::module_base()?;
        let target = support::target_address(module_base, REFRESH_USER_RVA, "refresh user target")?;
        support::validate_bytes(target, &REFRESH_USER_ENTRY, "refresh user entry")?;

        // SAFETY: exact executable identity and entry bytes are validated. The
        // detour preserves ECX and the no-argument native ABI, and suppresses
        // only calls from the two validated physical F5 return addresses.
        let mut prepared = unsafe {
            support::prepare_detour(
                target,
                refresh_user_detour as *mut u8,
                DETOUR_RANGE_LEN,
                &REFRESH_HOOK_ACTIVITY,
                "refresh user detour",
            )
        }?;
        let relocated_bytes = support::relocated_bytes(&prepared, "refresh user prologue")?;
        REFRESH_MODULE_BASE.store(module_base, Ordering::Release);
        REFRESH_TRAMPOLINE.store(
            prepared.trampoline_address().map_err(InstallError::from)?,
            Ordering::Release,
        );
        PHYSICAL_REFRESH_DEFERRED_COUNT.store(0, Ordering::Release);

        let mut detour = match support::install_prepared(
            &mut prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                REFRESH_MODULE_BASE.store(0, Ordering::Release);
                REFRESH_TRAMPOLINE.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        };
        let install_warning = detour.take_resume_warning().map(support::detour_error);
        Ok(Self {
            detour,
            relocated_bytes,
            install_warning,
        })
    }

    pub(super) const fn relocated_bytes(&self) -> u8 {
        self.relocated_bytes
    }

    pub(super) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(super) fn uninstall(&mut self) -> io::Result<bool> {
        let changed =
            support::uninstall_detour(&mut self.detour, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
                .map_err(support::detour_error)?;
        REFRESH_MODULE_BASE.store(0, Ordering::Release);
        REFRESH_TRAMPOLINE.store(0, Ordering::Release);
        Ok(changed)
    }
}

pub(super) fn deferred_count() -> u32 {
    PHYSICAL_REFRESH_DEFERRED_COUNT.load(Ordering::Acquire)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn refresh_user_detour(_context: *mut core::ffi::c_void) -> u32 {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push ecx",
        "push dword ptr [esp + 4]",
        "call {defer}",
        "add esp, 4",
        "pop ecx",
        "test eax, eax",
        "jnz 1f",
        "call dword ptr [{trampoline}]",
        "push eax",
        "lock dec dword ptr [{activity}]",
        "pop eax",
        "ret",
        "1:",
        "xor eax, eax",
        "lock dec dword ptr [{activity}]",
        "ret",
        activity = sym REFRESH_HOOK_ACTIVITY,
        trampoline = sym REFRESH_TRAMPOLINE,
        defer = sym defer_physical_refresh,
    );
}

extern "C" fn defer_physical_refresh(caller_return: usize) -> i32 {
    panic::catch_unwind(|| {
        let module_base = REFRESH_MODULE_BASE.load(Ordering::Acquire);
        let Some(caller_rva) = caller_return.checked_sub(module_base) else {
            return 0;
        };
        if module_base == 0 || !is_physical_refresh_caller(caller_rva) {
            return 0;
        }
        PHYSICAL_REFRESH_DEFERRED_COUNT.fetch_add(1, Ordering::Relaxed);
        crate::state::refresh::request_physical();
        1
    })
    .unwrap_or(0)
}

const fn is_physical_refresh_caller(caller_rva: usize) -> bool {
    caller_rva == PHYSICAL_REFRESH_CALLER_RETURN_RVAS[0]
        || caller_rva == PHYSICAL_REFRESH_CALLER_RETURN_RVAS[1]
}

#[cfg(test)]
mod tests {
    use super::is_physical_refresh_caller;
    use darpc_game_client::{
        MOVEMENT_CORRECTION_REFRESH_CALLER_RETURN_RVA, PHYSICAL_REFRESH_CALLER_RETURN_RVAS,
    };

    #[test]
    fn defers_only_physical_f5_callers() {
        assert!(is_physical_refresh_caller(
            PHYSICAL_REFRESH_CALLER_RETURN_RVAS[0]
        ));
        assert!(is_physical_refresh_caller(
            PHYSICAL_REFRESH_CALLER_RETURN_RVAS[1]
        ));
        assert!(!is_physical_refresh_caller(
            MOVEMENT_CORRECTION_REFRESH_CALLER_RETURN_RVA
        ));
    }
}
