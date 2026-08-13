use super::{module_base, read};
use crate::hooks::path::{CanMoveFn, native_edge_allowed};
use crate::process_memory::ProcessValue;
use crate::route_retry;
use darpc_game_client::{
    ADVANCE_PATH_RVA, BUILD_PATH_RVA, MAP_CAN_MOVE_DIRECTION_RVA, RESET_MOVEMENT_RVA,
    ROUTE_STEP_PUSH_BACK_RVA, SELF_OBJECT_RVA, TURN_RVA, WALK_RVA, WORLD_ENTITY_INTERACTION_RVA,
    WORLD_PANE_ADJUSTMENT, WORLD_PANE_MAP_ID_OFFSET, WORLD_PANE_POINTER_RVA,
    WORLD_PANE_ROUTE_ACTIVE_OFFSET, WORLD_PANE_ROUTE_STEP_COUNT_OFFSET,
    WORLD_PANE_ROUTE_VECTOR_OFFSET,
};
use darpc_model::{Direction, MovementUpdate, TilePosition, WalkMode};
use darpc_protocol::{CommandFailure, PathExclusions, RouteTile, WalkRoute};
use std::{
    ffi::c_void,
    mem,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicUsize, Ordering},
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
type InteractFn = unsafe extern "thiscall" fn(*mut c_void, u32) -> usize;
type PushRouteStepFn = unsafe extern "thiscall" fn(*mut c_void, *const PathRouteStep) -> usize;

#[derive(Clone, Copy)]
#[repr(C)]
struct PathRouteStep {
    direction: u8,
    reserved: [u8; 3],
    source_y: i32,
    source_x: i32,
}

const _: () = assert!(mem::size_of::<PathRouteStep>() == 12);

static HAS_ROUTE_DESTINATION: AtomicBool = AtomicBool::new(false);
static ROUTE_MODE: AtomicU8 = AtomicU8::new(0);
static ROUTE_DESTINATION_X: AtomicI32 = AtomicI32::new(0);
static ROUTE_DESTINATION_Y: AtomicI32 = AtomicI32::new(0);
static REPLAN_PENDING: AtomicBool = AtomicBool::new(false);
static REPLAN_WORLD: AtomicUsize = AtomicUsize::new(0);
static REPLAN_DUE_TICK: AtomicU32 = AtomicU32::new(0);
static REPLAN_DEADLINE_TICK: AtomicU32 = AtomicU32::new(0);
static REPLAN_ATTEMPT: AtomicU32 = AtomicU32::new(0);
static ROUTE_PROGRESS_VALID: AtomicBool = AtomicBool::new(false);
static ROUTE_PROGRESS_X: AtomicI32 = AtomicI32::new(0);
static ROUTE_PROGRESS_Y: AtomicI32 = AtomicI32::new(0);
static ROUTE_PROGRESS_TICK: AtomicU32 = AtomicU32::new(0);
static POSITION_SYNC_REPLAN: AtomicBool = AtomicBool::new(false);

pub(super) fn turn(direction: Direction) -> Result<(), CommandFailure> {
    clear_route_destination();
    Movement::resolve()?.turn(direction)
}

pub(super) fn walk(direction: Direction) -> Result<(), CommandFailure> {
    clear_route_destination();
    Movement::resolve()?.walk(direction)
}

pub(super) fn walk_to(x: i32, y: i32) -> Result<(), CommandFailure> {
    clear_route_destination();
    let result = Movement::resolve()?.walk_to(x, y);
    if result.is_ok() {
        set_route_destination(x, y, WalkMode::NativeRoute);
    }
    result
}

pub(super) fn walk_route(route: WalkRoute) -> Result<(), CommandFailure> {
    clear_route_destination();
    Movement::resolve()?.walk_route(route)
}

pub(super) fn set_path_exclusions(exclusions: PathExclusions) -> Result<(), CommandFailure> {
    let current_map_id = Movement::resolve()
        .and_then(|movement| movement.map_id())
        .ok();
    let update = crate::path_exclusions::replace(exclusions, current_map_id)
        .map_err(|()| CommandFailure::InvalidDestination)?;
    if let Some(update) = update {
        crate::state::observe_path_exclusions(update, darpc_win32::pipe::sender_tick_ms());
    }
    Ok(())
}

pub(super) fn remove_path_exclusions(map_id: u32) -> Result<(), CommandFailure> {
    if map_id > u32::from(u16::MAX) {
        return Err(CommandFailure::InvalidDestination);
    }
    let current_map_id = Movement::resolve()
        .and_then(|movement| movement.map_id())
        .ok();
    if let Some(update) = crate::path_exclusions::remove(map_id, current_map_id) {
        crate::state::observe_path_exclusions(update, darpc_win32::pipe::sender_tick_ms());
    }
    Ok(())
}

