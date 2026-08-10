#![cfg_attr(not(windows), allow(dead_code))]

use darpc_game_client::RawWorldObject;
use darpc_model::{
    CharacterProfileUpdate, EquipmentSlot, LegendIcon, LegendMark, Nation, PlayerEquipmentItem,
    PlayerIdentity, PlayerInspectionChanges, PlayerInspectionTrigger, PlayerProfile, PlayerUpdate,
    UserState, WorldObject,
};
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    ops::Range,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};

const RESPONSE_OPCODE: u8 = 0x34;
const REQUEST_OPCODE: u8 = 0x43;
const REQUEST_SUBTYPE: u8 = 1;
const BODY_CAPACITY: usize = u16::MAX as usize;
const PROFILE_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 4;
const PENDING_CAPACITY: usize = 64;
const ORIGIN_CAPACITY: usize = 16;
const ORIGIN_TTL_MS: u32 = 5_000;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;
const ORIGIN_USER: u8 = 1;
const ORIGIN_DARPC: u8 = 2;
const EQUIPMENT_SLOTS: [u8; 18] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 13, 15, 16, 17, 18,
];

#[unsafe(no_mangle)]
pub(crate) static INTERCEPT_PENDING: AtomicBool = AtomicBool::new(false);

static SUBMITTING: AtomicBool = AtomicBool::new(false);
static SUBMIT_ID: AtomicU32 = AtomicU32::new(0);
static SUBMIT_TRIGGER: AtomicU8 = AtomicU8::new(0);
static SUBMIT_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static SUBMIT_OBSERVED: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT_ID: AtomicU32 = AtomicU32::new(0);
static IN_FLIGHT_TICK: AtomicU32 = AtomicU32::new(0);
static NEXT_PROFILE_SLOT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
struct Origin {
    kind: u8,
    id: u32,
    trigger: PlayerInspectionTrigger,
    command_id: u32,
    tick_ms: u32,
}

const EMPTY_ORIGIN: Origin = Origin {
    kind: 0,
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
struct Pending {
    id: u32,
    trigger: PlayerInspectionTrigger,
    command_id: u32,
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

#[derive(Clone, Copy)]
struct RawPlayerEvent {
    player: RawWorldObject,
    length: usize,
    changes: PlayerInspectionChanges,
    trigger: PlayerInspectionTrigger,
    tick_ms: u32,
}

struct EventSlot {
    state: AtomicU8,
    event: UnsafeCell<MaybeUninit<RawPlayerEvent>>,
    body: UnsafeCell<[u8; BODY_CAPACITY]>,
}

// SAFETY: state transfers exclusive ownership between the main-thread producer
// and the IPC consumer.
unsafe impl Sync for EventSlot {}

impl EventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            event: UnsafeCell::new(MaybeUninit::uninit()),
            body: UnsafeCell::new([0; BODY_CAPACITY]),
        }
    }
}

static EVENTS: [EventSlot; EVENT_CAPACITY] = [const { EventSlot::new() }; EVENT_CAPACITY];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedPlayer(u8);

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawText {
    bytes: [u8; u8::MAX as usize],
    length: u8,
}

impl RawText {
    const fn empty() -> Self {
        Self {
            bytes: [0; u8::MAX as usize],
            length: 0,
        }
    }

