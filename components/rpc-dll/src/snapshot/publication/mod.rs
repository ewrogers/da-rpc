use super::convert;

use crate::atomic_sequence::next_nonzero;
use crate::dialog::RawDialog;
use crate::exchange::RawExchange;
use crate::legend::RawLegendState;
use crate::route::RawRoute;
use darpc_game_client::{RawObjects, RawStateSnapshot, StateReadError};
use darpc_model::ClientSnapshot;
use darpc_win32::pipe::sender_tick_ms;
use std::{
    cell::UnsafeCell,
    fmt,
    mem::{MaybeUninit, size_of},
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

const SLOT_EMPTY: u8 = 0;
const SLOT_WRITING: u8 = 1;
const SLOT_READY: u8 = 2;
const SLOT_READING: u8 = 3;
pub(super) const SNAPSHOT_BUFFER_BYTES: usize = 64 * 1024;

const _: () = assert!(size_of::<RawStateSnapshot>() <= SNAPSHOT_BUFFER_BYTES);

static LAST_WORLD_TOKEN: AtomicU32 = AtomicU32::new(0);
static WORLD_GENERATION: AtomicU32 = AtomicU32::new(0);
static SLOT: PublicationSlot = PublicationSlot::new();

pub(super) fn reset() {
    LAST_WORLD_TOKEN.store(0, Ordering::Release);
    WORLD_GENERATION.store(0, Ordering::Release);
    SLOT.reset();
}

pub(super) fn begin() -> Option<PublicationWriter<'static>> {
    SLOT.begin_write()
}

pub(super) fn read() -> Option<Publication> {
    let reader = SLOT.begin_read()?;
    let stored = reader.stored();
    Some(Publication {
        request_generation: stored.request_generation,
        result: stored.result.map(|ready| {
            convert::snapshot(
                ready,
                reader.raw(),
                reader.objects(),
                reader.dialog(),
                reader.exchange(),
                reader.legend(),
                reader.route(),
            )
        }),
    })
}

struct PublicationSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<StoredPublication>>,
    snapshot: UnsafeCell<SnapshotBuffer>,
    objects: UnsafeCell<RawObjects>,
    dialog: UnsafeCell<RawDialog>,
    exchange: UnsafeCell<RawExchange>,
    legend: UnsafeCell<RawLegendState>,
    route: UnsafeCell<RawRoute>,
}

// SAFETY: access to `value` is exclusively transferred between the main-thread
// writer and pipe-thread reader by the atomic state machine below.
unsafe impl Sync for PublicationSlot {}

impl PublicationSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
            snapshot: UnsafeCell::new(SnapshotBuffer::new()),
            objects: UnsafeCell::new(RawObjects::empty()),
            dialog: UnsafeCell::new(RawDialog::empty()),
            exchange: UnsafeCell::new(RawExchange::empty()),
            legend: UnsafeCell::new(RawLegendState::empty()),
            route: UnsafeCell::new(RawRoute::empty()),
        }
    }

    fn reset(&self) {
        self.state.store(SLOT_EMPTY, Ordering::Release);
    }

    fn begin_write(&self) -> Option<PublicationWriter<'_>> {
        loop {
            let state = self.state.load(Ordering::Acquire);
            if state == SLOT_READING {
                return None;
            }
            if !matches!(state, SLOT_EMPTY | SLOT_READY) {
                return None;
            }
            if self
                .state
                .compare_exchange(state, SLOT_WRITING, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            return Some(PublicationWriter {
                slot: self,
                finished: false,
            });
        }
    }

    fn begin_read(&self) -> Option<PublicationReader<'_>> {
        self.state
            .compare_exchange(
                SLOT_READY,
                SLOT_READING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()?;
        Some(PublicationReader { slot: self })
    }
}

pub(super) struct PublicationWriter<'a> {
    slot: &'a PublicationSlot,
    finished: bool,
}

impl PublicationWriter<'_> {
    pub(super) fn buffers(&mut self) -> (&mut RawStateSnapshot, &mut RawObjects) {
        // SAFETY: this writer owns the slot's WRITING state, which excludes the
        // IPC reader and any other writer until publication completes.
        unsafe {
            (
                &mut (*self.slot.snapshot.get()).raw,
                &mut *self.slot.objects.get(),
            )
        }
    }

    pub(super) fn publish_ready(mut self, request_generation: u32, duration_us: u32) {
        // SAFETY: this writer still owns exclusive access to both buffers.
        let raw = unsafe { &mut (*self.slot.snapshot.get()).raw };
        // SAFETY: this writer still owns exclusive access to both buffers.
        let objects = unsafe { &*self.slot.objects.get() };
        let previous = LAST_WORLD_TOKEN.swap(raw.world_token, Ordering::AcqRel);
        let world_generation = if previous == raw.world_token {
            WORLD_GENERATION.load(Ordering::Acquire)
        } else {
            next_nonzero(&WORLD_GENERATION)
        };
        let captured_tick_ms = sender_tick_ms();
        let boundary = crate::state::snapshot_boundary(raw, objects, captured_tick_ms);
        // SAFETY: publication runs on the main thread while this writer owns
        // the destination slot, so both dialog copies are stable.
        unsafe { crate::dialog::copy_current(&mut *self.slot.dialog.get()) };
        // SAFETY: publication runs on the main thread while this writer owns
        // the destination slot, so the exchange copy is stable.
        unsafe { crate::exchange::copy_current(&mut *self.slot.exchange.get()) };
        // SAFETY: publication runs on the main thread while this writer owns
        // the destination slot, so the legend copy is stable.
        unsafe { crate::legend::copy_current(&mut *self.slot.legend.get()) };
        // SAFETY: publication runs on the main thread while this writer owns
        // the destination slot, so the route copy is stable.
        unsafe { crate::route::copy_current(&mut *self.slot.route.get()) };
        self.finish(StoredPublication {
            request_generation,
            result: Ok(ReadyPublication {
                revision: boundary.revision,
                event_sequence: boundary.event_sequence,
                captured_tick_ms,
                updated_tick_ms: boundary.tick_ms,
                capture_duration_us: duration_us,
                world_generation,
            }),
        });
    }

    pub(super) fn publish_failed(mut self, request_generation: u32, failure: CaptureFailure) {
        self.finish(StoredPublication {
            request_generation,
            result: Err(failure),
        });
    }

    fn finish(&mut self, value: StoredPublication) {
        // SAFETY: this writer owns WRITING and the metadata is Copy, so an
        // unread older publication can be overwritten without a drop.
        unsafe { (*self.slot.value.get()).write(value) };
        self.slot.state.store(SLOT_READY, Ordering::Release);
        self.finished = true;
    }
}