pub(super) fn clear_path_exclusions() -> Result<(), CommandFailure> {
    if let Some(update) = crate::path_exclusions::clear() {
        crate::state::observe_path_exclusions(update, darpc_win32::pipe::sender_tick_ms());
    }
    Ok(())
}

pub(super) fn interact(id: std::num::NonZeroU32) -> Result<(), CommandFailure> {
    let movement = Movement::resolve()?;
    let self_id = local_object_id()?;
    if self_id == id.get() || crate::state::target_position(id.get()).is_none() {
        return Err(CommandFailure::InvalidTarget);
    }
    // SAFETY: exact client validation fixes this RVA and ABI. The target was
    // resolved from the current main-thread object cache.
    let accepted = unsafe {
        let interact: InteractFn =
            mem::transmute(movement.module_base + WORLD_ENTITY_INTERACTION_RVA);
        interact(movement.world.as_ptr(), id.get())
    } != 0;
    if accepted {
        Ok(())
    } else {
        Err(CommandFailure::Rejected)
    }
}

pub(super) fn validate_tile(x: i32, y: i32) -> Result<(u16, u16), CommandFailure> {
    let movement = Movement::resolve()?;
    let width = movement.read_world::<i32>(MAP_WIDTH_OFFSET)?;
    let height = movement.read_world::<i32>(MAP_HEIGHT_OFFSET)?;
    if x < 0 || y < 0 || x >= width || y >= height {
        return Err(CommandFailure::InvalidDestination);
    }
    let x = u16::try_from(x).map_err(|_| CommandFailure::InvalidDestination)?;
    let y = u16::try_from(y).map_err(|_| CommandFailure::InvalidDestination)?;
    Ok((x, y))
}

pub(super) fn local_object_id() -> Result<u32, CommandFailure> {
    let local = Movement::resolve()?.self_object()?;
    read::<u32>(local.as_ptr() as usize + 0x24).ok_or(CommandFailure::InvalidState)
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
    ROUTE_MODE.store(0, Ordering::Release);
    POSITION_SYNC_REPLAN.store(false, Ordering::Release);
    ROUTE_PROGRESS_VALID.store(false, Ordering::Release);
    ROUTE_PROGRESS_TICK.store(0, Ordering::Release);
    clear_replan();
}

fn clear_replan() {
    clear_pending_replan();
    REPLAN_DEADLINE_TICK.store(0, Ordering::Release);
    REPLAN_ATTEMPT.store(0, Ordering::Release);
}

fn clear_pending_replan() {
    REPLAN_PENDING.store(false, Ordering::Release);
    REPLAN_WORLD.store(0, Ordering::Release);
    REPLAN_DUE_TICK.store(0, Ordering::Release);
}

pub(crate) fn reset_tracking() {
    clear_route_destination();
}

pub(crate) fn schedule_position_sync_replan() {
    if HAS_ROUTE_DESTINATION.load(Ordering::Acquire) {
        POSITION_SYNC_REPLAN.store(true, Ordering::Release);
    }
}

