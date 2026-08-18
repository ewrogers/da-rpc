use darpc_game_client::{
    BUILD_BREADTH_FIRST_PATH_ENTRY, BUILD_BREADTH_FIRST_PATH_RVA, QUEUED_STEP_CALL,
    QUEUED_STEP_CALL_RVA, RESET_MOVEMENT_RVA, WALK_RVA,
};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use std::{
    ffi::c_void,
    io, mem, panic,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use super::support;

pub(crate) const NAME: &str = "path_control";

const PATH_DETOUR_RANGE_LEN: usize = 192;
const INLINE_DETOUR_RANGE_LEN: usize = 64;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);

pub(crate) type CanMoveFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, u8, u8) -> usize;
type WalkFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;
type ResetFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;

#[unsafe(no_mangle)]
static PATH_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
#[unsafe(no_mangle)]
static QUEUED_STEP_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();

static PATH_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static QUEUED_STEP_RESUME: AtomicUsize = AtomicUsize::new(0);
static WALK_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static RESET_ADDRESS: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct PathHook {
    path_detour: Option<InstalledDetour>,
    step_detour: Option<InstalledDetour>,
    path_relocated_bytes: u8,
    step_relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

impl PathHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let module = support::module_base()?;
        let path_target =
            support::target_address(module, BUILD_BREADTH_FIRST_PATH_RVA, "path builder")?;
        let step_target =
            support::target_address(module, QUEUED_STEP_CALL_RVA, "queued-step call")?;

        support::validate_bytes(
            path_target,
            &BUILD_BREADTH_FIRST_PATH_ENTRY,
            "path-builder entry",
        )?;
        support::validate_bytes(step_target, &QUEUED_STEP_CALL, "queued-step call")?;

        // SAFETY: the supported executable identity and exact target bytes
        // were validated, and each detour preserves its native x86 ABI.
        let mut path_prepared = unsafe {
            support::prepare_detour(
                path_target,
                path_builder_detour as *mut u8,
                PATH_DETOUR_RANGE_LEN,
                &PATH_HOOK_ACTIVITY,
                "path-builder detour",
            )
        }?;
        // SAFETY: the validated call site and detour preserve the native x86 ABI.
        let mut step_prepared = unsafe {
            support::prepare_detour(
                step_target,
                queued_step_detour as *mut u8,
                INLINE_DETOUR_RANGE_LEN,
                &QUEUED_STEP_HOOK_ACTIVITY,
                "queued-step detour",
            )
        }?;

        let path_relocated_bytes = support::relocated_bytes(&path_prepared, "path builder")?;
        let step_relocated_bytes = support::relocated_bytes(&step_prepared, "queued step")?;
        let path_trampoline = path_prepared
            .trampoline_address()
            .map_err(InstallError::from)?;

        QUEUED_STEP_RESUME.store(
            step_target.as_ptr() as usize + QUEUED_STEP_CALL.len(),
            Ordering::Release,
        );
        WALK_ADDRESS.store(module + WALK_RVA, Ordering::Release);
        RESET_ADDRESS.store(module + RESET_MOVEMENT_RVA, Ordering::Release);

        let mut step_detour = match support::install_prepared(
            &mut step_prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                if error.unload_is_safe() {
                    clear_inline_addresses();
                }
                return Err(InstallError::from(error));
            }
        };
        if let Some(warning) = step_detour.take_resume_warning().map(support::detour_error) {
            return Ok(Self::partial(
                None,
                Some(step_detour),
                path_relocated_bytes,
                step_relocated_bytes,
                warning,
            ));
        }

        PATH_TRAMPOLINE.store(path_trampoline, Ordering::Release);
        let mut path_detour = match support::install_prepared(
            &mut path_prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                return Ok(Self::partial(
                    None,
                    Some(step_detour),
                    path_relocated_bytes,
                    step_relocated_bytes,
                    io::Error::other(format!("path-builder hook installation failed: {error}")),
                ));
            }
        };
        let install_warning = path_detour.take_resume_warning().map(support::detour_error);
        Ok(Self {
            path_detour: Some(path_detour),
            step_detour: Some(step_detour),
            path_relocated_bytes,
            step_relocated_bytes,
            install_warning,
        })
    }

    fn partial(
        path_detour: Option<InstalledDetour>,
        step_detour: Option<InstalledDetour>,
        path_relocated_bytes: u8,
        step_relocated_bytes: u8,
        warning: io::Error,
    ) -> Self {
        Self {
            path_detour,
            step_detour,
            path_relocated_bytes,
            step_relocated_bytes,
            install_warning: Some(warning),
        }
    }

    pub(crate) const fn relocated_bytes(&self) -> u8 {
        self.path_relocated_bytes
    }

    pub(crate) const fn step_relocated_bytes(&self) -> u8 {
        self.step_relocated_bytes
    }

    pub(crate) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(crate) fn uninstall(&mut self) -> io::Result<bool> {
        let mut changed = false;
        changed |= uninstall_owned(&mut self.path_detour)?;
        if self.path_detour.is_none() {
            PATH_TRAMPOLINE.store(0, Ordering::Release);
        }
        changed |= uninstall_owned(&mut self.step_detour)?;
        if self.path_detour.is_none() && self.step_detour.is_none() {
            clear_inline_addresses();
        }
        Ok(changed)
    }
}

