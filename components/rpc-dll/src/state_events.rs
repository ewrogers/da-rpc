#![cfg_attr(not(windows), allow(dead_code))]

use crate::{
    collections::{CollectionTracker, QueuedCollectionUpdate},
    event_queue::EventQueue,
    message_packet::{ParsedMessage, Participant},
    object_packet::WorldUpdate,
    objects::{ObjectCache, QueuedObjectUpdate},
};
use darpc_game_client::{RawCharacter, RawModifiers, RawObjects, RawStateSnapshot, RawWorldObject};
use darpc_model::{
    CharacterModifiers, CharacterStats, ClientMessage, CollectionBatch, CollectionKind, CoreStatus,
    CurrentVitals, Effect, EffectDuration, EffectUpdate, Element, LocationUpdate, MapChange,
    MessageKind, ProgressionStatus, StateEvent, StateUpdate, StatusUpdate,
};
use darpc_protocol::EventPollResult;
#[cfg(windows)]
use darpc_win32::pipe::sender_tick_ms;
use std::{
    cell::UnsafeCell,
    mem::size_of,
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::{Duration, Instant},
};

pub(crate) const EVENT_QUEUE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_MAP_NAME_BYTES: usize = u8::MAX as usize;
const MAX_EVENT_MESSAGE_NAME_BYTES: usize = 15;
const MAX_EVENT_MESSAGE_TEXT_BYTES: usize = 256;
const EVENT_QUEUE_CAPACITY: usize = EVENT_QUEUE_BYTES / size_of::<QueuedStateEvent>();
const POLL_INTERVAL: Duration = Duration::from_millis(2);

static REVISION: AtomicU32 = AtomicU32::new(0);
static EVENT_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static CACHE: MainThreadCache = MainThreadCache::new();
static OBJECTS: MainThreadObjects = MainThreadObjects::new();
static COLLECTIONS: MainThreadCollections = MainThreadCollections::new();
static QUEUE: EventQueue<EVENT_QUEUE_CAPACITY> = EventQueue::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotBoundary {
    pub(crate) revision: u32,
    pub(crate) event_sequence: u32,
    pub(crate) tick_ms: u32,
}

pub(crate) fn reset() {
    REVISION.store(0, Ordering::Release);
    EVENT_SEQUENCE.store(0, Ordering::Release);
    QUEUE.reset();
    // SAFETY: reset runs before the event hook is installed and after the IPC
    // consumer has stopped, so no other thread accesses the cache.
    unsafe { CACHE.replace(StateCache::default()) };
    // SAFETY: reset has the same exclusive lifecycle access described above.
    unsafe { OBJECTS.clear() };
    // SAFETY: reset has the same exclusive lifecycle access described above.
    unsafe { COLLECTIONS.reset() };
}

#[must_use]
pub(crate) fn map_transition_pending() -> bool {
    // SAFETY: snapshot ticks and packet observations run on the client main
    // thread, which is the sole cache producer.
    unsafe { CACHE.map_transition_pending() }
}

pub(crate) fn stage_map_transition(map_id: u32, width: i32, height: i32, name: &[u8]) {
    // SAFETY: the map-size hook runs on the client main thread, which is the
    // sole cache producer.
    unsafe { CACHE.stage_map_transition(map_id, width, height, name) };
}

pub(crate) fn snapshot_boundary(
    raw: &RawStateSnapshot,
    objects: &RawObjects,
    tick_ms: u32,
) -> SnapshotBoundary {
    // SAFETY: snapshot capture and event observation both run on the client
    // main thread, so cache mutation cannot overlap.
    unsafe { CACHE.replace(StateCache::from_raw(raw)) };
    // SAFETY: snapshot capture and event observation are serialized on the
    // client main thread.
    unsafe { OBJECTS.replace(objects) };
    // SAFETY: snapshot capture and packet observation are serialized on the
    // client main thread.
    unsafe { COLLECTIONS.replace(raw) };
    SnapshotBoundary {
        revision: next_nonzero(&REVISION),
        event_sequence: EVENT_SEQUENCE.load(Ordering::Acquire),
        tick_ms,
    }
}

