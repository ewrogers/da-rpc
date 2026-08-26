#![cfg_attr(not(windows), allow(dead_code))]

use darpc_model::{ActionSource, PlannedRoute, TilePosition};
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
    source: ActionSource,
    generation: u32,
    length: u32,
    tiles: [RawTile; MAX_PLANNED_ROUTE_TILES],
}

impl RawRoute {
    pub(crate) const fn empty() -> Self {
        Self {
            source: ActionSource::Unknown,
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

    fn destination(&self) -> Option<TilePosition> {
        self.available()
            .then(|| self.tiles[..self.length()].last())
            .flatten()
            .map(|tile| TilePosition {
                x: i32::from(tile.x),
                y: i32::from(tile.y),
            })
    }

    fn header(&self) -> Option<RouteHeader> {
        self.available().then(|| RouteHeader {
            generation: self.generation,
            step_count: self.length.saturating_sub(1),
        })
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
struct RouteHeader {
    generation: u32,
    step_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedRoute(u8);

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe {
        let current = &mut *CURRENT.0.get();
        current.source = ActionSource::Unknown;
        current.generation = 0;
        current.length = UNAVAILABLE_LENGTH;
    }
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn clear(tick_ms: u32) {
    // SAFETY: map transitions and route observations are serialized on the
    // client main thread.
    let changed = unsafe { clear_route(&mut *CURRENT.0.get()) };
    if !changed {
        return;
    }

    let Some((index, slot)) = claim_event() else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives this main-thread producer exclusive access to the
    // event buffer, and route snapshots are also copied on the main thread.
    unsafe { copy_route(&mut *slot.route.get(), &*CURRENT.0.get()) };
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedRoute(u8::try_from(index).expect("route event index fits u8"));
    if !crate::state::observe_route(queued, tick_ms) {
        release(queued);
    }
}

fn clear_route(route: &mut RawRoute) -> bool {
    if !route.available() || route.length == 0 {
        return false;
    }
    route.length = 0;
    true
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe_current(tick_ms: u32) {
    let Some(world) = world_pane() else {
        return;
    };
    // SAFETY: `world_pane` resolved the live WorldPane on the client main
    // thread, where native route mutation and observation are serialized.
    let Some(header) = (unsafe { read_header(world.cast::<u8>()) }) else {
        return;
    };
    // SAFETY: the client main thread is the sole current-route reader and
    // writer. The pipe worker only reads published event slots.
    if !tick_capture_required(unsafe { &*CURRENT.0.get() }, header) {
        return;
    }
    // SAFETY: the client main thread is the sole current-route reader.
    let source = unsafe { (&*CURRENT.0.get()).source };
    let _ = observe_with_header(world, tick_ms, header, source);
}

#[cfg(windows)]
pub(crate) fn observe(
    world: *const core::ffi::c_void,
    tick_ms: u32,
    source: ActionSource,
) -> Option<TilePosition> {
    if world.is_null() {
        return None;
    }
    // Path construction and exact-route installation deliberately bypass the
    // tick gate so a replacement with the same header still compares tiles.
    // SAFETY: callers provide the live receiver of a hooked native method or
    // a WorldPane resolved on the client main thread.
    let header = unsafe { read_header(world.cast::<u8>()) }?;
    observe_with_header(world, tick_ms, header, source)
}

#[cfg(windows)]
fn observe_with_header(
    world: *const core::ffi::c_void,
    tick_ms: u32,
    header: RouteHeader,
    source: ActionSource,
) -> Option<TilePosition> {
    let Some((index, slot)) = claim_event() else {
        crate::state::mark_resync_required();
        return None;
    };
    // SAFETY: WRITING gives the main-thread producer exclusive access to this
    // event buffer, and `world` is the live receiver of the hooked native
    // method or was resolved from the validated world-pane global.
    let captured = unsafe { capture(world.cast::<u8>(), header, &mut *slot.route.get()) };
    if !captured {
        slot.state.store(EMPTY, Ordering::Release);
        return None;
    }
    // SAFETY: WRITING still gives this producer exclusive access to the slot.
    unsafe { (&mut *slot.route.get()).source = source };
    // SAFETY: WRITING still gives this producer exclusive access to the
    // captured route until the slot is published below.
    let destination = unsafe { (&*slot.route.get()).destination() };
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
        return destination;
    }
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedRoute(u8::try_from(index).expect("route event index fits u8"));
    if !crate::state::observe_route(queued, tick_ms) {
        release(queued);
    }
    destination
}

fn tick_capture_required(current: &RawRoute, header: RouteHeader) -> bool {
    current.header() != Some(header)
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
        source: raw.source,
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
    left.source == right.source
        && left.length == right.length
        && left.generation == right.generation
        && (!left.available() || left.tiles[..left.length()] == right.tiles[..right.length()])
}

fn copy_route(output: &mut RawRoute, input: &RawRoute) {
    output.source = input.source;
    output.generation = input.generation;
    output.length = input.length;
    if input.available() {
        let length = input.length();
        output.tiles[..length].copy_from_slice(&input.tiles[..length]);
    }
}

#[cfg(windows)]
unsafe fn read_header(world: *const u8) -> Option<RouteHeader> {
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
    let step_count = unsafe {
        world
            .add(ROUTE_STEP_COUNT_OFFSET)
            .cast::<u32>()
            .read_unaligned()
    };
    let Ok(max_step_count) = u32::try_from(MAX_PLANNED_ROUTE_TILES) else {
        return None;
    };
    if step_count >= max_step_count {
        return None;
    }
    Some(RouteHeader {
        generation,
        step_count,
    })
}

#[cfg(windows)]
unsafe fn capture(world: *const u8, header: RouteHeader, output: &mut RawRoute) -> bool {
    let count = usize::try_from(header.step_count).expect("route step count fits usize");
    output.generation = header.generation;
    if count == 0 {
        output.length = 0;
        return true;
    }

    // SAFETY: the caller established a live complete WorldPane pointer. The
    // supported-client fingerprint fixes these vector fields.
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
        route.source = ActionSource::Command { command_id: 41 };
        route.generation = 7;
        route.length = 0;
        assert_eq!(
            model(&route),
            Some(PlannedRoute {
                source: ActionSource::Command { command_id: 41 },
                generation: 7,
                tiles: Vec::new(),
            })
        );
    }

    #[test]
    fn otherwise_identical_routes_are_distinguished_by_source() {
        let mut client = RawRoute::empty();
        client.source = ActionSource::Client;
        client.generation = 7;
        client.length = 0;
        let mut command = RawRoute::empty();
        copy_route(&mut command, &client);
        command.source = ActionSource::Command { command_id: 41 };

        assert!(!routes_equal(&client, &command));
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
        assert_eq!(output.destination(), Some(TilePosition { x: 6, y: 6 }));
    }

    #[test]
    fn clearing_a_route_retains_telemetry_with_no_tiles() {
        let mut route = RawRoute::empty();
        route.generation = 7;
        route.length = 1;
        route.tiles[0] = RawTile { x: 4, y: 5 };

        assert!(clear_route(&mut route));
        assert!(!clear_route(&mut route));

        assert_eq!(
            model(&route),
            Some(PlannedRoute {
                source: ActionSource::Unknown,
                generation: 7,
                tiles: Vec::new(),
            })
        );
    }

    #[test]
    fn tick_capture_is_skipped_when_the_route_header_is_unchanged() {
        let mut route = RawRoute::empty();
        route.generation = 7;
        route.length = 4;

        assert!(!tick_capture_required(
            &route,
            RouteHeader {
                generation: 7,
                step_count: 3,
            }
        ));
    }

    #[test]
    fn tick_capture_is_required_for_unavailable_changed_or_cleared_routes() {
        let unavailable = RawRoute::empty();
        let mut active = RawRoute::empty();
        active.generation = 7;
        active.length = 4;

        assert!(tick_capture_required(
            &unavailable,
            RouteHeader {
                generation: 7,
                step_count: 3,
            }
        ));
        assert!(tick_capture_required(
            &active,
            RouteHeader {
                generation: 8,
                step_count: 3,
            }
        ));
        assert!(tick_capture_required(
            &active,
            RouteHeader {
                generation: 7,
                step_count: 2,
            }
        ));
        assert!(tick_capture_required(
            &active,
            RouteHeader {
                generation: 7,
                step_count: 0,
            }
        ));

        active.length = 0;
        assert!(!tick_capture_required(
            &active,
            RouteHeader {
                generation: 7,
                step_count: 0,
            }
        ));
    }
}