impl Drop for PublicationWriter<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.slot.state.store(SLOT_EMPTY, Ordering::Release);
        }
    }
}

struct PublicationReader<'a> {
    slot: &'a PublicationSlot,
}

impl PublicationReader<'_> {
    fn stored(&self) -> StoredPublication {
        // SAFETY: this reader owns READING, and READY was published only after
        // the metadata and both buffers were fully written.
        unsafe { *(*self.slot.value.get()).assume_init_ref() }
    }

    fn raw(&self) -> &RawStateSnapshot {
        // SAFETY: READING excludes the only writer for the guard's lifetime.
        unsafe { &(*self.slot.snapshot.get()).raw }
    }

    fn objects(&self) -> &RawObjects {
        // SAFETY: READING excludes the only writer for the guard's lifetime.
        unsafe { &*self.slot.objects.get() }
    }

    fn dialog(&self) -> RawDialog {
        // SAFETY: READING excludes the writer and RawDialog is Copy.
        unsafe { *self.slot.dialog.get() }
    }

    fn exchange(&self) -> RawExchange {
        // SAFETY: READING excludes the writer and RawExchange is Copy.
        unsafe { *self.slot.exchange.get() }
    }

    fn legend(&self) -> &RawLegendState {
        // SAFETY: READING excludes the writer for this reader's lifetime.
        unsafe { &*self.slot.legend.get() }
    }

    fn route(&self) -> &RawRoute {
        // SAFETY: READING excludes the writer for this reader's lifetime.
        unsafe { &*self.slot.route.get() }
    }
}

impl Drop for PublicationReader<'_> {
    fn drop(&mut self) {
        self.slot.state.store(SLOT_EMPTY, Ordering::Release);
    }
}

#[repr(C)]
struct SnapshotBuffer {
    raw: RawStateSnapshot,
    _reserve: [u8; SNAPSHOT_BUFFER_BYTES - size_of::<RawStateSnapshot>()],
}

impl SnapshotBuffer {
    const fn new() -> Self {
        Self {
            raw: RawStateSnapshot::empty(),
            _reserve: [0; SNAPSHOT_BUFFER_BYTES - size_of::<RawStateSnapshot>()],
        }
    }
}

const _: () = assert!(size_of::<SnapshotBuffer>() == SNAPSHOT_BUFFER_BYTES);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_storage_is_reserved_off_stack() {
        assert_eq!(size_of::<SnapshotBuffer>(), 64 * 1024);
        assert!(size_of::<StoredPublication>() <= 64);
    }
}

#[derive(Clone, Copy)]
struct StoredPublication {
    request_generation: u32,
    result: Result<ReadyPublication, CaptureFailure>,
}

#[derive(Clone, Copy)]
pub(super) struct ReadyPublication {
    pub(super) revision: u32,
    pub(super) event_sequence: u32,
    pub(super) captured_tick_ms: u32,
    pub(super) updated_tick_ms: u32,
    pub(super) capture_duration_us: u32,
    pub(super) world_generation: u32,
}

pub(super) struct Publication {
    pub(super) request_generation: u32,
    pub(super) result: Result<ClientSnapshot, CaptureFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureFailure {
    WrongThread,
    PointersChanged,
    InvalidObjectTree,
    InvalidCollection,
    InvalidPaneList,
    InvalidGroupState,
    UnreadableMemory,
    AddressOverflow,
}

impl From<StateReadError> for CaptureFailure {
    fn from(error: StateReadError) -> Self {
        match error {
            StateReadError::WrongThread { .. } => Self::WrongThread,
            StateReadError::PointersChanged => Self::PointersChanged,
            StateReadError::InvalidObjectTree => Self::InvalidObjectTree,
            StateReadError::InvalidCollection => Self::InvalidCollection,
            StateReadError::InvalidPaneList => Self::InvalidPaneList,
            StateReadError::InvalidGroupState => Self::InvalidGroupState,
            StateReadError::UnreadableMemory { .. } => Self::UnreadableMemory,
            StateReadError::AddressOverflow => Self::AddressOverflow,
        }
    }
}

impl fmt::Display for CaptureFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongThread => formatter.write_str("snapshot ran outside the client main thread"),
            Self::PointersChanged => {
                formatter.write_str("client state pointers changed during capture")
            }
            Self::InvalidObjectTree => formatter.write_str("client object tree validation failed"),
            Self::InvalidCollection => formatter.write_str("client collection validation failed"),
            Self::InvalidPaneList => formatter.write_str("client event pane validation failed"),
            Self::InvalidGroupState => formatter.write_str("client group state validation failed"),
            Self::UnreadableMemory => formatter.write_str("client memory validation failed"),
            Self::AddressOverflow => formatter.write_str("client address arithmetic overflowed"),
        }
    }
}