pub(crate) fn mark_collection_dirty(kind: CollectionKind, slot: u8, tick_ms: u32) {
    // SAFETY: the event hook runs on the client main thread, which is the sole
    // collection producer.
    unsafe { COLLECTIONS.mark(kind, slot, tick_ms) };
}

pub(crate) fn mark_resync_required() {
    let missing_sequence = next_nonzero(&EVENT_SEQUENCE);
    QUEUE.mark_resync_required(missing_sequence);
}

pub(crate) fn observe_tick() {
    #[cfg(windows)]
    let tick_ms = sender_tick_ms();
    #[cfg(not(windows))]
    let tick_ms = 0;
    // SAFETY: the tick hook runs on the client main thread, which is the sole
    // collection producer.
    unsafe {
        COLLECTIONS.observe_tick(tick_ms, |update, tick_ms| {
            push_event(QueuedStateUpdate::Collection(update), tick_ms);
        });
    }
}

pub(crate) fn observe_status(update: StatusUpdate, tick_ms: u32) {
    // SAFETY: the event hook observes decoded packets on the client main
    // thread, which is the sole cache producer.
    let update = unsafe { CACHE.filter_status(update) };
    if update.is_empty() {
        return;
    }
    push_event(QueuedStateUpdate::Status(update), tick_ms);
}

pub(crate) fn observe_user_position(x: i32, y: i32, tick_ms: u32) {
    // SAFETY: the event hook runs on the client main thread, which is the sole
    // cache producer.
    let Some((update, map_changed)) = (unsafe { CACHE.user_position(x, y) }) else {
        return;
    };
    push_event(QueuedStateUpdate::Location(update), tick_ms);
    if map_changed {
        if let Some(update) = unsafe { OBJECTS.clear() } {
            push_event(QueuedStateUpdate::Object(update), tick_ms);
        }
    } else {
        observe_self_position(x, y, tick_ms);
    }
}

pub(crate) fn observe_move(x: i32, y: i32, tick_ms: u32) {
    // SAFETY: the event hook runs on the client main thread, which is the sole
    // cache producer.
    let Some(update) = (unsafe { CACHE.move_position(x, y) }) else {
        return;
    };
    push_event(QueuedStateUpdate::Location(update), tick_ms);
    observe_self_position(x, y, tick_ms);
}

pub(crate) fn observe_effect(icon: u16, duration: Option<EffectDuration>, tick_ms: u32) {
    // SAFETY: the event hook runs on the client main thread, which is the sole
    // cache producer.
    let Some(update) = (unsafe { CACHE.effect(icon, duration) }) else {
        return;
    };
    push_event(QueuedStateUpdate::Effect(update), tick_ms);
}

pub(crate) fn observe_world(update: WorldUpdate, objects: &RawObjects, tick_ms: u32) {
    match update {
        WorldUpdate::Draw => {
            for object in objects
                .entries
                .iter()
                .take(usize::from(objects.count))
                .flatten()
                .copied()
            {
                if let Some(update) = unsafe { OBJECTS.draw(object) } {
                    push_event(QueuedStateUpdate::Object(update), tick_ms);
                }
            }
        }
        WorldUpdate::Move {
            id,
            x,
            y,
            direction,
        } => {
            if let Some(update) = unsafe { OBJECTS.move_object(id, x, y, direction) } {
                push_event(QueuedStateUpdate::Object(update), tick_ms);
            }
        }
        WorldUpdate::Direction { id, direction } => {
            if let Some(update) = unsafe { OBJECTS.change_direction(id, direction) } {
                push_event(QueuedStateUpdate::Object(update), tick_ms);
            }
        }
        WorldUpdate::Remove { id } => {
            if let Some(update) = unsafe { OBJECTS.remove(id) } {
                push_event(QueuedStateUpdate::Object(update), tick_ms);
            }
        }
    }
}