fn uninstall_owned(detour: &mut Option<InstalledDetour>) -> io::Result<bool> {
    let Some(installed) = detour.as_mut() else {
        return Ok(false);
    };
    let changed = support::uninstall_detour(installed, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
        .map_err(support::detour_error)?;
    *detour = None;
    Ok(changed)
}

fn clear_inline_addresses() {
    QUEUED_STEP_RESUME.store(0, Ordering::Release);
    WALK_ADDRESS.store(0, Ordering::Release);
    RESET_ADDRESS.store(0, Ordering::Release);
}

pub(crate) unsafe fn native_edge_allowed(
    can_move: CanMoveFn,
    world: *mut c_void,
    x: i32,
    y: i32,
    direction: u8,
) -> bool {
    // SAFETY: callers provide the exact client helper and its validated live
    // WorldPane. Mode 1 preserves live changes and known dynamic occupants.
    if unsafe { can_move(world, x, y, direction, 1) } == 0 {
        return false;
    }
    // SAFETY: the same native invariants hold for mode 0, which adds
    // collision from complete raw map storage.
    unsafe { can_move(world, x, y, direction, 0) != 0 }
}

#[unsafe(naked)]
unsafe extern "thiscall" fn queued_step_detour(_world: *mut c_void, _direction: u8) -> usize {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "call {try_step}",
        "lock dec dword ptr [{activity}]",
        "jmp dword ptr [{resume}]",
        activity = sym QUEUED_STEP_HOOK_ACTIVITY,
        try_step = sym try_queued_step,
        resume = sym QUEUED_STEP_RESUME,
    );
}

unsafe extern "thiscall" fn try_queued_step(world: *mut c_void, direction: u8) -> usize {
    panic::catch_unwind(|| {
        let walk_address = WALK_ADDRESS.load(Ordering::Acquire);
        let reset_address = RESET_ADDRESS.load(Ordering::Acquire);
        if walk_address == 0 || reset_address == 0 || world.is_null() {
            return 0;
        }
        // SAFETY: the call site and exact client fingerprint fix the receiver,
        // direction, RVA, and x86 thiscall ABI.
        let walk: WalkFn = unsafe { mem::transmute(walk_address) };
        // SAFETY: this wrapper replaces the client's original call with the
        // same receiver and direction.
        let result = unsafe { walk(world, direction) };
        #[cfg(not(test))]
        let reset_exact_route =
            crate::actions::movement::observe_queued_step(world, direction, result != 0);
        #[cfg(test)]
        let reset_exact_route = false;
        if reset_exact_route {
            // SAFETY: the failed native step left the same complete WorldPane
            // live on the main thread. Only an externally installed exact
            // route is reset; native ground routes and pursuits remain stock.
            let reset: ResetFn = unsafe { mem::transmute(reset_address) };
            unsafe { reset(world, 0) };
        }
        result
    })
    .unwrap_or(0)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn path_builder_detour(
    _world: *mut c_void,
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

extern "C" fn observe_path(world: *const c_void, result: usize) {
    let _ = panic::catch_unwind(|| {
        if result != 0 {
            let tick_ms = darpc_win32::pipe::sender_tick_ms();
            if let Some(destination) = crate::route::observe(world, tick_ms) {
                #[cfg(not(test))]
                crate::actions::movement::observe_native_route(world, destination);
                #[cfg(test)]
                let _ = destination;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{native_edge_allowed, support};
    use darpc_game_client::{BUILD_BREADTH_FIRST_PATH_ENTRY, QUEUED_STEP_CALL};
    use std::{
        ffi::c_void,
        ptr::NonNull,
        sync::atomic::{AtomicI32, AtomicU8, Ordering},
    };

    static SEEN_X: AtomicI32 = AtomicI32::new(0);
    static SEEN_Y: AtomicI32 = AtomicI32::new(0);
    static SEEN_MODE: AtomicU8 = AtomicU8::new(u8::MAX);

    unsafe extern "thiscall" fn allow_edge(
        _world: *mut c_void,
        x: i32,
        y: i32,
        _direction: u8,
        mode: u8,
    ) -> usize {
        SEEN_X.store(x, Ordering::Relaxed);
        SEEN_Y.store(y, Ordering::Relaxed);
        SEEN_MODE.store(mode, Ordering::Relaxed);
        1
    }

    #[test]
    fn validates_route_observation_sites() {
        for (label, expected) in [
            ("path entry", BUILD_BREADTH_FIRST_PATH_ENTRY.as_slice()),
            ("queued-step call", QUEUED_STEP_CALL.as_slice()),
        ] {
            let mut bytes = expected.to_vec();
            let target = NonNull::new(bytes.as_mut_ptr()).unwrap();
            support::validate_bytes(target, expected, label).unwrap();
            bytes[0] ^= 0xFF;
            let error = support::validate_bytes(target, expected, label).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn passes_native_collision_coordinates_as_x_then_y() {
        // SAFETY: the fake helper ignores its null WorldPane and only records
        // the scalar ABI arguments supplied by the wrapper.
        assert!(unsafe { native_edge_allowed(allow_edge, std::ptr::null_mut(), 4, 9, 2) });
        assert_eq!(SEEN_X.load(Ordering::Relaxed), 4);
        assert_eq!(SEEN_Y.load(Ordering::Relaxed), 9);
        assert_eq!(SEEN_MODE.load(Ordering::Relaxed), 0);
    }
}