pub(crate) fn observe_tick() {
    let Some(destination) = route_destination() else {
        return;
    };
    let tick_ms = darpc_win32::pipe::sender_tick_ms();
    let Ok(movement) = Movement::resolve() else {
        clear_route_destination();
        return;
    };
    let Ok(position) = movement.local_position() else {
        clear_route_destination();
        return;
    };
    if POSITION_SYNC_REPLAN.swap(false, Ordering::AcqRel) {
        if position == destination {
            movement.reset();
            clear_route_destination();
            return;
        }
        movement.reset();
        record_route_progress(position, tick_ms);
        start_replan(movement.world.as_ptr() as usize, tick_ms);
    }
    if !ROUTE_PROGRESS_VALID.load(Ordering::Acquire)
        || position.x != ROUTE_PROGRESS_X.load(Ordering::Relaxed)
        || position.y != ROUTE_PROGRESS_Y.load(Ordering::Relaxed)
    {
        record_route_progress(position, tick_ms);
        if let Some(deadline) =
            route_retry::deadline_after_progress(REPLAN_PENDING.load(Ordering::Acquire), tick_ms)
        {
            REPLAN_DEADLINE_TICK.store(deadline, Ordering::Relaxed);
            REPLAN_ATTEMPT.store(0, Ordering::Relaxed);
        } else {
            clear_replan();
        }
    }
    if position == destination {
        clear_replan();
        return;
    }

    if !REPLAN_PENDING.load(Ordering::Acquire) {
        if movement.route_active() != Some(true) {
            return;
        }
        let deadline = REPLAN_DEADLINE_TICK.load(Ordering::Acquire);
        if deadline != 0 && crate::wrapping_time::deadline_reached(tick_ms, deadline) {
            movement.reset();
            clear_route_destination();
            return;
        }
        if !route_retry::stalled(tick_ms, ROUTE_PROGRESS_TICK.load(Ordering::Acquire)) {
            return;
        }
        movement.reset();
        start_replan(movement.world.as_ptr() as usize, tick_ms);
    }

    if crate::wrapping_time::deadline_reached(tick_ms, REPLAN_DEADLINE_TICK.load(Ordering::Acquire))
    {
        clear_route_destination();
        return;
    }
    if movement.world.as_ptr() as usize != REPLAN_WORLD.load(Ordering::Acquire) {
        clear_route_destination();
        return;
    }
    if movement.route_active() == Some(true) {
        clear_route_destination();
        return;
    }
    if !crate::wrapping_time::deadline_reached(tick_ms, REPLAN_DUE_TICK.load(Ordering::Acquire)) {
        return;
    }

    REPLAN_PENDING.store(false, Ordering::Release);
    match movement.walk_to(destination.x, destination.y) {
        Ok(()) => {
            clear_pending_replan();
            ROUTE_PROGRESS_TICK.store(tick_ms, Ordering::Release);
        }
        Err(CommandFailure::NoPath) => schedule_next_replan(tick_ms),
        Err(_) => clear_route_destination(),
    }
}

pub(crate) fn is_replan_pending() -> bool {
    REPLAN_PENDING.load(Ordering::Acquire)
}

/// Records a queued-step result and returns the native reset mode to apply.
///
/// A successful step needs no reset. A failed pursuit step preserves its
/// native retry timer with mode 1. A failed ground route uses a full reset;
/// daRPC-owned routes also retain their goal for a next-tick replan.
pub(crate) fn queued_step_reset_mode(world: *mut c_void, direction: u8, moved: bool) -> Option<u8> {
    if moved || world.is_null() {
        return None;
    }
    let pursuit_target =
        read::<u32>(world as usize + darpc_game_client::WORLD_PANE_PURSUIT_TARGET_ID_OFFSET)
            .unwrap_or(0);
    let mode = if pursuit_target != 0 {
        WalkMode::Pursuit
    } else if ROUTE_MODE.load(Ordering::Acquire) == 2 {
        WalkMode::ExactRoute
    } else {
        WalkMode::NativeRoute
    };
    publish_obstruction(world, Direction::from_raw(direction), mode);
    if pursuit_target != 0 {
        return Some(1);
    }
    if HAS_ROUTE_DESTINATION.load(Ordering::Acquire) && mode != WalkMode::ExactRoute {
        start_replan(world as usize, darpc_win32::pipe::sender_tick_ms());
    }
    Some(0)
}

fn start_replan(world: usize, tick_ms: u32) {
    REPLAN_WORLD.store(world, Ordering::Relaxed);
    REPLAN_DUE_TICK.store(tick_ms, Ordering::Relaxed);
    if REPLAN_DEADLINE_TICK.load(Ordering::Relaxed) == 0 {
        REPLAN_DEADLINE_TICK.store(route_retry::deadline(tick_ms), Ordering::Relaxed);
        REPLAN_ATTEMPT.store(0, Ordering::Relaxed);
    }
    REPLAN_PENDING.store(true, Ordering::Release);
}

fn schedule_next_replan(tick_ms: u32) {
    let attempt = REPLAN_ATTEMPT.fetch_add(1, Ordering::AcqRel);
    REPLAN_DUE_TICK.store(
        tick_ms.wrapping_add(route_retry::delay_ms(attempt)),
        Ordering::Relaxed,
    );
    REPLAN_PENDING.store(true, Ordering::Release);
}

struct Movement {
    module_base: usize,
    world: NonNull<c_void>,
}