    fn from_bytes(value: &[u8]) -> Option<Self> {
        let mut text = Self::empty();
        text.length = u8::try_from(value.len()).ok()?;
        text.bytes[..value.len()].copy_from_slice(value);
        Some(text)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawIdentity {
    nation: u8,
    title: RawText,
    guild_rank: RawText,
    display_class: RawText,
    guild: RawText,
}

impl RawIdentity {
    const fn empty() -> Self {
        Self {
            nation: 0,
            title: RawText::empty(),
            guild_rank: RawText::empty(),
            display_class: RawText::empty(),
            guild: RawText::empty(),
        }
    }
}

struct IdentityCache {
    sequence: AtomicU32,
    available: AtomicBool,
    value: UnsafeCell<RawIdentity>,
}

// SAFETY: the client main thread is the sole writer and readers use the seqlock.
unsafe impl Sync for IdentityCache {}

static IDENTITY: IdentityCache = IdentityCache {
    sequence: AtomicU32::new(0),
    available: AtomicBool::new(false),
    value: UnsafeCell::new(RawIdentity::empty()),
};

#[derive(Clone, Copy)]
struct RawIdentityEvent {
    previous: Option<RawIdentity>,
    current: RawIdentity,
}

struct IdentityEventSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<RawIdentityEvent>>,
}

// SAFETY: state transfers exclusive ownership between producer and consumer.
unsafe impl Sync for IdentityEventSlot {}

impl IdentityEventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

static IDENTITY_EVENTS: [IdentityEventSlot; 4] = [const { IdentityEventSlot::new() }; 4];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedCharacterProfile(u8);

#[derive(Clone)]
struct Sections {
    id: u32,
    equipment: Range<usize>,
    user_state: usize,
    info: Range<usize>,
    legend: Range<usize>,
}

pub(crate) fn reset() {
    SUBMITTING.store(false, Ordering::Release);
    SUBMIT_ID.store(0, Ordering::Release);
    SUBMIT_TRIGGER.store(0, Ordering::Release);
    SUBMIT_COMMAND_ID.store(0, Ordering::Release);
    SUBMIT_OBSERVED.store(false, Ordering::Release);
    IN_FLIGHT_ID.store(0, Ordering::Release);
    IN_FLIGHT_TICK.store(0, Ordering::Release);
    INTERCEPT_PENDING.store(false, Ordering::Release);
    // SAFETY: reset runs outside the installed producer lifecycle.
    unsafe {
        (*ORIGINS.0.get()).count = 0;
        (*PENDING.0.get()).count = 0;
    }
    for slot in &PROFILES {
        slot.id.store(0, Ordering::Release);
        slot.length.store(0, Ordering::Release);
        slot.sequence.store(0, Ordering::Release);
    }
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
    IDENTITY.available.store(false, Ordering::Release);
    IDENTITY.sequence.store(0, Ordering::Release);
    for slot in &IDENTITY_EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn appeared(player: RawWorldObject) {
    let RawWorldObject::Player { id, .. } = player else {
        return;
    };
    clear_profile(id);
    enqueue(Pending {
        id,
        trigger: PlayerInspectionTrigger::Appeared,
        command_id: 0,
    });
}

#[cfg(not(test))]
pub(crate) fn request(command_id: u32, id: u32) -> Result<(), darpc_protocol::CommandFailure> {
    if crate::state::observed_player(id).is_none() {
        return Err(darpc_protocol::CommandFailure::InvalidTarget);
    }
    // SAFETY: commands execute on the same client main thread as the producer.
    let origins = unsafe { &mut *ORIGINS.0.get() };
    if let Some(origin) = origins.entries[..origins.count]
        .iter_mut()
        .find(|origin| origin.kind == ORIGIN_DARPC && origin.id == id)
    {
        origin.trigger = PlayerInspectionTrigger::Manual;
        origin.command_id = command_id;
        return Ok(());
    }
    // SAFETY: commands and packet observation share the client main thread.
    let pending = unsafe { &mut *PENDING.0.get() };
    if let Some(item) = pending.entries[..pending.count]
        .iter_mut()
        .find(|item| item.id == id)
    {
        item.trigger = PlayerInspectionTrigger::Manual;
        item.command_id = command_id;
        return Ok(());
    }
    enqueue(Pending {
        id,
        trigger: PlayerInspectionTrigger::Manual,
        command_id,
    });
    Ok(())
}

pub(crate) fn removed(id: u32) {
    clear_profile(id);
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

pub(crate) fn cleared() {
    for slot in &PROFILES {
        slot.sequence.fetch_add(1, Ordering::AcqRel);
        slot.id.store(0, Ordering::Relaxed);
        slot.length.store(0, Ordering::Relaxed);
        slot.sequence.fetch_add(1, Ordering::Release);
    }
    // SAFETY: clear is observed on the client main thread.
    unsafe { (*PENDING.0.get()).count = 0 };
}

pub(crate) fn observe_tick(tick_ms: u32) {
    prune_origins(tick_ms);
    let in_flight = IN_FLIGHT_ID.load(Ordering::Acquire);
    if in_flight != 0 {
        if tick_ms.wrapping_sub(IN_FLIGHT_TICK.load(Ordering::Acquire)) > ORIGIN_TTL_MS {
            IN_FLIGHT_ID.store(0, Ordering::Release);
        } else {
            return;
        }
    }
    let Some(pending) = pop_pending() else {
        return;
    };
    if crate::state::observed_player(pending.id).is_none() {
        return;
    }
    let mut body = [REQUEST_OPCODE, REQUEST_SUBTYPE, 0, 0, 0, 0];
    body[2..].copy_from_slice(&pending.id.to_be_bytes());
    SUBMIT_ID.store(pending.id, Ordering::Release);
    SUBMIT_TRIGGER.store(trigger_raw(pending.trigger), Ordering::Release);
    SUBMIT_COMMAND_ID.store(pending.command_id, Ordering::Release);
    SUBMIT_OBSERVED.store(false, Ordering::Release);
    SUBMITTING.store(true, Ordering::Release);
    #[cfg(all(windows, not(test)))]
    let submitted = crate::actions::network::submit(&body).is_ok();
    #[cfg(any(not(windows), test))]
    let submitted = false;
    SUBMITTING.store(false, Ordering::Release);
    SUBMIT_ID.store(0, Ordering::Release);
    SUBMIT_COMMAND_ID.store(0, Ordering::Release);
    if submitted && SUBMIT_OBSERVED.load(Ordering::Acquire) {
        IN_FLIGHT_ID.store(pending.id, Ordering::Release);
        IN_FLIGHT_TICK.store(tick_ms, Ordering::Release);
    }
}

pub(crate) fn observe_request(id: u32, tick_ms: u32) {
    let internal = SUBMITTING.load(Ordering::Acquire) && SUBMIT_ID.load(Ordering::Acquire) == id;
    let origin = if internal {
        SUBMIT_OBSERVED.store(true, Ordering::Release);
        Origin {
            kind: ORIGIN_DARPC,
            id,
            trigger: trigger_from_raw(SUBMIT_TRIGGER.load(Ordering::Acquire)),
            command_id: SUBMIT_COMMAND_ID.load(Ordering::Acquire),
            tick_ms,
        }
    } else {
        Origin {
            kind: ORIGIN_USER,
            id,
            trigger: PlayerInspectionTrigger::User,
            command_id: 0,
            tick_ms,
        }
    };
    push_origin(origin);
}

pub(crate) fn intercept_response(body: &[u8], tick_ms: u32) -> bool {
    if body.len() < 5 || body[0] != RESPONSE_OPCODE {
        return false;
    }
    let id = u32::from_be_bytes(body[1..5].try_into().expect("checked object-info header"));
    let Some(origin) = take_origin(id, tick_ms) else {
        return false;
    };
    if origin.kind != ORIGIN_DARPC {
        return false;
    }
    if IN_FLIGHT_ID.load(Ordering::Acquire) == id {
        IN_FLIGHT_ID.store(0, Ordering::Release);
    }
    if scan(body).is_some() && observe_response(body, tick_ms, origin.trigger) {
        if origin.command_id != 0 {
            crate::commands::complete_player(origin.command_id);
        }
    } else if origin.command_id != 0 {
        crate::commands::fail_player(origin.command_id);
    }
    let _ = origin.command_id;
    true
}

pub(crate) fn observe_user_response(body: &[u8], tick_ms: u32) {
    if body.first() == Some(&RESPONSE_OPCODE) {
        observe_response(body, tick_ms, PlayerInspectionTrigger::User);
    }
}

pub(crate) fn profile(id: u32) -> Option<PlayerProfile> {
    let (body, tick_ms, _) = copy_profile(id)?;
    parse_profile(&body, tick_ms)
}

pub(crate) fn inspected_player(id: u32) -> Option<WorldObject> {
    let (body, tick_ms, raw) = copy_profile(id)?;
    let profile = parse_profile(&body, tick_ms)?;
    let mut player = crate::objects::object_model(raw);
    match &mut player {
        WorldObject::Player {
            profile: current, ..
        } => *current = Some(Box::new(profile)),
        _ => return None,
    }
    Some(player)
}

pub(crate) fn take(queued: QueuedPlayer) -> Option<PlayerUpdate> {
    let slot = EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive access to event metadata.
    let raw = unsafe { (*slot.event.get()).assume_init_read() };
    // SAFETY: READING prevents producer reuse until the bounded copy completes.
    let body = unsafe { (&*slot.body.get())[..raw.length].to_vec() };
    slot.state.store(EMPTY, Ordering::Release);
    let profile = parse_profile(&body, raw.tick_ms)?;
    let mut player = crate::objects::object_model(raw.player);
    match &mut player {
        WorldObject::Player {
            profile: current, ..
        } => *current = Some(Box::new(profile)),
        _ => return None,
    }
    Some(PlayerUpdate {
        player,
        changes: raw.changes,
        trigger: raw.trigger,
    })
}

pub(crate) fn release(queued: QueuedPlayer) {
    if let Some(slot) = EVENTS.get(usize::from(queued.0)) {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn observe_self_look(body: &[u8], tick_ms: u32) {
    let Some(current) = parse_self_identity(body) else {
        return;
    };
    let previous = raw_identity();
    if previous.as_ref() == Some(&current) {
        return;
    }
    IDENTITY.sequence.fetch_add(1, Ordering::AcqRel);
    // SAFETY: self-look observation is the sole writer while the sequence is odd.
    unsafe { *IDENTITY.value.get() = current };
    IDENTITY.available.store(true, Ordering::Relaxed);
    IDENTITY.sequence.fetch_add(1, Ordering::Release);
    queue_identity(RawIdentityEvent { previous, current }, tick_ms);
}

pub(crate) fn self_identity() -> Option<PlayerIdentity> {
    raw_identity().and_then(identity_model)
}

pub(crate) fn take_identity(queued: QueuedCharacterProfile) -> Option<CharacterProfileUpdate> {
    let slot = IDENTITY_EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive ownership.
    let raw = unsafe { (*slot.value.get()).assume_init_read() };
    slot.state.store(EMPTY, Ordering::Release);
    Some(CharacterProfileUpdate {
        previous: raw.previous.and_then(identity_model),
        current: identity_model(raw.current)?,
    })
}

pub(crate) fn release_identity(queued: QueuedCharacterProfile) {
    if let Some(slot) = IDENTITY_EVENTS.get(usize::from(queued.0)) {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

fn observe_response(body: &[u8], tick_ms: u32, trigger: PlayerInspectionTrigger) -> bool {
    let Some(sections) = scan(body) else {
        return false;
    };
    let Some(player) = crate::state::observed_player(sections.id) else {
        return false;
    };
    let changes = previous_body(sections.id)
        .and_then(|previous| scan(previous).map(|old| changes(previous, &old, body, &sections)))
        .unwrap_or_else(PlayerInspectionChanges::all);
    publish_profile(sections.id, player, body, tick_ms);
    queue_player(player, body, changes, trigger, tick_ms);
    true
}

fn changes(
    old_body: &[u8],
    old: &Sections,
    body: &[u8],
    new: &Sections,
) -> PlayerInspectionChanges {
    PlayerInspectionChanges {
        info: old_body[old.user_state] != body[new.user_state]
            || old_body[old.info.clone()] != body[new.info.clone()],
        equipment: old_body[old.equipment.clone()] != body[new.equipment.clone()],
        legend: old_body[old.legend.clone()] != body[new.legend.clone()],
    }
}

fn queue_player(
    player: RawWorldObject,
    body: &[u8],
    changes: PlayerInspectionChanges,
    trigger: PlayerInspectionTrigger,
    tick_ms: u32,
) {
    let Some((index, slot)) = EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }) else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives the producer exclusive slot access.
    unsafe {
        (&mut *slot.body.get())[..body.len()].copy_from_slice(body);
        (*slot.event.get()).write(RawPlayerEvent {
            player,
            length: body.len(),
            changes,
            trigger,
            tick_ms,
        });
    }
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedPlayer(u8::try_from(index).expect("player event index fits u8"));
    if !crate::state::observe_player(queued, tick_ms) {
        release(queued);
    }
}

fn queue_identity(update: RawIdentityEvent, tick_ms: u32) {
    let Some((index, slot)) = IDENTITY_EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }) else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives the producer exclusive slot ownership.
    unsafe { (*slot.value.get()).write(update) };
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedCharacterProfile(u8::try_from(index).expect("identity event index fits u8"));
    if !crate::state::observe_character_profile(queued, tick_ms) {
        release_identity(queued);
    }
}

fn publish_profile(id: u32, player: RawWorldObject, body: &[u8], tick_ms: u32) {
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

fn clear_profile(id: u32) {
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

fn copy_profile(id: u32) -> Option<(Vec<u8>, u32, RawWorldObject)> {
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

fn previous_body(id: u32) -> Option<&'static [u8]> {
    let slot = PROFILES
        .iter()
        .find(|slot| slot.id.load(Ordering::Acquire) == id)?;
    let length = slot.length.load(Ordering::Relaxed);
    (length != 0 && length <= BODY_CAPACITY).then(|| {
        // SAFETY: called by the sole writer before it mutates this slot.
        unsafe { &(&*slot.body.get())[..length] }
    })
}

fn enqueue(value: Pending) {
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

fn pop_pending() -> Option<Pending> {
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

fn push_origin(origin: Origin) {
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

fn take_origin(id: u32, tick_ms: u32) -> Option<Origin> {
    prune_origins(tick_ms);
    // SAFETY: event interception is serialized on the client main thread.
    let queue = unsafe { &mut *ORIGINS.0.get() };
    let index = queue.entries[..queue.count]
        .iter()
        .position(|origin| origin.id == id)?;
    let origin = queue.entries[index];
    queue.entries.copy_within(index + 1..queue.count, index);
    queue.count -= 1;
    queue.entries[queue.count] = EMPTY_ORIGIN;
    update_intercept_pending(queue);
    Some(origin)
}

fn prune_origins(tick_ms: u32) {
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

fn scan(body: &[u8]) -> Option<Sections> {
    let mut reader = Reader::new(body);
    reader.expect(RESPONSE_OPCODE)?;
    let id = reader.u32_be()?;
    let equipment = reader.position..reader.position.checked_add(18 * 3)?;
    reader.take(18 * 3)?;
    let user_state = reader.position;
    reader.u8()?;
    reader.string8()?;
    let info_start = reader.position;
    Nation::from_raw(reader.u8()?)?;
    reader.string8()?;
    reader.u8()?;
    reader.string8()?;
    reader.string8()?;
    reader.string8()?;
    let info = info_start..reader.position;
    let legend_start = reader.position;
    let count = reader.u8()?;
    for _ in 0..count {
        reader.take(2)?;
        reader.string8()?;
        reader.string8()?;
    }
    let legend = legend_start..reader.position;
    let content_length = usize::from(reader.u16_be()?);
    if content_length != 0 {
        let portrait_length = usize::from(reader.u16_be()?);
        reader.take(portrait_length)?;
        reader.string16()?;
    }
    Some(Sections {
        id,
        equipment,
        user_state,
        info,
        legend,
    })
}

fn parse_profile(body: &[u8], inspected_tick_ms: u32) -> Option<PlayerProfile> {
    scan(body)?;
    let mut reader = Reader::new(body);
    reader.expect(RESPONSE_OPCODE)?;
    reader.u32_be()?;
    let mut equipment = Vec::with_capacity(18);
    for slot in EQUIPMENT_SLOTS {
        let sprite = reader.u16_be()?;
        let dye_color = reader.u8()?;
        if sprite != 0 {
            equipment.push(PlayerEquipmentItem {
                slot: EquipmentSlot::from_raw(slot)?,
                sprite,
                dye_color,
            });
        }
    }
    let user_state = UserState::from_raw(reader.u8()?);
    reader.string8()?;
    let nation = Nation::from_raw(reader.u8()?)?;
    let title = decode(reader.string8()?)?;
    let is_group_open = reader.u8()? != 0;
    let guild_rank = decode(reader.string8()?)?;
    let display_class = decode(reader.string8()?)?;
    let guild = decode(reader.string8()?)?;
    let count = reader.u8()?;
    let mut legend = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let icon = LegendIcon::from_raw(reader.u8()?);
        let color = reader.u8()?;
        let tag = decode(reader.string8()?)?;
        let text = decode(reader.string8()?)?;
        legend.push(LegendMark {
            icon,
            color,
            tag,
            text,
        });
    }
    Some(PlayerProfile {
        identity: PlayerIdentity {
            nation,
            title,
            guild_rank,
            display_class,
            guild,
        },
        user_state,
        is_group_open,
        equipment,
        legend,
        inspected_tick_ms,
    })
}

fn parse_self_identity(body: &[u8]) -> Option<RawIdentity> {
    let mut reader = Reader::new(body);
    reader.expect(0x39)?;
    let nation = reader.u8()?;
    Nation::from_raw(nation)?;
    let guild_rank = RawText::from_bytes(reader.string8()?)?;
    let title = RawText::from_bytes(reader.string8()?)?;
    reader.string8()?;
    reader.take(1)?;
    let recruiting = reader.u8()?;
    if recruiting == 1 {
        reader.string8()?;
        reader.string8()?;
        reader.string8()?;
        reader.take(12)?;
    }
    reader.take(3)?;
    let display_class = RawText::from_bytes(reader.string8()?)?;
    let guild = RawText::from_bytes(reader.string8()?)?;
    Some(RawIdentity {
        nation,
        title,
        guild_rank,
        display_class,
        guild,
    })
}

fn raw_identity() -> Option<RawIdentity> {
    loop {
        let before = IDENTITY.sequence.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }
        if !IDENTITY.available.load(Ordering::Relaxed) {
            return None;
        }
        // SAFETY: the value is copied and accepted only if the seqlock is stable.
        let value = unsafe { *IDENTITY.value.get() };
        if before == IDENTITY.sequence.load(Ordering::Acquire) {
            return Some(value);
        }
    }
}

fn identity_model(raw: RawIdentity) -> Option<PlayerIdentity> {
    Some(PlayerIdentity {
        nation: Nation::from_raw(raw.nation)?,
        title: decode(raw.title.as_bytes())?,
        guild_rank: decode(raw.guild_rank.as_bytes())?,
        display_class: decode(raw.display_class.as_bytes())?,
        guild: decode(raw.guild.as_bytes())?,
    })
}

#[cfg(windows)]
fn decode(value: &[u8]) -> Option<String> {
    crate::client_text::decode(value).or_else(|| value.is_empty().then(String::new))
}

#[cfg(not(windows))]
fn decode(value: &[u8]) -> Option<String> {
    Some(String::from_utf8_lossy(value).into_owned())
}

const fn trigger_raw(trigger: PlayerInspectionTrigger) -> u8 {
    match trigger {
        PlayerInspectionTrigger::Appeared => 0,
        PlayerInspectionTrigger::Manual => 1,
        PlayerInspectionTrigger::User => 2,
    }
}

const fn trigger_from_raw(value: u8) -> PlayerInspectionTrigger {
    match value {
        1 => PlayerInspectionTrigger::Manual,
        2 => PlayerInspectionTrigger::User,
        _ => PlayerInspectionTrigger::Appeared,
    }
}

struct Reader<'a> {
    body: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(body: &'a [u8]) -> Self {
        Self { body, position: 0 }
    }

    fn expect(&mut self, expected: u8) -> Option<()> {
        (self.u8()? == expected).then_some(())
    }

    fn u8(&mut self) -> Option<u8> {
        let value = *self.body.get(self.position)?;
        self.position += 1;
        Some(value)
    }

    fn u16_be(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32_be(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string8(&mut self) -> Option<&'a [u8]> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn string16(&mut self) -> Option<&'a [u8]> {
        let length = usize::from(self.u16_be()?);
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(length)?;
        let value = self.body.get(self.position..end)?;
        self.position = end;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn object_info() -> Vec<u8> {
        let mut body = vec![0x34, 0, 0, 0, 7];
        for slot in EQUIPMENT_SLOTS {
            body.extend_from_slice(&u16::from(slot).to_be_bytes());
            body.push(slot + 20);
        }
        body.extend_from_slice(&[4, 4]);
        body.extend_from_slice(b"Erik");
        body.push(4);
        body.extend_from_slice(&[6]);
        body.extend_from_slice(b"Mentor");
        body.push(1);
        body.extend_from_slice(&[6]);
        body.extend_from_slice(b"Leader");
        body.extend_from_slice(&[8]);
        body.extend_from_slice(b"Summoner");
        body.extend_from_slice(&[5]);
        body.extend_from_slice(b"Guild");
        body.extend_from_slice(&[1, 3, 7, 5]);
        body.extend_from_slice(b"Quest");
        body.extend_from_slice(&[4]);
        body.extend_from_slice(b"Done");
        body.extend_from_slice(&0_u16.to_be_bytes());
        body
    }

    #[test]
    fn parses_complete_other_player_profile_and_equipment_order() {
        let body = object_info();
        let profile = parse_profile(&body, 123).expect("valid object info");
        assert_eq!(profile.identity.nation, Nation::Mileth);
        assert_eq!(profile.identity.display_class, "Summoner");
        assert!(profile.is_group_open);
        assert_eq!(profile.equipment.len(), 18);
        assert_eq!(profile.equipment[12].slot, EquipmentSlot::Accessory1);
        assert_eq!(profile.equipment[13].slot, EquipmentSlot::Boots);
        assert_eq!(profile.legend[0].text, "Done");
    }

    #[test]
    fn rejects_every_truncated_object_info_prefix() {
        let body = object_info();
        for length in 0..body.len() {
            assert!(scan(&body[..length]).is_none(), "accepted {length} bytes");
        }
    }

    #[test]
    fn change_domains_compare_only_their_owned_fields() {
        let body = object_info();
        let sections = scan(&body).unwrap();
        let mut equipment = body.clone();
        equipment[sections.equipment.start] ^= 1;
        let equipment_sections = scan(&equipment).unwrap();
        assert_eq!(
            changes(&body, &sections, &equipment, &equipment_sections),
            PlayerInspectionChanges {
                info: false,
                equipment: true,
                legend: false,
            }
        );

        let mut legend = body.clone();
        legend[sections.legend.end - 1] ^= 1;
        let legend_sections = scan(&legend).unwrap();
        assert_eq!(
            changes(&body, &sections, &legend, &legend_sections),
            PlayerInspectionChanges {
                info: false,
                equipment: false,
                legend: true,
            }
        );
    }

    #[test]
    fn same_target_user_origin_precedes_internal_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_USER,
            id: 7,
            trigger: PlayerInspectionTrigger::User,
            command_id: 0,
            tick_ms: 10,
        });
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 11,
        });
        let body = object_info();
        assert!(!intercept_response(&body, 12));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(intercept_response(&body[..5], 13));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn expired_internal_origin_fails_open() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        assert!(!intercept_response(&object_info(), 10 + ORIGIN_TTL_MS + 1));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn treats_object_info_tail_marker_as_presence_only() {
        let mut body = object_info();
        body.truncate(body.len() - 2);
        body.extend_from_slice(&3_u16.to_be_bytes());
        body.extend_from_slice(&0_u16.to_be_bytes());
        body.extend_from_slice(&0_u16.to_be_bytes());
        assert!(scan(&body).is_some());
    }

    #[test]
    fn accepts_object_info_extension_bytes_after_known_fields() {
        let mut body = object_info();
        body.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        assert!(scan(&body).is_some());
    }

    #[test]
    fn parses_self_identity_with_recruiting_block() {
        let mut body = vec![0x39, 1, 4];
        body.extend_from_slice(b"Rank");
        body.extend_from_slice(&[5]);
        body.extend_from_slice(b"Title");
        body.extend_from_slice(&[0, 1, 1]);
        for value in [b"Lead".as_slice(), b"Team", b"Note"] {
            body.push(value.len() as u8);
            body.extend_from_slice(value);
        }
        body.extend_from_slice(&[1, 99]);
        body.extend_from_slice(&[0; 10]);
        body.extend_from_slice(&[3, 1, 1, 8]);
        body.extend_from_slice(b"Summoner");
        body.extend_from_slice(&[5]);
        body.extend_from_slice(b"Guild");
        let identity = identity_model(parse_self_identity(&body).unwrap()).unwrap();
        assert_eq!(identity.nation, Nation::Suomi);
        assert_eq!(identity.guild_rank, "Rank");
        assert_eq!(identity.title, "Title");
        assert_eq!(identity.display_class, "Summoner");
        assert_eq!(identity.guild, "Guild");
    }
}
