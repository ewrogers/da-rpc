use crate::state::QueuedStateEvent;
use darpc_model::{CollectionBatch, SequenceNumber};
use darpc_protocol::EventPollResult;
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU32, Ordering},
};

pub(crate) struct EventQueue<const N: usize> {
    slots: [UnsafeCell<MaybeUninit<QueuedStateEvent>>; N],
    write_position: AtomicU32,
    read_position: AtomicU32,
    latest_sequence: AtomicU32,
    latest_dropped_sequence: AtomicU32,
}

// SAFETY: the client main thread is the only producer, the IPC worker is the
// only consumer, and slot ownership is transferred by the atomic positions.
unsafe impl<const N: usize> Sync for EventQueue<N> {}

impl<const N: usize> EventQueue<N> {
    pub(crate) const fn new() -> Self {
        assert!(N > 0);
        Self {
            slots: [const { UnsafeCell::new(MaybeUninit::uninit()) }; N],
            write_position: AtomicU32::new(0),
            read_position: AtomicU32::new(0),
            latest_sequence: AtomicU32::new(0),
            latest_dropped_sequence: AtomicU32::new(0),
        }
    }

    pub(crate) fn reset(&self) {
        self.write_position.store(0, Ordering::Release);
        self.read_position.store(0, Ordering::Release);
        self.latest_sequence.store(0, Ordering::Release);
        self.latest_dropped_sequence.store(0, Ordering::Release);
    }

    pub(crate) fn push(&self, event: QueuedStateEvent) -> bool {
        self.latest_sequence
            .store(event.sequence(), Ordering::Release);
        let write = self.write_position.load(Ordering::Relaxed);
        let read = self.read_position.load(Ordering::Acquire);
        if write.wrapping_sub(read) >= u32::try_from(N).expect("event queue capacity fits u32") {
            self.latest_dropped_sequence
                .store(event.sequence(), Ordering::Release);
            return false;
        }
        let slot = &self.slots[write as usize % N];
        // SAFETY: this producer owns the slot until the release store advances
        // write_position, and the capacity check prevents overwrite.
        unsafe { (*slot.get()).write(event) };
        self.write_position
            .store(write.wrapping_add(1), Ordering::Release);
        true
    }

    pub(crate) fn mark_resync_required(&self, missing_sequence: u32) {
        self.latest_sequence
            .store(missing_sequence, Ordering::Release);
        self.latest_dropped_sequence
            .store(missing_sequence, Ordering::Release);
    }

    pub(crate) fn take_after(&self, after_sequence: u32, max_events: usize) -> EventPollResult {
        let dropped = self.latest_dropped_sequence.load(Ordering::Acquire);
        let latest = self.latest_sequence.load(Ordering::Acquire);
        if SequenceNumber::new(dropped).is_after(SequenceNumber::new(after_sequence)) {
            return EventPollResult::ResyncRequired {
                missing_sequence: dropped,
                latest_sequence: latest,
            };
        }

        let mut events = Vec::with_capacity(max_events.min(N));
        let mut expected = SequenceNumber::new(after_sequence).next().get();
        while events.len() < max_events {
            let Some(next) = self.peek() else {
                break;
            };
            if !SequenceNumber::new(next.sequence()).is_after(SequenceNumber::new(after_sequence)) {
                if let Some(event) = self.pop() {
                    event.discard();
                }
                continue;
            }
            if next.sequence() != expected {
                return EventPollResult::ResyncRequired {
                    missing_sequence: expected,
                    latest_sequence: latest,
                };
            }
            if let Some((kind, batch)) = next.collection_batch() {
                let count = usize::from(batch.count);
                if batch.index != 0 || count == 0 || count > max_events {
                    return EventPollResult::ResyncRequired {
                        missing_sequence: expected,
                        latest_sequence: latest,
                    };
                }
                if events.len() + count > max_events {
                    break;
                }
                if self.available() < count {
                    break;
                }
                for index in 0..batch.count {
                    let event = self.pop().expect("complete collection batch is available");
                    if event.sequence() != expected
                        || event.collection_batch()
                            != Some((
                                kind,
                                CollectionBatch {
                                    index,
                                    count: batch.count,
                                },
                            ))
                    {
                        return EventPollResult::ResyncRequired {
                            missing_sequence: expected,
                            latest_sequence: latest,
                        };
                    }
                    expected = SequenceNumber::new(event.sequence()).next().get();
                    events.push(
                        event
                            .into_model()
                            .expect("collection events always have inline payloads"),
                    );
                }
            } else {
                let event = self.pop().expect("peeked event is available");
                expected = SequenceNumber::new(event.sequence()).next().get();
                let sequence = event.sequence();
                let Some(event) = event.into_model() else {
                    return EventPollResult::ResyncRequired {
                        missing_sequence: sequence,
                        latest_sequence: latest,
                    };
                };
                events.push(event);
            }
        }
        EventPollResult::Events(events)
    }

    pub(crate) fn discard_through(&self, snapshot_sequence: u32) {
        while self.peek().is_some_and(|event| {
            !SequenceNumber::new(event.sequence()).is_after(SequenceNumber::new(snapshot_sequence))
        }) {
            if let Some(event) = self.pop() {
                event.discard();
            }
        }
    }

    fn available(&self) -> usize {
        let read = self.read_position.load(Ordering::Relaxed);
        let write = self.write_position.load(Ordering::Acquire);
        usize::try_from(write.wrapping_sub(read)).unwrap_or(usize::MAX)
    }

    fn peek(&self) -> Option<QueuedStateEvent> {
        let read = self.read_position.load(Ordering::Relaxed);
        let write = self.write_position.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let slot = &self.slots[read as usize % N];
        // SAFETY: write_position's acquire load makes the initialized slot
        // visible. The sole consumer does not advance the slot while copying.
        Some(unsafe { *(*slot.get()).assume_init_ref() })
    }

    fn pop(&self) -> Option<QueuedStateEvent> {
        let read = self.read_position.load(Ordering::Relaxed);
        let write = self.write_position.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let slot = &self.slots[read as usize % N];
        // SAFETY: write_position's acquire load makes the initialized slot
        // visible, and this sole consumer owns it until advancing read_position.
        let event = unsafe { (*slot.get()).assume_init_read() };
        self.read_position
            .store(read.wrapping_add(1), Ordering::Release);
        Some(event)
    }
}