impl Movement {
    fn resolve() -> Result<Self, CommandFailure> {
        let module_base = module_base()?;
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
            publish_obstruction(self.world.as_ptr(), Some(direction), WalkMode::Direct);
            Err(CommandFailure::Rejected)
        }
    }

    fn walk_to(&self, x: i32, y: i32) -> Result<(), CommandFailure> {
        let width = self.read_world::<i32>(MAP_WIDTH_OFFSET)?;
        let height = self.read_world::<i32>(MAP_HEIGHT_OFFSET)?;
        if x < 0 || y < 0 || x >= width || y >= height {
            return Err(CommandFailure::InvalidDestination);
        }

        let position = self.local_position()?;
        self.reset();
        if position.x == x && position.y == y {
            return Ok(());
        }

        // SAFETY: exact client validation fixes this RVA and ABI. Coordinates
        // are checked against the live zero-based map dimensions above.
        let built =
            unsafe { self.build_path_fn()(self.world.as_ptr(), position.y, position.x, y, x, 1) }
                != 0;
        if !built {
            return Err(CommandFailure::NoPath);
        }
        // SAFETY: the successful native builder populated this WorldPane's
        // route, and execution remains on the client main thread.
        let advanced = unsafe { self.advance_fn()(self.world.as_ptr()) } != 0;
        if advanced {
            Ok(())
        } else {
            self.reset();
            Err(CommandFailure::Rejected)
        }
    }

    fn walk_route(&self, route: WalkRoute) -> Result<(), CommandFailure> {
        if crate::state::map_transition_pending() || self.map_id()? != route.map_id() {
            return Err(CommandFailure::InvalidState);
        }
        let width = self.read_world::<i32>(MAP_WIDTH_OFFSET)?;
        let height = self.read_world::<i32>(MAP_HEIGHT_OFFSET)?;
        let position = self.local_position()?;
        let tiles = route.tiles();
        let Some(first) = tiles.first() else {
            return Err(CommandFailure::InvalidDestination);
        };
        if i32::from(first.x) != position.x || i32::from(first.y) != position.y {
            return Err(CommandFailure::InvalidDestination);
        }

        let mut directions = [Direction::North; darpc_protocol::MAX_WALK_ROUTE_TILES - 1];
        for (index, tile) in tiles.iter().enumerate() {
            if i32::from(tile.x) >= width || i32::from(tile.y) >= height {
                return Err(CommandFailure::InvalidDestination);
            }
            if tiles[..index].contains(tile) {
                return Err(CommandFailure::InvalidDestination);
            }
            let Some(next) = tiles.get(index + 1) else {
                continue;
            };
            let direction =
                route_direction(*tile, *next).ok_or(CommandFailure::InvalidDestination)?;
            if !self.route_step_allowed(*tile, direction) {
                return Err(CommandFailure::Rejected);
            }
            directions[index] = direction;
        }

        self.reset();
        if tiles.len() == 1 {
            return Ok(());
        }
        for index in (0..tiles.len() - 1).rev() {
            let source = tiles[index];
            let step = PathRouteStep {
                direction: directions[index].raw(),
                reserved: [0; 3],
                source_y: i32::from(source.y),
                source_x: i32::from(source.x),
            };
            // SAFETY: the supported client fingerprint fixes the vector helper
            // and record layout. The receiver is this live WorldPane's native
            // route vector and execution is on the client main thread.
            unsafe {
                self.push_route_step_fn()(
                    self.world
                        .as_ptr()
                        .cast::<u8>()
                        .add(WORLD_PANE_ROUTE_VECTOR_OFFSET)
                        .cast(),
                    &step,
                )
            };
        }
        let step_count = u32::try_from(tiles.len() - 1).expect("bounded route length fits u32");
        // SAFETY: the live complete WorldPane layout is fixed by the supported
        // executable fingerprint and remains owned by the client main thread.
        unsafe {
            self.world
                .as_ptr()
                .cast::<u8>()
                .add(WORLD_PANE_ROUTE_STEP_COUNT_OFFSET)
                .cast::<u32>()
                .write_unaligned(step_count);
            self.world
                .as_ptr()
                .cast::<u8>()
                .add(WORLD_PANE_ROUTE_ACTIVE_OFFSET)
                .write(1);
        }
        let destination = tiles.last().expect("nonempty route was validated");
        set_route_destination(
            i32::from(destination.x),
            i32::from(destination.y),
            WalkMode::ExactRoute,
        );
        crate::route::observe(self.world.as_ptr(), darpc_win32::pipe::sender_tick_ms());
        // SAFETY: the route vector and count were installed above using the
        // client's native allocator and exact record layout.
        if unsafe { self.advance_fn()(self.world.as_ptr()) } != 0 {
            Ok(())
        } else {
            self.reset();
            clear_route_destination();
            Err(CommandFailure::Rejected)
        }
    }

    fn route_active(&self) -> Option<bool> {
        read::<u8>(self.world.as_ptr() as usize + WORLD_PANE_ROUTE_ACTIVE_OFFSET)
            .map(|active| active != 0)
    }

    fn local_position(&self) -> Result<TilePosition, CommandFailure> {
        let local = self.self_object()?;
        let y = read::<i32>(local.as_ptr() as usize + LOCAL_Y_OFFSET)
            .ok_or(CommandFailure::InvalidState)?;
        let x = read::<i32>(local.as_ptr() as usize + LOCAL_X_OFFSET)
            .ok_or(CommandFailure::InvalidState)?;
        Ok(TilePosition { x, y })
    }

    fn map_id(&self) -> Result<u32, CommandFailure> {
        self.read_world(WORLD_PANE_MAP_ID_OFFSET)
    }

    fn route_step_allowed(&self, source: RouteTile, direction: Direction) -> bool {
        // SAFETY: the supported client fingerprint fixes this helper and ABI.
        // Coordinates were checked against the live map dimensions above. The
        // native helper takes x before y even though route records store y
        // before x.
        let can_move = self.can_move_fn();
        // SAFETY: this live WorldPane and the in-map source coordinates meet
        // the native collision helper's requirements.
        unsafe {
            native_edge_allowed(
                can_move,
                self.world.as_ptr(),
                i32::from(source.x),
                i32::from(source.y),
                direction.raw(),
            )
        }
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

    fn read_world<T: ProcessValue>(&self, offset: usize) -> Result<T, CommandFailure> {
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

    fn can_move_fn(&self) -> CanMoveFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA
        // and its x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + MAP_CAN_MOVE_DIRECTION_RVA) }
    }

    fn push_route_step_fn(&self) -> PushRouteStepFn {
        // SAFETY: the supported executable fingerprint fixes the function RVA,
        // 12-byte route record layout, and x86 thiscall signature.
        unsafe { mem::transmute(self.module_base + ROUTE_STEP_PUSH_BACK_RVA) }
    }
}

