use super::*;

const PENDING_CAPACITY: usize = 64;
const ORIGIN_CAPACITY: usize = 16;
pub(super) const IN_FLIGHT_TIMEOUT_MS: u32 = 5_000;
pub(super) const ORIGIN_TTL_MS: u32 = 30_000;
pub(super) const ORIGIN_USER: u8 = 1;
pub(super) const ORIGIN_DARPC: u8 = 2;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ResponseKind {
    ObjectInfo,
    SelfLook,
}

#[derive(Clone, Copy)]
pub(super) struct Origin {
    pub(super) kind: u8,
    pub(super) response: ResponseKind,
    pub(super) id: u32,
    pub(super) trigger: PlayerInspectionTrigger,
    pub(super) command_id: u32,
    pub(super) tick_ms: u32,
}

const EMPTY_ORIGIN: Origin = Origin {
    kind: 0,
    response: ResponseKind::ObjectInfo,
    id: 0,
    trigger: PlayerInspectionTrigger::User,
    command_id: 0,
    tick_ms: 0,
};

struct OriginQueue {
    entries: [Origin; ORIGIN_CAPACITY],
    count: usize,
}

struct OriginCell(UnsafeCell<OriginQueue>);

// SAFETY: outgoing observation, event interception, and tick work all run on
// the client main thread. INTERCEPT_PENDING is the only cross-hook read.
unsafe impl Sync for OriginCell {}

static ORIGINS: OriginCell = OriginCell(UnsafeCell::new(OriginQueue {
    entries: [EMPTY_ORIGIN; ORIGIN_CAPACITY],
    count: 0,
}));

#[derive(Clone, Copy)]
pub(super) struct Pending {
    pub(super) id: u32,
    pub(super) trigger: PlayerInspectionTrigger,
    pub(super) command_id: u32,
}

const EMPTY_PENDING: Pending = Pending {
    id: 0,
    trigger: PlayerInspectionTrigger::Appeared,
    command_id: 0,
};

struct PendingQueue {
    entries: [Pending; PENDING_CAPACITY],
    count: usize,
}

struct PendingCell(UnsafeCell<PendingQueue>);

// SAFETY: packet and tick callbacks are serialized on the client main thread.
unsafe impl Sync for PendingCell {}

static PENDING: PendingCell = PendingCell(UnsafeCell::new(PendingQueue {
    entries: [EMPTY_PENDING; PENDING_CAPACITY],
    count: 0,
}));

static IN_FLIGHT_ID: AtomicU32 = AtomicU32::new(0);
static IN_FLIGHT_TICK: AtomicU32 = AtomicU32::new(0);

pub(super) fn enqueue(value: Pending) {
    // SAFETY: packet and tick work is serialized on the main thread.
    let queue = unsafe { &mut *PENDING.0.get() };
    if IN_FLIGHT_ID.load(Ordering::Acquire) == value.id
        || queue.entries[..queue.count]
            .iter()
            .any(|item| item.id == value.id)
    {
        return;
    }
    if queue.count == PENDING_CAPACITY {
        return;
    }
    if value.trigger == PlayerInspectionTrigger::Manual {
        queue.entries.copy_within(0..queue.count, 1);
        queue.entries[0] = value;
    } else {
        queue.entries[queue.count] = value;
    }
    queue.count += 1;
}

#[cfg(not(test))]
pub(super) fn upgrade(id: u32, command_id: u32) -> bool {
    // SAFETY: commands execute on the same client main thread as the producer.
    let origins = unsafe { &mut *ORIGINS.0.get() };
    if let Some(origin) = origins.entries[..origins.count].iter_mut().find(|origin| {
        origin.kind == ORIGIN_DARPC
            && origin.response == ResponseKind::ObjectInfo
            && origin.id == id
    }) {
        origin.trigger = PlayerInspectionTrigger::Manual;
        origin.command_id = command_id;
        return true;
    }
    // SAFETY: commands and packet observation share the client main thread.
    let pending = unsafe { &mut *PENDING.0.get() };
    if let Some(item) = pending.entries[..pending.count]
        .iter_mut()
        .find(|item| item.id == id)
    {
        item.trigger = PlayerInspectionTrigger::Manual;
        item.command_id = command_id;
        return true;
    }
    false
}

pub(super) fn remove(id: u32) {
    // SAFETY: removal is observed on the client main thread.
    let pending = unsafe { &mut *PENDING.0.get() };
    let mut index = 0;
    while index < pending.count {
        if pending.entries[index].id == id {
            remove_pending(pending, index);
        } else {
            index += 1;
        }
    }
}