pub(crate) fn observe_message(message: ParsedMessage<'_>, tick_ms: u32) {
    let sender = participant(message.sender, message.sender_id);
    let recipient = participant(message.recipient, None);
    let Some(message) = QueuedMessage::new(message.kind, sender, recipient, message.text) else {
        return;
    };
    push_event(QueuedStateUpdate::Message(message), tick_ms);
}

fn participant(
    participant: Participant<'_>,
    fallback_id: Option<u32>,
) -> Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>> {
    match participant {
        Participant::Named(name) => QueuedClientText::new(name),
        Participant::SelfPlayer => unsafe { CACHE.self_name() }
            .and_then(|(name, length)| QueuedClientText::new(&name[..usize::from(length)])),
        Participant::None => fallback_id
            .and_then(|id| unsafe { OBJECTS.name(id) })
            .and_then(|(name, length)| QueuedClientText::new(&name[..usize::from(length)])),
    }
}

fn observe_self_position(x: i32, y: i32, tick_ms: u32) {
    let self_id = unsafe { CACHE.self_id() };
    if let Some(update) = unsafe { OBJECTS.move_self(self_id, x, y) } {
        push_event(QueuedStateUpdate::Object(update), tick_ms);
    }
    while let Some(update) = unsafe { OBJECTS.take_outside(x, y) } {
        push_event(QueuedStateUpdate::Object(update), tick_ms);
    }
}

fn push_event(update: QueuedStateUpdate, tick_ms: u32) {
    let event = QueuedStateEvent {
        sequence: next_nonzero(&EVENT_SEQUENCE),
        revision: next_nonzero(&REVISION),
        tick_ms,
        update,
    };
    QUEUE.push(event);
}

