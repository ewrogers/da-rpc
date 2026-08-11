#![cfg_attr(not(windows), allow(dead_code))]

use darpc_model::{PlannedRoute, TilePosition};
use darpc_protocol::MAX_PLANNED_ROUTE_TILES;
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU8, Ordering},
};

const EVENT_CAPACITY: usize = 4;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;
const UNAVAILABLE_LENGTH: u32 = u32::MAX;

const ROUTE_START_OFFSET: usize = 0x2A8;
const ROUTE_END_OFFSET: usize = 0x2AC;
const ROUTE_CAPACITY_OFFSET: usize = 0x2B0;
const ROUTE_STEP_COUNT_OFFSET: usize = 0x2B8;
const ROUTE_GENERATION_OFFSET: usize = 0x2C8;
const STEP_BYTES: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
struct RawTile {
    x: u16,
    y: u16,
}

impl RawTile {
    const ZERO: Self = Self { x: 0, y: 0 };
}

pub(crate) struct RawRoute {
    generation: u32,
    length: u32,
    tiles: [RawTile; MAX_PLANNED_ROUTE_TILES],
}

impl RawRoute {
    pub(crate) const fn empty() -> Self {
        Self {
            generation: 0,
            length: UNAVAILABLE_LENGTH,
            tiles: [RawTile::ZERO; MAX_PLANNED_ROUTE_TILES],
        }
    }

    fn available(&self) -> bool {
        self.length != UNAVAILABLE_LENGTH
    }

    fn length(&self) -> usize {
        usize::try_from(self.length).expect("route length fits usize")
    }
}

struct CurrentRoute(UnsafeCell<RawRoute>);

// SAFETY: the client main thread is the sole current-route reader and writer.
unsafe impl Sync for CurrentRoute {}

struct EventSlot {
    state: AtomicU8,
    route: UnsafeCell<RawRoute>,
}

// SAFETY: state transfers exclusive ownership between the main-thread
// producer and IPC consumer.
unsafe impl Sync for EventSlot {}

impl EventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            route: UnsafeCell::new(RawRoute::empty()),
        }
    }
}

static CURRENT: CurrentRoute = CurrentRoute(UnsafeCell::new(RawRoute::empty()));
static EVENTS: [EventSlot; EVENT_CAPACITY] = [const { EventSlot::new() }; EVENT_CAPACITY];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedRoute(u8);

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe {
        let current = &mut *CURRENT.0.get();
        current.generation = 0;
        current.length = UNAVAILABLE_LENGTH;
    }
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe_current(tick_ms: u32) {
    let Some(world) = world_pane() else {
        return;
    };
    observe(world, tick_ms);
}

#[cfg(windows)]
pub(crate) fn observe(world: *const core::ffi::c_void, tick_ms: u32) {
    if world.is_null() {
        return;
    }
    let Some((index, slot)) = claim_event() else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives the main-thread producer exclusive access to this
    // event buffer, and `world` is the live receiver of the hooked native
    // method or was resolved from the validated world-pane global.
    let captured = unsafe { capture(world.cast::<u8>(), &mut *slot.route.get()) };
    if !captured {
        slot.state.store(EMPTY, Ordering::Release);
        return;
    }
    // SAFETY: route observation and current-route snapshot publication are
    // serialized on the client main thread.
    let changed = unsafe {
        let captured = &*slot.route.get();
        let current = &mut *CURRENT.0.get();
        if routes_equal(current, captured) {
            false
        } else {
            copy_route(current, captured);
            true
        }
    };
    if !changed {
        slot.state.store(EMPTY, Ordering::Release);
        return;
    }
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedRoute(u8::try_from(index).expect("route event index fits u8"));
    if !crate::state::observe_route(queued, tick_ms) {
        release(queued);
    }
}

fn claim_event() -> Option<(usize, &'static EventSlot)> {
    EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    })
}

pub(crate) fn take(queued: QueuedRoute) -> Option<PlannedRoute> {
    let slot = EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive access to the published
    // prefix until it releases the slot.
    let route = unsafe { model(&*slot.route.get()) };
    slot.state.store(EMPTY, Ordering::Release);
    route
}

