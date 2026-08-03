use darpc_game_client::{
    ADVANCE_PATH_RVA, BUILD_PATH_RVA, RESET_MOVEMENT_RVA, SELF_OBJECT_RVA, TURN_RVA, WALK_RVA,
    WORLD_PANE_ADJUSTMENT, WORLD_PANE_POINTER_RVA, WORLD_PANE_ROUTE_ACTIVE_OFFSET,
};
use darpc_model::{Direction, TilePosition};
use darpc_protocol::{CommandFailure, CommandKind, WalkTarget};
use std::{
    ffi::c_void,
    mem, ptr,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory, LibraryLoader::GetModuleHandleW,
    Threading::GetCurrentProcess,
};

const LOCAL_Y_OFFSET: usize = 0x40;
const LOCAL_X_OFFSET: usize = 0x44;
const MAP_WIDTH_OFFSET: usize = 0x1C4;
const MAP_HEIGHT_OFFSET: usize = 0x1C8;

type TurnFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;
type SelfObjectFn = unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void;
type WalkFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;
type ResetFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> usize;
type AdvanceFn = unsafe extern "thiscall" fn(*mut c_void) -> usize;
type BuildPathFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32, u8) -> usize;

static HAS_ROUTE_DESTINATION: AtomicBool = AtomicBool::new(false);
static ROUTE_DESTINATION_X: AtomicI32 = AtomicI32::new(0);
static ROUTE_DESTINATION_Y: AtomicI32 = AtomicI32::new(0);

pub(crate) fn execute(command: CommandKind) -> Result<(), CommandFailure> {
    match command {
        CommandKind::Diagnostic => Ok(()),
        CommandKind::Turn(direction) => Movement::resolve()?.turn(direction),
        CommandKind::Walk(WalkTarget::Direction(direction)) => Movement::resolve()?.walk(direction),
        CommandKind::Walk(WalkTarget::Destination { x, y }) => Movement::resolve()?.walk_to(x, y),
    }
}

pub(crate) fn is_walking() -> Option<bool> {
    Movement::resolve().ok()?.route_active()
}

pub(crate) fn route_destination() -> Option<TilePosition> {
    HAS_ROUTE_DESTINATION
        .load(Ordering::Acquire)
        .then(|| TilePosition {
            x: ROUTE_DESTINATION_X.load(Ordering::Relaxed),
            y: ROUTE_DESTINATION_Y.load(Ordering::Relaxed),
        })
}

pub(crate) fn clear_route_destination() {
    HAS_ROUTE_DESTINATION.store(false, Ordering::Release);
}

pub(crate) fn reset_tracking() {
    clear_route_destination();
}

struct Movement {
    module_base: usize,
    world: NonNull<c_void>,
}

impl Movement {
    fn resolve() -> Result<Self, CommandFailure> {
        // SAFETY: a null module name requests the executable module for the
        // current process and does not transfer ownership.
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        let module_base = module as usize;
        if module_base == 0 {
            return Err(CommandFailure::InvalidState);
        }
        let interface = read::<u32>(
            module_base
                .checked_add(WORLD_PANE_POINTER_RVA)
                .ok_or(CommandFailure::Internal)?,
        )
        .ok_or(CommandFailure::InvalidState)?;
        let world = usize::try_from(interface)
            .ok()
            .and_then(|address| address.checked_sub(WORLD_PANE_ADJUSTMENT))
            .and_then(|address| NonNull::new(address as *mut c_void))
            .ok_or(CommandFailure::InvalidState)?;
        read::<u32>(world.as_ptr() as usize).ok_or(CommandFailure::InvalidState)?;
        Ok(Self { module_base, world })
    }

    fn turn(&self, direction: Direction) -> Result<(), CommandFailure> {
        self.self_object()?;
        self.reset();
        // SAFETY: exact client validation fixes this RVA and ABI. The command
        // runs on the client main thread with the live complete WorldPane.
        unsafe { self.turn_fn()(self.world.as_ptr(), direction.raw()) };
        Ok(())
    }

    fn walk(&self, direction: Direction) -> Result<(), CommandFailure> {
        self.self_object()?;
        self.reset();
        // SAFETY: exact client validation fixes this RVA and ABI. The native
        // helper owns movement permission, collision, animation, and CMove.
        let accepted = unsafe { self.walk_fn()(self.world.as_ptr(), direction.raw()) } != 0;
        if accepted {
            Ok(())
        } else {
            Err(CommandFailure::Rejected)
        }
    }

