use super::*;

pub(in crate::state) struct MainThreadObjects(UnsafeCell<ObjectCache>);

// SAFETY: access is restricted to the client main thread except during reset,
// which runs only while the producer hook is absent.
unsafe impl Sync for MainThreadObjects {}

impl MainThreadObjects {
    pub(in crate::state) const fn new() -> Self {
        Self(UnsafeCell::new(ObjectCache::empty()))
    }

    pub(in crate::state) unsafe fn replace(&self, objects: &RawObjects) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).replace(objects) };
    }

    pub(in crate::state) unsafe fn name(
        &self,
        id: u32,
    ) -> Option<([u8; darpc_game_client::MAX_OBJECT_NAME_BYTES], u8)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).name(id) }
    }

    pub(in crate::state) unsafe fn get(&self, id: u32) -> Option<RawWorldObject> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).get(id) }
    }

    pub(in crate::state) unsafe fn player_occupied(&self, x: i32, y: i32) -> bool {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).player_occupied(x, y) }
    }

    pub(in crate::state) unsafe fn refresh_player_occupancy(&self) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).refresh_player_occupancy() };
    }

    pub(in crate::state) unsafe fn remove_player_with_name(
        &self,
        observed: RawWorldObject,
    ) -> Option<u32> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).remove_player_with_name(observed) }
    }

    #[cfg(not(test))]
    pub(in crate::state) unsafe fn position(&self, id: u32) -> Option<(i32, i32)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).position(id) }
    }

    pub(in crate::state) unsafe fn draw(
        &self,
        object: RawWorldObject,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).upsert(object) }
    }

    pub(in crate::state) unsafe fn move_object(
        &self,
        id: u32,
        x: i32,
        y: i32,
        direction: Option<u8>,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_object(id, x, y, direction) }
    }

    pub(in crate::state) unsafe fn change_direction(
        &self,
        id: u32,
        direction: u8,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).change_direction(id, direction) }
    }

    pub(in crate::state) unsafe fn remove(&self, id: u32) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).remove(id) }
    }

    pub(in crate::state) unsafe fn move_self(
        &self,
        id: Option<u32>,
        x: i32,
        y: i32,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_self(id, x, y) }
    }

    pub(in crate::state) unsafe fn take_outside(
        &self,
        x: i32,
        y: i32,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).take_outside(x, y) }
    }

    pub(in crate::state) unsafe fn clear(&self) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).clear() }
    }
}
