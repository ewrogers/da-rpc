#![cfg_attr(not(windows), allow(dead_code))]

use crate::{
    atomic_sequence::next_nonzero,
    collections::{CollectionTracker, QueuedCollectionUpdate},
    event_queue::EventQueue,
    objects::{ObjectCache, QueuedObjectUpdate},
    packet::{
        message::{ParsedMessage, Participant},
        object::WorldUpdate,
        visual::VisualUpdate,
    },
};
use darpc_game_client::{
    MAX_OBJECT_NAME_BYTES, RawCharacter, RawLifecycle, RawModifiers, RawObjects, RawStateSnapshot,
    RawWorldObject,
};
use darpc_model::{
    AbilityUpdate, ActionUpdate, AudioUpdate, CharacterModifiers, CharacterStats, ClientCommand,
    ClientLifecycle, ClientMessage, CollectionBatch, CollectionKind, CoreStatus, CurrentVitals,
    Effect, EffectDuration, EffectUpdate, Element, EntityUpdate, LifecycleUpdate, LocationUpdate,
    MapChange, MessageKind, MovementUpdate, ProgressionStatus, SpellCancellationSource,
    SpellCastArguments, StateEvent, StateUpdate, StatusUpdate, TilePosition,
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

mod ability;
mod action;
mod cache;
mod update;

#[cfg(windows)]
use ability::casting_state;
use ability::{CastingState, QueuedAbilityUpdate};
use cache::*;
pub(crate) use update::QueuedStateEvent;
use update::*;

pub(crate) fn observe_outgoing(body: &[u8], tick_ms: u32) {
    ability::observe_outgoing(body, tick_ms);
    action::observe_outgoing(body, tick_ms);
    crate::exchange::observe_outgoing(body, tick_ms);
}

pub(crate) fn observe_audio(update: AudioUpdate, tick_ms: u32) {
    push_event(QueuedStateUpdate::Audio(update), tick_ms);
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn current_gold() -> Option<u32> {
    // SAFETY: commands run on the client main thread, which owns the cache.
    unsafe { CACHE.gold() }
}

pub(crate) fn observe_exchange(update: crate::exchange::QueuedExchange, tick_ms: u32) -> bool {
    push_event(QueuedStateUpdate::Exchange(update), tick_ms)
}

pub(crate) fn observe_spell_cancelled(tick_ms: u32) {
    ability::observe_spell_cancelled(tick_ms);
}

pub(crate) fn observe_dialog(body: &[u8], tick_ms: u32) {
    if let Some(dialog) = crate::dialog::observe_server(body)
        && !push_event(QueuedStateUpdate::Dialog(dialog), tick_ms)
    {
        crate::dialog::release(dialog);
    }
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn observe_dialog_submission(submission: darpc_model::DialogSubmission, tick_ms: u32) {
    if let Some(dialog) = crate::dialog::submit(submission)
        && !push_event(QueuedStateUpdate::Dialog(dialog), tick_ms)
    {
        crate::dialog::release(dialog);
    }
}

pub(crate) fn observe_dialog_closed(reason: darpc_model::DialogCloseReason, tick_ms: u32) {
    if let Some(dialog) = crate::dialog::close(reason)
        && !push_event(QueuedStateUpdate::Dialog(dialog), tick_ms)
    {
        crate::dialog::release(dialog);
    }
}

pub(crate) fn observe_group(update: crate::group::QueuedGroup, tick_ms: u32) -> bool {
    push_event(QueuedStateUpdate::Group(update), tick_ms)
}

pub(crate) fn observe_legend(update: crate::legend::QueuedLegend, tick_ms: u32) -> bool {
    push_event(QueuedStateUpdate::Legend(update), tick_ms)
}

pub(crate) fn observe_player(update: crate::player::QueuedPlayer, tick_ms: u32) -> bool {
    push_event(QueuedStateUpdate::Player(update), tick_ms)
}

pub(crate) fn observe_character_profile(
    update: crate::player::QueuedCharacterProfile,
    tick_ms: u32,
) -> bool {
    push_event(QueuedStateUpdate::CharacterProfile(update), tick_ms)
}

pub(crate) fn observe_route(update: crate::route::QueuedRoute, tick_ms: u32) -> bool {
    push_event(QueuedStateUpdate::PlannedRoute(update), tick_ms)
}

pub(crate) fn observe_visual(update: VisualUpdate, tick_ms: u32) {
    match update {
        VisualUpdate::Motion {
            object_id,
            animation,
            duration_10ms,
        } => {
            let Some(entity) = observed_entity(object_id) else {
                return;
            };
            push_event(
                QueuedStateUpdate::Entity(QueuedEntityUpdate::Animated {
                    entity,
                    animation,
                    duration_10ms,
                }),
                tick_ms,
            );
        }
        VisualUpdate::Damage {
            object_id,
            health_percent,
        } => {
            if health_percent > 100 {
                return;
            }
            let Some(entity) = observed_entity(object_id) else {
                return;
            };
            push_event(
                QueuedStateUpdate::Entity(QueuedEntityUpdate::Damaged {
                    entity,
                    health_percent,
                }),
                tick_ms,
            );
        }
        VisualUpdate::Effect {
            target_id,
            source_id,
            target_effect,
            source_effect,
            frame_interval_ms,
        } => observe_entity_effect(
            target_id,
            source_id,
            target_effect,
            source_effect,
            frame_interval_ms,
            tick_ms,
        ),
    }
}

fn observe_entity_effect(
    target_id: u32,
    source_id: u32,
    target_effect: u16,
    source_effect: u16,
    frame_interval_ms: i16,
    tick_ms: u32,
) {
    if target_effect == 0 || target_effect == 0x00FF {
        return;
    }
    let Some(target) = observed_entity(target_id) else {
        return;
    };
    let source = (source_id != 0)
        .then(|| observed_entity(source_id))
        .flatten();
    let moving = (10_000..=11_999).contains(&target_effect);
    if moving && source.is_none() {
        return;
    }
    push_event(
        QueuedStateUpdate::Entity(QueuedEntityUpdate::Effect {
            entity: target,
            effect: target_effect,
            source,
            frame_interval_ms: (!moving).then_some(frame_interval_ms),
        }),
        tick_ms,
    );
    if !moving
        && source_effect != 0
        && let Some(source) = source
    {
        push_event(
            QueuedStateUpdate::Entity(QueuedEntityUpdate::Effect {
                entity: source,
                effect: source_effect,
                source: None,
                frame_interval_ms: Some(frame_interval_ms),
            }),
            tick_ms,
        );
    }
}

fn observed_entity(id: u32) -> Option<RawWorldObject> {
    // SAFETY: decoded packet observation runs on the client main thread, which
    // is the sole owner of the object cache.
    unsafe { OBJECTS.get(id).or_else(|| CACHE.self_entity(id)) }
}

pub(crate) const EVENT_QUEUE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_MAP_NAME_BYTES: usize = u8::MAX as usize;
const MAX_EVENT_MESSAGE_NAME_BYTES: usize = 15;
const MAX_EVENT_MESSAGE_TEXT_BYTES: usize = 256;
const MAX_EVENT_COMMAND_BYTES: usize = u8::MAX as usize - 1;
const MAX_SPELL_INPUT_BYTES: usize = 100;
const EVENT_QUEUE_CAPACITY: usize = EVENT_QUEUE_BYTES / size_of::<QueuedStateEvent>();
const POLL_INTERVAL: Duration = Duration::from_millis(2);
#[cfg(not(test))]
const LIFECYCLE_POLL_INTERVAL_MS: u32 = 100;

static REVISION: AtomicU32 = AtomicU32::new(0);
static EVENT_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static NEXT_LIFECYCLE_POLL_MS: AtomicU32 = AtomicU32::new(0);
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
    NEXT_LIFECYCLE_POLL_MS.store(0, Ordering::Release);
    QUEUE.reset();
    #[cfg(all(windows, not(test)))]
    crate::actions::movement::reset_tracking();
    // SAFETY: reset runs before the event hook is installed and after the IPC
    // consumer has stopped, so no other thread accesses the cache.
    unsafe { CACHE.replace(StateCache::default()) };
    // SAFETY: reset has the same exclusive lifecycle access described above.
    unsafe { OBJECTS.clear() };
    // SAFETY: reset has the same exclusive lifecycle access described above.
    unsafe { COLLECTIONS.reset() };
    crate::legend::reset();
    crate::player::reset();
}

#[must_use]
pub(crate) fn map_transition_pending() -> bool {
    // SAFETY: snapshot ticks and packet observations run on the client main
    // thread, which is the sole cache producer.
    unsafe { CACHE.map_transition_pending() }
}

pub(crate) fn stage_map_transition(
    map_id: u32,
    width: i32,
    height: i32,
    name: &[u8],
    tick_ms: u32,
) {
    #[cfg(all(windows, not(test)))]
    crate::actions::movement::clear_route_destination();
    #[cfg(windows)]
    if crate::dialog::is_active() {
        observe_dialog_closed(darpc_model::DialogCloseReason::WorldChanged, tick_ms);
    }
    // SAFETY: the map-size hook runs on the client main thread, which is the
    // sole cache producer.
    if let Some(update) = unsafe { CACHE.stage_map_transition(map_id, width, height, name) } {
        publish_location_update(update, true, tick_ms);
    }
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

pub(crate) fn merge_snapshot_position(raw: &mut RawStateSnapshot) {
    if raw.character_available {
        // SAFETY: snapshot capture runs on the client main thread, which is the
        // sole cache producer.
        unsafe { CACHE.merge_position(&mut raw.character) };
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

#[must_use]
#[cfg(not(test))]
pub(crate) fn target_position(id: u32) -> Option<TilePosition> {
    // SAFETY: commands execute from the tick hook on the client main thread,
    // which is the sole owner of both caches.
    let position = unsafe {
        if CACHE.self_id() == Some(id) {
            CACHE.position()
        } else {
            OBJECTS.position(id)
        }
    }?;
    Some(TilePosition {
        x: position.0,
        y: position.1,
    })
}

#[must_use]
#[cfg(not(test))]
pub(crate) fn self_target() -> Option<(u32, TilePosition)> {
    // SAFETY: commands execute from the tick hook on the client main thread,
    // which is the sole owner of the state cache.
    let id = unsafe { CACHE.self_id() }?;
    target_position(id).map(|position| (id, position))
}

#[must_use]
#[cfg(not(test))]
pub(crate) fn valid_tile(x: i32, y: i32) -> bool {
    // SAFETY: commands execute from the tick hook on the client main thread.
    unsafe { CACHE.valid_tile(x, y) }
}

pub(crate) fn observe_tick() {
    #[cfg(windows)]
    let tick_ms = sender_tick_ms();
    #[cfg(not(windows))]
    let tick_ms = 0;
    #[cfg(all(windows, not(test)))]
    observe_lifecycle(tick_ms);
    #[cfg(all(windows, not(test)))]
    crate::route::observe_current(tick_ms);
    #[cfg(all(windows, not(test)))]
    if crate::dialog::is_active() && !crate::actions::dialog::is_open() {
        observe_dialog_closed(darpc_model::DialogCloseReason::Client, tick_ms);
    }
    #[cfg(all(windows, not(test)))]
    crate::actions::group::observe_tick(tick_ms);
    // SAFETY: the tick hook runs on the client main thread, which is the sole
    // collection producer.
    unsafe {
        COLLECTIONS.observe_tick(tick_ms, |update, tick_ms| {
            push_event(QueuedStateUpdate::Collection(update), tick_ms);
        });
    }
    #[cfg(all(windows, not(test)))]
    if let Some(is_walking) = crate::actions::movement::is_walking() {
        let destination = crate::actions::movement::route_destination();
        if let Some(update) = unsafe { CACHE.movement(is_walking, destination) } {
            if matches!(update, MovementUpdate::Stopped { .. })
                && !crate::actions::movement::is_replan_pending()
            {
                crate::actions::movement::clear_route_destination();
            }
            push_event(QueuedStateUpdate::Movement(update), tick_ms);
        }
    }
    #[cfg(windows)]
    if let Some(casting) = casting_state() {
        // SAFETY: casting state is observed by the client main-thread tick.
        if let Some(update) = unsafe { CACHE.observe_casting(casting) } {
            push_event(QueuedStateUpdate::Ability(update), tick_ms);
        }
    }
}

#[cfg(all(windows, not(test)))]
fn observe_lifecycle(tick_ms: u32) {
    let next_poll = NEXT_LIFECYCLE_POLL_MS.load(Ordering::Acquire);
    if !crate::wrapping_time::deadline_reached(tick_ms, next_poll) {
        return;
    }
    NEXT_LIFECYCLE_POLL_MS.store(
        tick_ms.wrapping_add(LIFECYCLE_POLL_INTERVAL_MS),
        Ordering::Release,
    );
    let Ok(raw) = crate::snapshot::capture_lifecycle() else {
        return;
    };
    let current = lifecycle(raw);
    // SAFETY: lifecycle capture runs from the client main-thread tick, which
    // is the sole state-cache producer.
    if let Some(update) = unsafe { CACHE.lifecycle(current) } {
        push_event(QueuedStateUpdate::Lifecycle(update), tick_ms);
    }
}

const fn lifecycle(raw: RawLifecycle) -> ClientLifecycle {
    match raw {
        RawLifecycle::Unknown => ClientLifecycle::Unknown,
        RawLifecycle::Title => ClientLifecycle::Title,
        RawLifecycle::Transition => ClientLifecycle::Transition,
        RawLifecycle::InGame => ClientLifecycle::InGame,
        RawLifecycle::Disconnected => ClientLifecycle::Disconnected,
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
    publish_location_update(update, map_changed, tick_ms);
}

fn publish_location_update(update: QueuedLocationUpdate, map_changed: bool, tick_ms: u32) {
    push_event(QueuedStateUpdate::Location(update), tick_ms);
    if map_changed {
        crate::player::cleared();
        if let Some(update) = unsafe { OBJECTS.clear() } {
            push_event(QueuedStateUpdate::Object(update), tick_ms);
        }
    } else {
        observe_self_position(update.x, update.y, tick_ms);
    }
}

pub(crate) fn schedule_position_sync_replan() {
    #[cfg(not(test))]
    crate::actions::movement::schedule_position_sync_replan();
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
            crate::player::removed(id);
            if let Some(update) = unsafe { OBJECTS.remove(id) } {
                push_event(QueuedStateUpdate::Object(update), tick_ms);
            }
        }
    }
}

pub(crate) fn observed_player(id: u32) -> Option<RawWorldObject> {
    // SAFETY: callers run on the client main thread, which owns the cache.
    match unsafe { OBJECTS.get(id) } {
        Some(player @ RawWorldObject::Player { .. }) => Some(player),
        _ => None,
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

pub(crate) fn observe_command(command: &[u8], tick_ms: u32) -> bool {
    let Some(command) = QueuedCommand::new(command) else {
        return false;
    };
    push_event(QueuedStateUpdate::Command(command), tick_ms)
}

fn participant(
    participant: Participant<'_>,
    fallback_id: Option<u32>,
) -> Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>> {
    match participant {
        Participant::Named(name) => QueuedClientText::try_nonempty(name),
        Participant::SelfPlayer => unsafe { CACHE.self_name() }.and_then(|(name, length)| {
            QueuedClientText::try_nonempty(&name[..usize::from(length)])
        }),
        Participant::None => fallback_id.and_then(|id| {
            // SAFETY: packet observation runs on the client main thread,
            // which is the sole owner of both name caches.
            unsafe {
                if CACHE.self_id() == Some(id) {
                    CACHE.self_name().and_then(|(name, length)| {
                        QueuedClientText::try_nonempty(&name[..usize::from(length)])
                    })
                } else {
                    OBJECTS.name(id).and_then(|(name, length)| {
                        QueuedClientText::try_nonempty(&name[..usize::from(length)])
                    })
                }
            }
        }),
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

fn observe_self_position(x: i32, y: i32, tick_ms: u32) {
    let self_id = unsafe { CACHE.self_id() };
    if let Some(update) = unsafe { OBJECTS.move_self(self_id, x, y) } {
        push_event(QueuedStateUpdate::Object(update), tick_ms);
    }
    while let Some(update) = unsafe { OBJECTS.take_outside(x, y) } {
        if let QueuedObjectUpdate::Disappeared(RawWorldObject::Player { id, .. }) = update {
            crate::player::removed(id);
        }
        push_event(QueuedStateUpdate::Object(update), tick_ms);
    }
}

fn push_event(update: QueuedStateUpdate, tick_ms: u32) -> bool {
    let event = QueuedStateEvent {
        sequence: next_nonzero(&EVENT_SEQUENCE),
        revision: next_nonzero(&REVISION),
        tick_ms,
        update,
    };
    QUEUE.push(event)
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
    fn movement_lifecycle_compares_the_confirmed_position_to_the_goal() {
        let destination = TilePosition { x: 6, y: 5 };
        let mut cache = StateCache {
            is_walking: Some(false),
            position: Some((2, 8)),
            ..StateCache::default()
        };
        assert_eq!(
            cache.movement(true, Some(destination)),
            Some(MovementUpdate::Started {
                current: TilePosition { x: 2, y: 8 },
                destination: Some(destination),
            })
        );

        cache.position = Some((5, 5));
        assert_eq!(
            cache.movement(false, Some(destination)),
            Some(MovementUpdate::Stopped {
                current: TilePosition { x: 5, y: 5 },
                destination: Some(destination),
                reached_destination: Some(false),
            })
        );
    }

    #[test]
    fn replacing_a_cast_cancels_the_previous_spell_before_tracking_the_new_one() {
        let mut cache = StateCache::default();
        assert_eq!(cache.spell_begin(4, 3), None);
        assert_eq!(cache.spell_begin(7, 2), Some(4));
        assert_eq!(
            cache.active_spell,
            Some(CachedCast {
                slot: 7,
                total_lines: 2
            })
        );
        assert_eq!(cache.is_casting, Some(true));
    }

    #[test]
    fn an_instant_spell_only_replaces_a_different_active_spell() {
        let mut cache = StateCache::default();
        assert_eq!(cache.spell_begin(4, 3), None);
        assert_eq!(cache.spell_cast(7), Some(4));
        assert_eq!(cache.is_casting, Some(false));
        assert_eq!(cache.active_spell, None);

        assert_eq!(cache.spell_begin(4, 3), None);
        assert_eq!(cache.spell_cast(4), None);
    }

    #[test]
    fn queue_preserves_order_and_snapshot_boundary() {
        let queue = EventQueue::<4>::new();
        queue.push(event(1));
        queue.push(event(2));
        assert_eq!(
            queue.take_after(1, 4),
            EventPollResult::Events(vec![event(2).into_model().unwrap()])
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
            EventPollResult::Events(vec![event(3).into_model().unwrap()])
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
            EventPollResult::Events(vec![event(1).into_model().unwrap()])
        );
        assert_eq!(
            queue.take_after(1, 4),
            EventPollResult::Events(vec![
                collection_event(2, 0, 2).into_model().unwrap(),
                collection_event(3, 1, 2).into_model().unwrap(),
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
                collection_event(1, 0, 2).into_model().unwrap(),
                collection_event(2, 1, 2).into_model().unwrap(),
            ])
        );
    }

    #[test]
    fn map_transition_commits_with_authoritative_position() {
        let mut cache = StateCache {
            map: Some(CachedMap {
                id: 3000,
                width: 90,
                height: 70,
            }),
            position: Some((10, 20)),
            ..StateCache::default()
        };
        assert_eq!(cache.stage_map_transition(3001, 100, 80, b"Mileth"), None);
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
    fn initial_map_commits_when_position_arrives_first() {
        let mut cache = StateCache::default();
        let (position, map_changed) = cache.user_position(43, 40).unwrap();
        assert!(!map_changed);
        assert_eq!(position.map, None);

        let update = cache
            .stage_map_transition(3001, 100, 80, b"Mileth")
            .unwrap()
            .into_model();
        assert_eq!((update.x, update.y), (43, 40));
        assert_eq!(
            update.map,
            Some(MapChange {
                id: 3001,
                name: Some("Mileth".into()),
                width: 100,
                height: 80,
            })
        );
        assert_eq!(
            cache.map,
            Some(CachedMap {
                id: 3001,
                width: 100,
                height: 80,
            })
        );
        assert!(cache.pending_map.is_none());
    }

    #[test]
    fn snapshot_uses_matching_authoritative_position() {
        let cache = StateCache {
            map: Some(CachedMap {
                id: 3001,
                width: 100,
                height: 80,
            }),
            position: Some((43, 40)),
            ..StateCache::default()
        };
        let mut raw = RawCharacter::empty();
        raw.location = Some(darpc_game_client::RawLocation {
            map_id: 3000,
            name: None,
            x: None,
            y: None,
            width: 100,
            height: 80,
        });

        cache.merge_position(&mut raw);
        assert_eq!(raw.location.unwrap().x.zip(raw.location.unwrap().y), None);

        raw.location.as_mut().unwrap().map_id = 3001;
        cache.merge_position(&mut raw);
        assert_eq!(
            raw.location.unwrap().x.zip(raw.location.unwrap().y),
            Some((43, 40))
        );
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

        assert_eq!(
            cache.stage_map_transition(498, 20, 15, b"Rucesion Inn"),
            None
        );
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