pub(crate) fn release(queued: QueuedRoute) {
    if let Some(slot) = EVENTS.get(usize::from(queued.0)) {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn copy_current(output: &mut RawRoute) {
    // SAFETY: called by snapshot publication on the client main thread.
    let current = unsafe { &*CURRENT.0.get() };
    copy_route(output, current);
}

pub(crate) fn model(raw: &RawRoute) -> Option<PlannedRoute> {
    raw.available().then(|| PlannedRoute {
        generation: raw.generation,
        tiles: raw.tiles[..raw.length()]
            .iter()
            .map(|tile| TilePosition {
                x: i32::from(tile.x),
                y: i32::from(tile.y),
            })
            .collect(),
    })
}

fn routes_equal(left: &RawRoute, right: &RawRoute) -> bool {
    left.length == right.length
        && left.generation == right.generation
        && (!left.available() || left.tiles[..left.length()] == right.tiles[..right.length()])
}

fn copy_route(output: &mut RawRoute, input: &RawRoute) {
    output.generation = input.generation;
    output.length = input.length;
    if input.available() {
        let length = input.length();
        output.tiles[..length].copy_from_slice(&input.tiles[..length]);
    }
}

#[cfg(windows)]
unsafe fn capture(world: *const u8, output: &mut RawRoute) -> bool {
    // SAFETY: caller established a live complete WorldPane pointer. These
    // fields are fixed by the supported-client fingerprint and are copied
    // synchronously on the client main thread.
    let generation = unsafe {
        world
            .add(ROUTE_GENERATION_OFFSET)
            .cast::<u32>()
            .read_unaligned()
    };
    // SAFETY: same invariant as above.
    let count = unsafe {
        world
            .add(ROUTE_STEP_COUNT_OFFSET)
            .cast::<u32>()
            .read_unaligned()
    };
    let Ok(count) = usize::try_from(count) else {
        return false;
    };
    if count >= MAX_PLANNED_ROUTE_TILES {
        return false;
    }
    output.generation = generation;
    if count == 0 {
        output.length = 0;
        return true;
    }

    // SAFETY: same WorldPane layout invariant as above.
    let start = unsafe { world.add(ROUTE_START_OFFSET).cast::<u32>().read_unaligned() } as usize;
    // SAFETY: same WorldPane layout invariant as above.
    let end = unsafe { world.add(ROUTE_END_OFFSET).cast::<u32>().read_unaligned() } as usize;
    // SAFETY: same WorldPane layout invariant as above.
    let capacity = unsafe {
        world
            .add(ROUTE_CAPACITY_OFFSET)
            .cast::<u32>()
            .read_unaligned()
    } as usize;
    if start == 0 || end < start || capacity < end {
        return false;
    }
    let vector_bytes = end - start;
    if !vector_bytes.is_multiple_of(STEP_BYTES) {
        return false;
    }
    let vector_count = vector_bytes / STEP_BYTES;
    if vector_count >= MAX_PLANNED_ROUTE_TILES || count > vector_count {
        return false;
    }

    expand(count, output, |index| {
        let record = start + index * STEP_BYTES;
        // SAFETY: every requested record lies within the validated vector
        // prefix and is read synchronously on the owning main thread.
        Some(unsafe {
            (
                (record as *const u8).read(),
                (record as *const u8).add(8).cast::<i32>().read_unaligned(),
                (record as *const u8).add(4).cast::<i32>().read_unaligned(),
            )
        })
    })
}

fn expand(
    count: usize,
    output: &mut RawRoute,
    mut record: impl FnMut(usize) -> Option<(u8, i32, i32)>,
) -> bool {
    let Some((_, mut x, mut y)) = record(count - 1) else {
        return false;
    };
    let Some(tile) = raw_tile(x, y) else {
        return false;
    };
    output.tiles[0] = tile;
    for route_index in 0..count {
        let Some((direction, source_x, source_y)) = record(count - 1 - route_index) else {
            return false;
        };
        if source_x != x || source_y != y || !advance(&mut x, &mut y, direction) {
            return false;
        }
        let Some(tile) = raw_tile(x, y) else {
            return false;
        };
        output.tiles[route_index + 1] = tile;
    }
    output.length = u32::try_from(count + 1).expect("bounded route length fits u32");
    true
}

fn advance(x: &mut i32, y: &mut i32, direction: u8) -> bool {
    let next = match direction {
        0 => (*x, y.checked_sub(1)),
        1 => (x.checked_add(1).unwrap_or(i32::MIN), Some(*y)),
        2 => (*x, y.checked_add(1)),
        3 => (x.checked_sub(1).unwrap_or(i32::MIN), Some(*y)),
        _ => return false,
    };
    let Some(next_y) = next.1 else {
        return false;
    };
    if next.0 == i32::MIN {
        return false;
    }
    *x = next.0;
    *y = next_y;
    true
}

fn raw_tile(x: i32, y: i32) -> Option<RawTile> {
    Some(RawTile {
        x: u16::try_from(x).ok()?,
        y: u16::try_from(y).ok()?,
    })
}

#[cfg(all(windows, not(test)))]
fn world_pane() -> Option<*const core::ffi::c_void> {
    use darpc_game_client::{WORLD_PANE_ADJUSTMENT, WORLD_PANE_POINTER_RVA};
    use std::ptr;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    // SAFETY: a null module name requests the executable module.
    let module = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    let pointer_address = module.checked_add(WORLD_PANE_POINTER_RVA)?;
    // SAFETY: the supported executable fingerprint fixes this readable global.
    let interface = unsafe { (pointer_address as *const usize).read_unaligned() };
    interface
        .checked_sub(WORLD_PANE_ADJUSTMENT)
        .filter(|world| *world != 0)
        .map(|world| world as *const core::ffi::c_void)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_available_routes_are_distinct() {
        let mut route = RawRoute::empty();
        assert_eq!(model(&route), None);
        route.generation = 7;
        route.length = 0;
        assert_eq!(
            model(&route),
            Some(PlannedRoute {
                generation: 7,
                tiles: Vec::new(),
            })
        );
    }

    #[test]
    fn direction_expansion_uses_absolute_tiles() {
        let mut x = 10;
        let mut y = 20;
        for (direction, expected) in [(0, (10, 19)), (1, (11, 19)), (2, (11, 20)), (3, (10, 20))] {
            assert!(advance(&mut x, &mut y, direction));
            assert_eq!((x, y), expected);
        }
        assert!(!advance(&mut x, &mut y, 4));
    }

    #[test]
    fn expands_reverse_queue_and_only_its_remaining_prefix() {
        // The final record is the first step. A consumed record may remain in
        // the native vector after the remaining count decreases.
        let records = [(2, 6, 5), (1, 5, 5), (1, 4, 5), (0, 4, 6)];
        let mut output = RawRoute::empty();
        assert!(expand(3, &mut output, |index| records.get(index).copied()));
        assert_eq!(
            model(&output).unwrap().tiles,
            vec![
                TilePosition { x: 4, y: 5 },
                TilePosition { x: 5, y: 5 },
                TilePosition { x: 6, y: 5 },
                TilePosition { x: 6, y: 6 },
            ]
        );
    }
}