fn route_direction(source: RouteTile, destination: RouteTile) -> Option<Direction> {
    match (
        i32::from(destination.x) - i32::from(source.x),
        i32::from(destination.y) - i32::from(source.y),
    ) {
        (0, -1) => Some(Direction::North),
        (1, 0) => Some(Direction::East),
        (0, 1) => Some(Direction::South),
        (-1, 0) => Some(Direction::West),
        _ => None,
    }
}

fn set_route_destination(x: i32, y: i32, mode: WalkMode) {
    ROUTE_DESTINATION_X.store(x, Ordering::Relaxed);
    ROUTE_DESTINATION_Y.store(y, Ordering::Relaxed);
    ROUTE_MODE.store(
        match mode {
            WalkMode::NativeRoute => 1,
            WalkMode::ExactRoute => 2,
            WalkMode::Direct | WalkMode::Pursuit => 0,
        },
        Ordering::Relaxed,
    );
    HAS_ROUTE_DESTINATION.store(true, Ordering::Release);
    if let Ok(position) = Movement::resolve().and_then(|movement| movement.local_position()) {
        record_route_progress(position, darpc_win32::pipe::sender_tick_ms());
    }
}

fn publish_obstruction(world: *mut c_void, direction: Option<Direction>, mode: WalkMode) {
    let (Some(world), Some(direction)) = (NonNull::new(world), direction) else {
        return;
    };
    let Ok(module_base) = module_base() else {
        return;
    };
    let movement = Movement { module_base, world };
    let (Ok(map_id), Ok(current)) = (movement.map_id(), movement.local_position()) else {
        return;
    };
    let Some(attempted) = step_position(current, direction) else {
        return;
    };
    crate::state::observe_movement(
        MovementUpdate::Obstructed {
            map_id,
            current,
            attempted,
            direction,
            destination: route_destination(),
            mode,
        },
        darpc_win32::pipe::sender_tick_ms(),
    );
}

fn step_position(position: TilePosition, direction: Direction) -> Option<TilePosition> {
    let (x, y) = match direction {
        Direction::North => (position.x, position.y.checked_sub(1)?),
        Direction::East => (position.x.checked_add(1)?, position.y),
        Direction::South => (position.x, position.y.checked_add(1)?),
        Direction::West => (position.x.checked_sub(1)?, position.y),
    };
    Some(TilePosition { x, y })
}

fn record_route_progress(position: TilePosition, tick_ms: u32) {
    ROUTE_PROGRESS_X.store(position.x, Ordering::Relaxed);
    ROUTE_PROGRESS_Y.store(position.y, Ordering::Relaxed);
    ROUTE_PROGRESS_TICK.store(tick_ms, Ordering::Relaxed);
    ROUTE_PROGRESS_VALID.store(true, Ordering::Release);
}
