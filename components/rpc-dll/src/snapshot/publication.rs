mod convert;

use darpc_game_client::{RawStateSnapshot, StateReadError};
use darpc_model::ClientSnapshot;
use darpc_win32::pipe::sender_tick_ms;
use std::{
    cell::UnsafeCell,
    fmt,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

const SLOT_EMPTY: u8 = 0;
const SLOT_WRITING: u8 = 1;
const SLOT_READY: u8 = 2;
const SLOT_READING: u8 = 3;

static SNAPSHOT_REVISION: AtomicU32 = AtomicU32::new(0);
static LAST_WORLD_TOKEN: AtomicU32 = AtomicU32::new(0);
static WORLD_GENERATION: AtomicU32 = AtomicU32::new(0);
static SLOT: PublicationSlot = PublicationSlot::new();

pub(super) fn reset() {
    SNAPSHOT_REVISION.store(0, Ordering::Release);
    LAST_WORLD_TOKEN.store(0, Ordering::Release);
    WORLD_GENERATION.store(0, Ordering::Release);
    SLOT.reset();
}

pub(super) fn publish_ready(request_generation: u32, duration_us: u32, raw: RawStateSnapshot) {
    let previous = LAST_WORLD_TOKEN.swap(raw.world_token, Ordering::AcqRel);
    let world_generation = if previous == raw.world_token {
        WORLD_GENERATION.load(Ordering::Acquire)
    } else {
        WORLD_GENERATION
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    };
    let revision = SNAPSHOT_REVISION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    SLOT.publish(StoredPublication {
        request_generation,
        result: Ok(ReadyPublication {
            revision,
            captured_tick_ms: sender_tick_ms(),
            capture_duration_us: duration_us,
            world_generation,
            raw,
        }),
    });
}

pub(super) fn publish_failed(request_generation: u32, failure: CaptureFailure) {
    SLOT.publish(StoredPublication {
        request_generation,
        result: Err(failure),
    });
}

pub(super) fn read() -> Option<Publication> {
    let stored = SLOT.take()?;
    Some(Publication {
        request_generation: stored.request_generation,
        result: stored.result.map(convert::snapshot),
    })
}

struct PublicationSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<StoredPublication>>,
}

// SAFETY: access to `value` is exclusively transferred between the main-thread
// writer and pipe-thread reader by the atomic state machine below.
unsafe impl Sync for PublicationSlot {}

impl PublicationSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn reset(&self) {
        self.state.store(SLOT_EMPTY, Ordering::Release);
    }

    fn publish(&self, value: StoredPublication) {
        loop {
            let state = self.state.load(Ordering::Acquire);
            if state == SLOT_READING {
                return;
            }
            if !matches!(state, SLOT_EMPTY | SLOT_READY) {
                continue;
            }
            if self
                .state
                .compare_exchange(state, SLOT_WRITING, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            // SAFETY: the successful transition to WRITING gives this thread
            // exclusive access. Stored publications contain only Copy data and
            // therefore overwriting an unread older value needs no drop.
            unsafe { (*self.value.get()).write(value) };
            self.state.store(SLOT_READY, Ordering::Release);
            return;
        }
    }

    fn take(&self) -> Option<StoredPublication> {
        self.state
            .compare_exchange(
                SLOT_READY,
                SLOT_READING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()?;
        // SAFETY: the successful transition to READING gives this thread
        // exclusive access to a fully initialized Copy value published before
        // the READY release store.
        let value = unsafe { (*self.value.get()).assume_init_read() };
        self.state.store(SLOT_EMPTY, Ordering::Release);
        Some(value)
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
    pub(super) captured_tick_ms: u32,
    pub(super) capture_duration_us: u32,
    pub(super) world_generation: u32,
    pub(super) raw: RawStateSnapshot,
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
            Self::UnreadableMemory => formatter.write_str("client memory validation failed"),
            Self::AddressOverflow => formatter.write_str("client address arithmetic overflowed"),
        }
    }
}
