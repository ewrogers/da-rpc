use super::*;

pub(in crate::state) struct MainThreadCollections(UnsafeCell<CollectionTracker>);

// SAFETY: collection state is mutated only by the client main thread during
// active hooks or by lifecycle reset while hooks and the IPC consumer are down.
unsafe impl Sync for MainThreadCollections {}

impl MainThreadCollections {
    pub(in crate::state) const fn new() -> Self {
        Self(UnsafeCell::new(CollectionTracker::new()))
    }

    pub(in crate::state) unsafe fn reset(&self) {
        // SAFETY: the caller guarantees exclusive lifecycle access.
        unsafe { &mut *self.0.get() }.reset();
    }

    pub(in crate::state) unsafe fn replace(&self, raw: &RawStateSnapshot, tick_ms: u32) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.replace(raw, tick_ms);
    }

    pub(in crate::state) unsafe fn mark(&self, kind: CollectionKind, slot: u8, tick_ms: u32) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.mark(kind, slot, tick_ms);
    }

    pub(in crate::state) unsafe fn watch_cooldown(
        &self,
        kind: CollectionKind,
        slot: u8,
        tick_ms: u32,
    ) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.watch_cooldown(kind, slot, tick_ms);
    }

    pub(in crate::state) unsafe fn observe_tick(
        &self,
        tick_ms: u32,
        emit: impl FnMut(QueuedCollectionUpdate, u32),
    ) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.observe_tick(tick_ms, emit);
    }
}
