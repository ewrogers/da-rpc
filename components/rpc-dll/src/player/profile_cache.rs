use super::*;

static NEXT_PROFILE_SLOT: AtomicUsize = AtomicUsize::new(0);

struct ProfileSlot {
    sequence: AtomicU32,
    id: AtomicU32,
    length: AtomicUsize,
    tick_ms: AtomicU32,
    player: UnsafeCell<MaybeUninit<RawWorldObject>>,
    body: UnsafeCell<[u8; BODY_CAPACITY]>,
}

// SAFETY: the client main thread is the sole writer. Readers use sequence as a
// seqlock and only consume a stable bounded copy.
unsafe impl Sync for ProfileSlot {}

impl ProfileSlot {
    const fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            id: AtomicU32::new(0),
            length: AtomicUsize::new(0),
            tick_ms: AtomicU32::new(0),
            player: UnsafeCell::new(MaybeUninit::uninit()),
            body: UnsafeCell::new([0; BODY_CAPACITY]),
        }
    }
}

static PROFILES: [ProfileSlot; PROFILE_CAPACITY] = [const { ProfileSlot::new() }; PROFILE_CAPACITY];

pub(super) fn publish_profile(id: u32, player: RawWorldObject, body: &[u8], tick_ms: u32) {
    let slot = PROFILES
        .iter()
        .find(|slot| slot.id.load(Ordering::Acquire) == id)
        .or_else(|| {
            PROFILES
                .iter()
                .find(|slot| slot.id.load(Ordering::Acquire) == 0)
        })
        .unwrap_or_else(|| {
            &PROFILES[NEXT_PROFILE_SLOT.fetch_add(1, Ordering::Relaxed) % PROFILE_CAPACITY]
        });
    slot.sequence.fetch_add(1, Ordering::AcqRel);
    // SAFETY: the client main thread is the sole writer and the sequence is odd.
    unsafe { (&mut *slot.body.get())[..body.len()].copy_from_slice(body) };
    // SAFETY: the sequence is odd and the client main thread is the sole writer.
    unsafe { (*slot.player.get()).write(player) };
    slot.length.store(body.len(), Ordering::Relaxed);
    slot.tick_ms.store(tick_ms, Ordering::Relaxed);
    slot.id.store(id, Ordering::Relaxed);
    slot.sequence.fetch_add(1, Ordering::Release);
}

pub(super) fn clear_profile(id: u32) {
    if let Some(slot) = PROFILES
        .iter()
        .find(|slot| slot.id.load(Ordering::Acquire) == id)
    {
        slot.sequence.fetch_add(1, Ordering::AcqRel);
        slot.id.store(0, Ordering::Relaxed);
        slot.length.store(0, Ordering::Relaxed);
        slot.sequence.fetch_add(1, Ordering::Release);
    }
}

pub(super) fn copy_profile(id: u32) -> Option<(Vec<u8>, u32, RawWorldObject)> {
    let slot = PROFILES
        .iter()
        .find(|slot| slot.id.load(Ordering::Acquire) == id)?;
    loop {
        let before = slot.sequence.load(Ordering::Acquire);
        if before & 1 != 0 || slot.id.load(Ordering::Relaxed) != id {
            std::hint::spin_loop();
            continue;
        }
        let length = slot.length.load(Ordering::Relaxed);
        let tick_ms = slot.tick_ms.load(Ordering::Relaxed);
        if length == 0 || length > BODY_CAPACITY {
            return None;
        }
        // SAFETY: the seqlock verifies this bounded copy was stable.
        let body = unsafe { (&*slot.body.get())[..length].to_vec() };
        // SAFETY: a nonzero published ID implies initialized player metadata,
        // and the seqlock verifies it was copied from the same publication.
        let player = unsafe { (*slot.player.get()).assume_init_read() };
        if before == slot.sequence.load(Ordering::Acquire) {
            return Some((body, tick_ms, player));
        }
    }
}

pub(super) fn previous_body(id: u32) -> Option<&'static [u8]> {
    let slot = PROFILES
        .iter()
        .find(|slot| slot.id.load(Ordering::Acquire) == id)?;
    let length = slot.length.load(Ordering::Relaxed);
    (length != 0 && length <= BODY_CAPACITY).then(|| {
        // SAFETY: called by the sole writer before it mutates this slot.
        unsafe { &(&*slot.body.get())[..length] }
    })
}

pub(super) fn reset() {
    NEXT_PROFILE_SLOT.store(0, Ordering::Release);
    for slot in &PROFILES {
        slot.id.store(0, Ordering::Release);
        slot.length.store(0, Ordering::Release);
        slot.sequence.store(0, Ordering::Release);
    }
}

pub(super) fn clear() {
    for slot in &PROFILES {
        slot.sequence.fetch_add(1, Ordering::AcqRel);
        slot.id.store(0, Ordering::Relaxed);
        slot.length.store(0, Ordering::Relaxed);
        slot.sequence.fetch_add(1, Ordering::Release);
    }
}