pub(crate) fn poll(after_sequence: u32, max_events: u16, wait: Duration) -> EventPollResult {
    let deadline = Instant::now() + wait;
    loop {
        let result = QUEUE.take_after(after_sequence, usize::from(max_events));
        if !matches!(&result, EventPollResult::Events(events) if events.is_empty()) {
            return result;
        }
        if Instant::now() >= deadline {
            return result;
        }
        thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

pub(crate) fn rebase(snapshot_sequence: u32) {
    QUEUE.discard_through(snapshot_sequence);
}

fn next_nonzero(counter: &AtomicU32) -> u32 {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            let next = value.wrapping_add(1);
            Some(if next == 0 { 1 } else { next })
        })
        .map(|previous| {
            let next = previous.wrapping_add(1);
            if next == 0 { 1 } else { next }
        })
        .expect("state counter update cannot fail")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedStateEvent {
    sequence: u32,
    revision: u32,
    tick_ms: u32,
    update: QueuedStateUpdate,
}

impl QueuedStateEvent {
    pub(crate) const fn sequence(&self) -> u32 {
        self.sequence
    }

    pub(crate) fn collection_batch(&self) -> Option<(CollectionKind, CollectionBatch)> {
        self.update.collection_batch()
    }

    pub(crate) fn into_model(self) -> StateEvent {
        let update = match self.update {
            QueuedStateUpdate::Status(update) => StateUpdate::Status(update),
            QueuedStateUpdate::Location(update) => StateUpdate::Location(update.into_model()),
            QueuedStateUpdate::Effect(update) => StateUpdate::Effect(update),
            QueuedStateUpdate::Object(update) => StateUpdate::Object(update.into_model()),
            QueuedStateUpdate::Message(update) => StateUpdate::Message(update.into_model()),
            QueuedStateUpdate::Collection(update) => update.into_model(self.tick_ms),
        };
        StateEvent {
            sequence: self.sequence,
            revision: self.revision,
            tick_ms: self.tick_ms,
            update,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Queue entries are fixed, pointer-free copies. Boxing the collection variant
// would allocate in the game-thread observer.
#[allow(clippy::large_enum_variant)]
enum QueuedStateUpdate {
    Status(StatusUpdate),
    Location(QueuedLocationUpdate),
    Effect(EffectUpdate),
    Object(QueuedObjectUpdate),
    Message(QueuedMessage),
    Collection(QueuedCollectionUpdate),
}

impl QueuedStateUpdate {
    fn collection_batch(self) -> Option<(CollectionKind, CollectionBatch)> {
        match self {
            Self::Collection(update) => Some((update.kind(), update.batch())),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueuedMessage {
    kind: MessageKind,
    sender: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
    recipient: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
    text: QueuedClientText<MAX_EVENT_MESSAGE_TEXT_BYTES>,
}

impl QueuedMessage {
    fn new(
        kind: MessageKind,
        sender: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
        recipient: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
        text: &[u8],
    ) -> Option<Self> {
        Some(Self {
            kind,
            sender,
            recipient,
            text: QueuedClientText::new(text)?,
        })
    }

    fn into_model(self) -> ClientMessage {
        ClientMessage {
            kind: self.kind,
            sender: self.sender.and_then(QueuedClientText::decode),
            recipient: self.recipient.and_then(QueuedClientText::decode),
            text: self.text.decode().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueuedClientText<const N: usize> {
    length: u16,
    bytes: [u8; N],
}

impl<const N: usize> QueuedClientText<N> {
    fn new(text: &[u8]) -> Option<Self> {
        if text.is_empty() || text.len() > N {
            return None;
        }
        let mut bytes = [0; N];
        bytes[..text.len()].copy_from_slice(text);
        Some(Self {
            length: u16::try_from(text.len()).expect("queued client text length fits u16"),
            bytes,
        })
    }

    fn decode(self) -> Option<String> {
        decode_client_text(&self.bytes[..usize::from(self.length)])
    }
}

#[cfg(windows)]
fn decode_client_text(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode_client_text(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueuedLocationUpdate {
    x: i32,
    y: i32,
    map: Option<QueuedMapChange>,
}

impl QueuedLocationUpdate {
    fn into_model(self) -> LocationUpdate {
        LocationUpdate {
            x: self.x,
            y: self.y,
            map: self.map.map(QueuedMapChange::into_model),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueuedMapChange {
    id: u32,
    width: i32,
    height: i32,
    name_length: u8,
    name: [u8; MAX_EVENT_MAP_NAME_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedMap {
    id: u32,
    width: i32,
    height: i32,
}

impl From<QueuedMapChange> for CachedMap {
    fn from(value: QueuedMapChange) -> Self {
        Self {
            id: value.id,
            width: value.width,
            height: value.height,
        }
    }
}

impl QueuedMapChange {
    fn new(id: u32, width: i32, height: i32, name: &[u8]) -> Self {
        let length = name.len().min(MAX_EVENT_MAP_NAME_BYTES);
        let mut owned_name = [0; MAX_EVENT_MAP_NAME_BYTES];
        owned_name[..length].copy_from_slice(&name[..length]);
        Self {
            id,
            width,
            height,
            name_length: u8::try_from(length).expect("map name length fits u8"),
            name: owned_name,
        }
    }

    fn into_model(self) -> MapChange {
        MapChange {
            id: self.id,
            name: decode_map_name(&self.name[..usize::from(self.name_length)]),
            width: self.width,
            height: self.height,
        }
    }
}

#[cfg(windows)]
fn decode_map_name(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode_map_name(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

struct MainThreadCollections(UnsafeCell<CollectionTracker>);

// SAFETY: collection state is mutated only by the client main thread during
// active hooks or by lifecycle reset while hooks and the IPC consumer are down.
unsafe impl Sync for MainThreadCollections {}

impl MainThreadCollections {
    const fn new() -> Self {
        Self(UnsafeCell::new(CollectionTracker::new()))
    }

    unsafe fn reset(&self) {
        // SAFETY: the caller guarantees exclusive lifecycle access.
        unsafe { &mut *self.0.get() }.reset();
    }

    unsafe fn replace(&self, raw: &RawStateSnapshot) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.replace(raw);
    }

    unsafe fn mark(&self, kind: CollectionKind, slot: u8, tick_ms: u32) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.mark(kind, slot, tick_ms);
    }

    unsafe fn observe_tick(&self, tick_ms: u32, emit: impl FnMut(QueuedCollectionUpdate, u32)) {
        // SAFETY: the caller guarantees client-main-thread access.
        unsafe { &mut *self.0.get() }.observe_tick(tick_ms, emit);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StateCache {
    self_id: Option<u32>,
    self_name: [u8; 16],
    self_name_len: u8,
    core: Option<CoreStatus>,
    vitals: Option<CurrentVitals>,
    progression: Option<ProgressionStatus>,
    gold: Option<u32>,
    modifiers: Option<CharacterModifiers>,
    is_blinded: Option<bool>,
    is_action_restricted: Option<bool>,
    map: Option<CachedMap>,
    position: Option<(i32, i32)>,
    pending_map: Option<QueuedMapChange>,
    effects: [Option<Effect>; 10],
}

impl StateCache {
    fn from_raw(raw: &RawStateSnapshot) -> Self {
        if raw.character_available {
            Self::from_character(&raw.character)
        } else {
            Self::default()
        }
    }

    fn from_character(raw: &RawCharacter) -> Self {
        Self {
            self_id: raw.id,
            self_name: raw.name,
            self_name_len: raw.name_len,
            core: Some(CoreStatus {
                level: raw.level,
                ability_level: raw.ability_level,
                max_health: raw.max_health,
                max_mana: raw.max_mana,
                weight: raw.weight,
                max_weight: raw.max_weight,
                stats: CharacterStats {
                    strength: raw.strength,
                    intelligence: raw.intelligence,
                    wisdom: raw.wisdom,
                    constitution: raw.constitution,
                    dexterity: raw.dexterity,
                },
            }),
            vitals: Some(CurrentVitals {
                health: raw.health,
                mana: raw.mana,
            }),
            progression: raw.pane_progression.map(|pane| ProgressionStatus {
                experience: raw.experience,
                ability_points: pane.ability_points,
                experience_to_next_level: pane.experience_to_next_level,
                ability_to_next_level: pane.ability_to_next_level,
            }),
            gold: Some(raw.gold),
            modifiers: raw.modifiers.map(modifiers),
            is_blinded: Some(raw.is_blinded),
            is_action_restricted: Some(raw.is_action_restricted),
            map: raw.location.map(|location| CachedMap {
                id: location.map_id,
                width: location.width,
                height: location.height,
            }),
            position: raw.location.and_then(|location| location.x.zip(location.y)),
            pending_map: None,
            effects: raw.effects.map_or([None; 10], |effects| {
                effects.effects.map(|effect| {
                    effect.map(|effect| Effect {
                        icon: effect.icon,
                        duration: EffectDuration::from_raw(effect.duration)
                            .expect("captured effect duration is valid"),
                    })
                })
            }),
        }
    }

    fn filter_status(&mut self, update: StatusUpdate) -> StatusUpdate {
        StatusUpdate {
            core: changed(&mut self.core, update.core),
            vitals: changed(&mut self.vitals, update.vitals),
            progression: changed(&mut self.progression, update.progression),
            gold: changed(&mut self.gold, update.gold),
            modifiers: changed(&mut self.modifiers, update.modifiers),
            is_blinded: changed(&mut self.is_blinded, update.is_blinded),
            is_action_restricted: changed(
                &mut self.is_action_restricted,
                update.is_action_restricted,
            ),
        }
    }

    fn stage_map_transition(&mut self, map_id: u32, width: i32, height: i32, name: &[u8]) {
        let pending = QueuedMapChange::new(map_id, width, height, name);
        if self.map == Some(pending.into()) {
            self.pending_map = None;
        } else {
            self.pending_map = Some(pending);
        }
    }

    fn user_position(&mut self, x: i32, y: i32) -> Option<(QueuedLocationUpdate, bool)> {
        let map = self.pending_map.take();
        if map.is_none() && self.position == Some((x, y)) {
            return None;
        }
        self.position = Some((x, y));
        if let Some(map) = map {
            self.map = Some(map.into());
        }
        let map_changed = map.is_some();
        Some((QueuedLocationUpdate { x, y, map }, map_changed))
    }

    fn move_position(&mut self, x: i32, y: i32) -> Option<QueuedLocationUpdate> {
        if self.pending_map.is_some() || self.position == Some((x, y)) {
            return None;
        }
        self.position = Some((x, y));
        Some(QueuedLocationUpdate { x, y, map: None })
    }

    fn effect(&mut self, icon: u16, duration: Option<EffectDuration>) -> Option<EffectUpdate> {
        if let Some(index) = self
            .effects
            .iter()
            .position(|effect| effect.is_some_and(|effect| effect.icon == icon))
        {
            return match duration {
                None => {
                    self.effects[index] = None;
                    Some(EffectUpdate::Removed { icon })
                }
                Some(duration) => {
                    let effect = Effect { icon, duration };
                    if self.effects[index] == Some(effect) {
                        None
                    } else {
                        self.effects[index] = Some(effect);
                        Some(EffectUpdate::Changed(effect))
                    }
                }
            };
        }

        let duration = duration?;
        let slot = self.effects.iter_mut().find(|effect| effect.is_none())?;
        let effect = Effect { icon, duration };
        *slot = Some(effect);
        Some(EffectUpdate::Added(effect))
    }
}

fn changed<T: Copy + Eq>(cached: &mut Option<T>, incoming: Option<T>) -> Option<T> {
    let value = incoming?;
    if *cached == Some(value) {
        return None;
    }
    *cached = Some(value);
    Some(value)
}

fn modifiers(raw: RawModifiers) -> CharacterModifiers {
    CharacterModifiers {
        armor_class: raw.armor_class,
        damage: raw.damage,
        hit: raw.hit,
        magic_resistance: raw.magic_resistance_units.saturating_mul(10),
        attack_element: Element::from_raw(raw.attack_element),
        defense_element: Element::from_raw(raw.defense_element),
    }
}

struct MainThreadCache(UnsafeCell<StateCache>);

// SAFETY: access is restricted to the client main thread except during reset,
// which runs only while the producer hook is absent.
unsafe impl Sync for MainThreadCache {}

impl MainThreadCache {
    const fn new() -> Self {
        Self(UnsafeCell::new(StateCache {
            self_id: None,
            self_name: [0; 16],
            self_name_len: 0,
            core: None,
            vitals: None,
            progression: None,
            gold: None,
            modifiers: None,
            is_blinded: None,
            is_action_restricted: None,
            map: None,
            position: None,
            pending_map: None,
            effects: [None; 10],
        }))
    }

    unsafe fn replace(&self, cache: StateCache) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { *self.0.get() = cache };
    }

    unsafe fn self_id(&self) -> Option<u32> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).self_id }
    }

    unsafe fn self_name(&self) -> Option<([u8; 16], u8)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        let cache = unsafe { &*self.0.get() };
        (cache.self_name_len != 0).then_some((cache.self_name, cache.self_name_len))
    }

    unsafe fn filter_status(&self, update: StatusUpdate) -> StatusUpdate {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).filter_status(update) }
    }

    unsafe fn map_transition_pending(&self) -> bool {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).pending_map.is_some() }
    }

    unsafe fn stage_map_transition(&self, map_id: u32, width: i32, height: i32, name: &[u8]) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).stage_map_transition(map_id, width, height, name) };
    }

    unsafe fn user_position(&self, x: i32, y: i32) -> Option<(QueuedLocationUpdate, bool)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).user_position(x, y) }
    }

    unsafe fn move_position(&self, x: i32, y: i32) -> Option<QueuedLocationUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_position(x, y) }
    }

    unsafe fn effect(&self, icon: u16, duration: Option<EffectDuration>) -> Option<EffectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).effect(icon, duration) }
    }
}

