use darpc_game_client::{
    BUILD_BREADTH_FIRST_PATH_ENTRY, BUILD_BREADTH_FIRST_PATH_RVA, MAP_CAN_MOVE_DIRECTION_RVA,
    QUEUED_STEP_CALL, QUEUED_STEP_CALL_RVA, RESET_MOVEMENT_RVA, ROUTE_COLLISION_CALL,
    ROUTE_COLLISION_CALL_RVA, WALK_RVA,
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

type CanMoveFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, u8, u8) -> usize;
type WalkFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;
type ResetFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;

#[unsafe(no_mangle)]
static PATH_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
#[unsafe(no_mangle)]
static ROUTE_COLLISION_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
#[unsafe(no_mangle)]
static QUEUED_STEP_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();

static PATH_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static ROUTE_COLLISION_RESUME: AtomicUsize = AtomicUsize::new(0);
static QUEUED_STEP_RESUME: AtomicUsize = AtomicUsize::new(0);
static CAN_MOVE_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static WALK_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static RESET_ADDRESS: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct PathHook {
    path_detour: Option<InstalledDetour>,
    collision_detour: Option<InstalledDetour>,
    step_detour: Option<InstalledDetour>,
    path_relocated_bytes: u8,
    collision_relocated_bytes: u8,
    step_relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

impl PathHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let module = support::module_base()?;
        let path_target =
            support::target_address(module, BUILD_BREADTH_FIRST_PATH_RVA, "path builder")?;
        let collision_target =
            support::target_address(module, ROUTE_COLLISION_CALL_RVA, "route collision call")?;
        let step_target =
            support::target_address(module, QUEUED_STEP_CALL_RVA, "queued-step call")?;

        support::validate_bytes(
            path_target,
            &BUILD_BREADTH_FIRST_PATH_ENTRY,
            "path-builder entry",
        )?;
        support::validate_bytes(
            collision_target,
            &ROUTE_COLLISION_CALL,
            "route-collision call",
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
        let mut collision_prepared = unsafe {
            support::prepare_detour(
                collision_target,
                route_collision_detour as *mut u8,
                INLINE_DETOUR_RANGE_LEN,
                &ROUTE_COLLISION_HOOK_ACTIVITY,
                "route-collision detour",
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
        let collision_relocated_bytes =
            support::relocated_bytes(&collision_prepared, "route collision")?;
        let step_relocated_bytes = support::relocated_bytes(&step_prepared, "queued step")?;
        let path_trampoline = path_prepared
            .trampoline_address()
            .map_err(InstallError::from)?;

        ROUTE_COLLISION_RESUME.store(
            collision_target.as_ptr() as usize + ROUTE_COLLISION_CALL.len(),
            Ordering::Release,
        );
        QUEUED_STEP_RESUME.store(
            step_target.as_ptr() as usize + QUEUED_STEP_CALL.len(),
            Ordering::Release,
        );
        CAN_MOVE_ADDRESS.store(module + MAP_CAN_MOVE_DIRECTION_RVA, Ordering::Release);
        WALK_ADDRESS.store(module + WALK_RVA, Ordering::Release);
        RESET_ADDRESS.store(module + RESET_MOVEMENT_RVA, Ordering::Release);

        let mut collision_detour = match support::install_prepared(
            &mut collision_prepared,
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
        if let Some(warning) = collision_detour
            .take_resume_warning()
            .map(support::detour_error)
        {
            return Ok(Self::partial(
                None,
                Some(collision_detour),
                None,
                path_relocated_bytes,
                collision_relocated_bytes,
                step_relocated_bytes,
                warning,
            ));
        }

        let mut step_detour = match support::install_prepared(
            &mut step_prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                return Ok(Self::partial(
                    None,
                    Some(collision_detour),
                    None,
                    path_relocated_bytes,
                    collision_relocated_bytes,
                    step_relocated_bytes,
                    io::Error::other(format!("queued-step hook installation failed: {error}")),
                ));
            }
        };
        if let Some(warning) = step_detour.take_resume_warning().map(support::detour_error) {
            return Ok(Self::partial(
                None,
                Some(collision_detour),
                Some(step_detour),
                path_relocated_bytes,
                collision_relocated_bytes,
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
                    Some(collision_detour),
                    Some(step_detour),
                    path_relocated_bytes,
                    collision_relocated_bytes,
                    step_relocated_bytes,
                    io::Error::other(format!("path-builder hook installation failed: {error}")),
                ));
            }
        };
        let install_warning = path_detour.take_resume_warning().map(support::detour_error);
        Ok(Self {
            path_detour: Some(path_detour),
            collision_detour: Some(collision_detour),
            step_detour: Some(step_detour),
            path_relocated_bytes,
            collision_relocated_bytes,
            step_relocated_bytes,
            install_warning,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn partial(
        path_detour: Option<InstalledDetour>,
        collision_detour: Option<InstalledDetour>,
        step_detour: Option<InstalledDetour>,
        path_relocated_bytes: u8,
        collision_relocated_bytes: u8,
        step_relocated_bytes: u8,
        warning: io::Error,
    ) -> Self {
        Self {
            path_detour,
            collision_detour,
            step_detour,
            path_relocated_bytes,
            collision_relocated_bytes,
            step_relocated_bytes,
            install_warning: Some(warning),
        }
    }

    pub(crate) const fn relocated_bytes(&self) -> u8 {
        self.path_relocated_bytes
    }

    pub(crate) const fn collision_relocated_bytes(&self) -> u8 {
        self.collision_relocated_bytes
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
        changed |= uninstall_owned(&mut self.collision_detour)?;
        if self.path_detour.is_none()
            && self.step_detour.is_none()
            && self.collision_detour.is_none()
        {
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
    ROUTE_COLLISION_RESUME.store(0, Ordering::Release);
    QUEUED_STEP_RESUME.store(0, Ordering::Release);
    CAN_MOVE_ADDRESS.store(0, Ordering::Release);
    WALK_ADDRESS.store(0, Ordering::Release);
    RESET_ADDRESS.store(0, Ordering::Release);
}

#[unsafe(naked)]
unsafe extern "thiscall" fn route_collision_detour(
    _world: *mut c_void,
    _y: i32,
    _x: i32,
    _direction: u8,
    _mode: u8,
) -> usize {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "call {combined}",
        "lock dec dword ptr [{activity}]",
        "jmp dword ptr [{resume}]",
        activity = sym ROUTE_COLLISION_HOOK_ACTIVITY,
        combined = sym combined_route_can_move,
        resume = sym ROUTE_COLLISION_RESUME,
    );
}

unsafe extern "thiscall" fn combined_route_can_move(
    world: *mut c_void,
    y: i32,
    x: i32,
    direction: u8,
    _mode: u8,
) -> usize {
    panic::catch_unwind(|| {
        let address = CAN_MOVE_ADDRESS.load(Ordering::Acquire);
        if address == 0 || world.is_null() {
            return 0;
        }
        // SAFETY: the exact client fingerprint fixes this RVA and ABI. The
        // native caller supplies its validated live WorldPane and coordinates.
        let can_move: CanMoveFn = unsafe { mem::transmute(address) };
        // SAFETY: the same native invariants hold for both supported collision
        // modes. Mode 1 preserves live changes and known dynamic occupants.
        if unsafe { can_move(world, y, x, direction, 1) } == 0 {
            return 0;
        }
        // SAFETY: mode 0 adds collision from complete raw map storage, so
        // off-screen statics are not mistaken for empty cells.
        unsafe { can_move(world, y, x, direction, 0) }
    })
    .unwrap_or(0)
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
        let reset_mode = crate::actions::movement::queued_step_reset_mode(world, result != 0);
        #[cfg(test)]
        let reset_mode = None;
        if let Some(mode) = reset_mode {
            // SAFETY: the failed native step left the same complete WorldPane
            // live on the main thread. Mode 0 fully resets a ground route;
            // mode 1 preserves the native pursuit retry generation.
            let reset: ResetFn = unsafe { mem::transmute(reset_address) };
            unsafe { reset(world, mode) };
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
            crate::route::observe(world, darpc_win32::pipe::sender_tick_ms());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::support;
    use darpc_game_client::{
        BUILD_BREADTH_FIRST_PATH_ENTRY, QUEUED_STEP_CALL, ROUTE_COLLISION_CALL,
    };
    use std::ptr::NonNull;

    #[test]
    fn validates_all_path_control_sites() {
        for (label, expected) in [
            ("path entry", BUILD_BREADTH_FIRST_PATH_ENTRY.as_slice()),
            ("collision call", ROUTE_COLLISION_CALL.as_slice()),
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
}