    fn walk_to(&self, x: i32, y: i32) -> Result<(), CommandFailure> {
        let width = self.read_world::<i32>(MAP_WIDTH_OFFSET)?;
        let height = self.read_world::<i32>(MAP_HEIGHT_OFFSET)?;
        if x < 0 || y < 0 || x >= width || y >= height {
            return Err(CommandFailure::InvalidDestination);
        }

        let local = self.self_object()?;
        let self_y = read::<i32>(local.as_ptr() as usize + LOCAL_Y_OFFSET)
            .ok_or(CommandFailure::InvalidState)?;
        let self_x = read::<i32>(local.as_ptr() as usize + LOCAL_X_OFFSET)
            .ok_or(CommandFailure::InvalidState)?;
        self.reset();
        if self_x == x && self_y == y {
            return Ok(());
        }

        // SAFETY: exact client validation fixes this RVA and ABI. Coordinates
        // are checked against the live zero-based map dimensions above.
        let built =
            unsafe { self.build_path_fn()(self.world.as_ptr(), self_y, self_x, y, x, 1) } != 0;
        if !built {
            return Err(CommandFailure::NoPath);
        }
        // SAFETY: the successful native builder populated this WorldPane's
        // route, and execution remains on the client main thread.
        let advanced = unsafe { self.advance_fn()(self.world.as_ptr()) } != 0;
        if advanced {
            set_route_destination(x, y);
            Ok(())
        } else {
            self.reset();
            Err(CommandFailure::Rejected)
        }
    }

    fn route_active(&self) -> Option<bool> {
        read::<u8>(self.world.as_ptr() as usize + WORLD_PANE_ROUTE_ACTIVE_OFFSET)
            .map(|active| active != 0)
    }

    fn self_object(&self) -> Result<NonNull<c_void>, CommandFailure> {
        // SAFETY: exact client validation fixes this RVA and x86 thiscall ABI.
        // Commands run on the main thread, where the native object tree is
        // stable for the duration of this lookup.
        let object = unsafe { self.self_object_fn()(self.world.as_ptr()) };
        NonNull::new(object).ok_or(CommandFailure::InvalidState)
    }

    fn reset(&self) {
        // SAFETY: exact client validation fixes this RVA and ABI. Zero requests
        // the native full reset, including invalidating pursuit timers.
        unsafe { self.reset_fn()(self.world.as_ptr(), 0) };
    }

    fn read_world<T: Copy>(&self, offset: usize) -> Result<T, CommandFailure> {
        read(self.world.as_ptr() as usize + offset).ok_or(CommandFailure::InvalidState)
    }

    fn turn_fn(&self) -> TurnFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + TURN_RVA) }
    }

    fn self_object_fn(&self) -> SelfObjectFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + SELF_OBJECT_RVA) }
    }

    fn walk_fn(&self) -> WalkFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + WALK_RVA) }
    }

    fn reset_fn(&self) -> ResetFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + RESET_MOVEMENT_RVA) }
    }

    fn advance_fn(&self) -> AdvanceFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + ADVANCE_PATH_RVA) }
    }

    fn build_path_fn(&self) -> BuildPathFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + BUILD_PATH_RVA) }
    }
}

fn set_route_destination(x: i32, y: i32) {
    ROUTE_DESTINATION_X.store(x, Ordering::Relaxed);
    ROUTE_DESTINATION_Y.store(y, Ordering::Relaxed);
    HAS_ROUTE_DESTINATION.store(true, Ordering::Release);
}

fn read<T: Copy>(address: usize) -> Option<T> {
    let mut value = mem::MaybeUninit::<T>::uninit();
    let mut read = 0_usize;
    // SAFETY: the destination is valid for one T. ReadProcessMemory validates
    // the source range and reports failure rather than dereferencing it here.
    let succeeded = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            value.as_mut_ptr().cast(),
            mem::size_of::<T>(),
            &mut read,
        )
    };
    (succeeded != 0 && read == mem::size_of::<T>()).then(|| {
        // SAFETY: ReadProcessMemory initialized every byte of T on this branch,
        // and every T used here is an integer or pointer-sized plain value.
        unsafe { value.assume_init() }
    })
}