struct MainThreadObjects(UnsafeCell<ObjectCache>);

// SAFETY: access is restricted to the client main thread except during reset,
// which runs only while the producer hook is absent.
unsafe impl Sync for MainThreadObjects {}

impl MainThreadObjects {
    const fn new() -> Self {
        Self(UnsafeCell::new(ObjectCache::empty()))
    }

    unsafe fn replace(&self, objects: &RawObjects) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).replace(objects) };
    }

    unsafe fn name(&self, id: u32) -> Option<([u8; darpc_game_client::MAX_OBJECT_NAME_BYTES], u8)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).name(id) }
    }

    unsafe fn draw(&self, object: RawWorldObject) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).upsert(object) }
    }

    unsafe fn move_object(
        &self,
        id: u32,
        x: i32,
        y: i32,
        direction: Option<u8>,
    ) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_object(id, x, y, direction) }
    }

    unsafe fn change_direction(&self, id: u32, direction: u8) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).change_direction(id, direction) }
    }

    unsafe fn remove(&self, id: u32) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).remove(id) }
    }

    unsafe fn move_self(&self, id: Option<u32>, x: i32, y: i32) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_self(id, x, y) }
    }

    unsafe fn take_outside(&self, x: i32, y: i32) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).take_outside(x, y) }
    }

    unsafe fn clear(&self) -> Option<QueuedObjectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).clear() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sequence: u32) -> QueuedStateEvent {
        QueuedStateEvent {
            sequence,
            revision: sequence,
            tick_ms: sequence,
            update: QueuedStateUpdate::Status(StatusUpdate {
                gold: Some(sequence),
                ..StatusUpdate::default()
            }),
        }
    }

    fn collection_event(sequence: u32, batch_index: u8, batch_count: u8) -> QueuedStateEvent {
        QueuedStateEvent {
            sequence,
            revision: sequence,
            tick_ms: sequence,
            update: QueuedStateUpdate::Collection(QueuedCollectionUpdate::test_inventory_batch(
                batch_index,
                batch_count,
            )),
        }
    }

    #[test]
    fn queue_preserves_order_and_snapshot_boundary() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.push(event(2));
        assert_eq!(
            queue.take_after(1, 4),
            EventPollResult::Events(vec![event(2).into_model()])
        );
    }

    #[test]
    fn overflow_requires_resynchronization_until_rebased() {
        let queue = EventQueue::<2>::new();
        queue.push(event(1));
        queue.push(event(2));
        queue.push(event(3));
        assert_eq!(
            queue.take_after(0, 2),
            EventPollResult::ResyncRequired {
                missing_sequence: 3,
                latest_sequence: 3,
            }
        );
        assert_eq!(queue.take_after(3, 2), EventPollResult::Events(Vec::new()));
    }

    #[test]
    fn explicit_resync_marker_requires_a_new_snapshot() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.mark_resync_required(2);
        assert_eq!(
            queue.take_after(1, 4),
            EventPollResult::ResyncRequired {
                missing_sequence: 2,
                latest_sequence: 2,
            }
        );
        assert_eq!(queue.take_after(2, 4), EventPollResult::Events(Vec::new()));
    }

    #[test]
    fn rebase_discards_only_events_covered_by_the_snapshot() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.push(event(2));
        queue.push(event(3));
        queue.discard_through(2);
        assert_eq!(
            queue.take_after(2, 4),
            EventPollResult::Events(vec![event(3).into_model()])
        );
    }

    #[test]
    fn a_noncontiguous_queue_entry_requires_resynchronization() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.push(event(3));
        assert_eq!(
            queue.take_after(0, 4),
            EventPollResult::ResyncRequired {
                missing_sequence: 2,
                latest_sequence: 3,
            }
        );
    }

    #[test]
    fn collection_batches_are_never_split_across_polls() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.push(collection_event(2, 0, 2));
        queue.push(collection_event(3, 1, 2));

        assert_eq!(
            queue.take_after(0, 2),
            EventPollResult::Events(vec![event(1).into_model()])
        );
        assert_eq!(
            queue.take_after(1, 4),
            EventPollResult::Events(vec![
                collection_event(2, 0, 2).into_model(),
                collection_event(3, 1, 2).into_model(),
            ])
        );
    }

    #[test]
    fn incomplete_collection_batches_wait_for_the_producer() {
        let queue = EventQueue::<4>::new();
        queue.push(collection_event(1, 0, 2));
        assert_eq!(queue.take_after(0, 4), EventPollResult::Events(Vec::new()));

        queue.push(collection_event(2, 1, 2));
        assert_eq!(
            queue.take_after(0, 4),
            EventPollResult::Events(vec![
                collection_event(1, 0, 2).into_model(),
                collection_event(2, 1, 2).into_model(),
            ])
        );
    }

    #[test]
    fn map_transition_commits_with_authoritative_position() {
        let mut cache = StateCache {
            position: Some((10, 20)),
            ..StateCache::default()
        };
        cache.stage_map_transition(3001, 100, 80, b"Mileth");
        assert_eq!(cache.move_position(11, 20), None);

        let (update, map_changed) = cache.user_position(43, 40).unwrap();
        let update = update.into_model();
        assert!(map_changed);
        assert_eq!(update.x, 43);
        assert_eq!(update.y, 40);
        assert_eq!(
            update.map,
            Some(MapChange {
                id: 3001,
                name: Some("Mileth".into()),
                width: 100,
                height: 80,
            })
        );
        assert_eq!(cache.position, Some((43, 40)));
        assert!(cache.pending_map.is_none());
    }

    #[test]
    fn same_map_refresh_does_not_stage_a_transition() {
        let mut cache = StateCache {
            map: Some(CachedMap {
                id: 498,
                width: 20,
                height: 15,
            }),
            position: Some((1, 8)),
            ..StateCache::default()
        };

        cache.stage_map_transition(498, 20, 15, b"Rucesion Inn");
        assert!(cache.pending_map.is_none());

        let (update, map_changed) = cache.user_position(2, 8).unwrap();
        let update = update.into_model();
        assert!(!map_changed);
        assert_eq!((update.x, update.y), (2, 8));
        assert_eq!(update.map, None);
    }

    #[test]
    fn effects_follow_client_slot_and_delta_rules() {
        let mut cache = StateCache::default();
        let white = Effect {
            icon: 300,
            duration: EffectDuration::White,
        };
        assert_eq!(
            cache.effect(300, Some(EffectDuration::White)),
            Some(EffectUpdate::Added(white))
        );
        assert_eq!(cache.effect(300, Some(EffectDuration::White)), None);
        assert_eq!(
            cache.effect(300, Some(EffectDuration::Red)),
            Some(EffectUpdate::Changed(Effect {
                icon: 300,
                duration: EffectDuration::Red,
            }))
        );
        assert_eq!(
            cache.effect(300, None),
            Some(EffectUpdate::Removed { icon: 300 })
        );
        assert_eq!(cache.effect(300, None), None);
    }

    #[test]
    fn full_effect_slots_ignore_a_new_icon() {
        let mut cache = StateCache::default();
        for icon in 1..=10 {
            assert!(cache.effect(icon, Some(EffectDuration::White)).is_some());
        }
        assert_eq!(cache.effect(11, Some(EffectDuration::White)), None);
    }
}