pub(super) fn clear_pending() {
    // SAFETY: clearing is observed on the client main thread.
    unsafe { (*PENDING.0.get()).count = 0 };
}

pub(super) fn ready_for_next(tick_ms: u32) -> bool {
    let in_flight = IN_FLIGHT_ID.load(Ordering::Acquire);
    if in_flight == 0 {
        return true;
    }
    if tick_ms.wrapping_sub(IN_FLIGHT_TICK.load(Ordering::Acquire)) <= IN_FLIGHT_TIMEOUT_MS {
        return false;
    }
    IN_FLIGHT_ID.store(0, Ordering::Release);
    true
}

pub(super) fn mark_in_flight(id: u32, tick_ms: u32) {
    IN_FLIGHT_ID.store(id, Ordering::Release);
    IN_FLIGHT_TICK.store(tick_ms, Ordering::Release);
}

pub(super) fn in_flight_id() -> Option<u32> {
    let id = IN_FLIGHT_ID.load(Ordering::Acquire);
    (id != 0).then_some(id)
}

pub(super) fn complete(id: u32) {
    if IN_FLIGHT_ID.load(Ordering::Acquire) == id {
        IN_FLIGHT_ID.store(0, Ordering::Release);
    }
}

pub(super) fn pop_pending() -> Option<Pending> {
    // SAFETY: tick work is the sole consumer on the main thread.
    let queue = unsafe { &mut *PENDING.0.get() };
    (queue.count != 0).then(|| {
        let value = queue.entries[0];
        remove_pending(queue, 0);
        value
    })
}

fn remove_pending(queue: &mut PendingQueue, index: usize) {
    queue.entries.copy_within(index + 1..queue.count, index);
    queue.count -= 1;
    queue.entries[queue.count] = EMPTY_PENDING;
}

pub(super) fn push_origin(origin: Origin) {
    // SAFETY: outgoing observation is serialized on the client main thread.
    let queue = unsafe { &mut *ORIGINS.0.get() };
    if queue.count == ORIGIN_CAPACITY {
        queue.entries.copy_within(1..queue.count, 0);
        queue.count -= 1;
    }
    queue.entries[queue.count] = origin;
    queue.count += 1;
    update_intercept_pending(queue);
}

pub(super) fn take_origin(response: ResponseKind, id: u32, tick_ms: u32) -> Option<Origin> {
    prune_origins(tick_ms);
    // SAFETY: event interception is serialized on the client main thread.
    let queue = unsafe { &mut *ORIGINS.0.get() };
    let index = queue.entries[..queue.count]
        .iter()
        .position(|origin| origin.response == response && origin.id == id)?;
    Some(remove_origin(queue, index))
}

pub(super) fn take_internal_origin(
    response: ResponseKind,
    id: u32,
    tick_ms: u32,
) -> Option<Origin> {
    prune_origins(tick_ms);
    // SAFETY: event observation is serialized on the client main thread.
    let queue = unsafe { &mut *ORIGINS.0.get() };
    let index = queue.entries[..queue.count].iter().position(|origin| {
        origin.kind == ORIGIN_DARPC && origin.response == response && origin.id == id
    })?;
    Some(remove_origin(queue, index))
}

fn remove_origin(queue: &mut OriginQueue, index: usize) -> Origin {
    let origin = queue.entries[index];
    queue.entries.copy_within(index + 1..queue.count, index);
    queue.count -= 1;
    queue.entries[queue.count] = EMPTY_ORIGIN;
    update_intercept_pending(queue);
    origin
}

pub(super) fn prune_origins(tick_ms: u32) {
    // SAFETY: tick/event work is serialized on the client main thread.
    let queue = unsafe { &mut *ORIGINS.0.get() };
    let mut index = 0;
    while index < queue.count {
        if tick_ms.wrapping_sub(queue.entries[index].tick_ms) > ORIGIN_TTL_MS {
            queue.entries.copy_within(index + 1..queue.count, index);
            queue.count -= 1;
            queue.entries[queue.count] = EMPTY_ORIGIN;
        } else {
            index += 1;
        }
    }
    update_intercept_pending(queue);
}

fn update_intercept_pending(queue: &OriginQueue) {
    INTERCEPT_PENDING.store(
        queue.entries[..queue.count]
            .iter()
            .any(|origin| origin.kind == ORIGIN_DARPC),
        Ordering::Release,
    );
}

pub(super) fn reset() {
    IN_FLIGHT_ID.store(0, Ordering::Release);
    IN_FLIGHT_TICK.store(0, Ordering::Release);
    // SAFETY: reset runs outside the installed producer lifecycle.
    unsafe {
        (*ORIGINS.0.get()).count = 0;
        (*PENDING.0.get()).count = 0;
    }
}
