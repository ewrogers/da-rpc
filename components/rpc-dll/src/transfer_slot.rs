use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

pub(crate) struct TransferSlot<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: the atomic state machine grants exclusive access to one producer or
// consumer at a time, and READY publishes the initialized value before transfer.
unsafe impl<T: Copy + Send> Sync for TransferSlot<T> {}

impl<T: Copy> TransferSlot<T> {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub(crate) fn try_write(&self, value: T) -> bool {
        if self
            .state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        // SAFETY: WRITING gives this producer exclusive ownership of the value.
        unsafe { (*self.value.get()).write(value) };
        self.state.store(READY, Ordering::Release);
        true
    }

    pub(crate) fn try_take(&self) -> Option<T> {
        self.state
            .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        // SAFETY: READING gives this consumer exclusive ownership, and READY
        // guarantees that the producer initialized the Copy value.
        let value = unsafe { (*self.value.get()).assume_init_read() };
        self.state.store(EMPTY, Ordering::Release);
        Some(value)
    }

    pub(crate) fn discard(&self) {
        let _ = self
            .state
            .compare_exchange(READY, EMPTY, Ordering::AcqRel, Ordering::Acquire);
    }

    pub(crate) fn reset(&self) {
        self.state.store(EMPTY, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::TransferSlot;

    #[test]
    fn transfers_one_value_at_a_time() {
        let slot = TransferSlot::new();
        assert!(slot.try_write(7));
        assert!(!slot.try_write(8));
        assert_eq!(slot.try_take(), Some(7));
        assert_eq!(slot.try_take(), None);
        assert!(slot.try_write(8));
    }

    #[test]
    fn discarded_values_release_the_slot() {
        let slot = TransferSlot::new();
        assert!(slot.try_write(7));
        slot.discard();
        assert_eq!(slot.try_take(), None);
        assert!(slot.try_write(8));
    }
}
