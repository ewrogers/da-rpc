#![cfg_attr(not(windows), allow(dead_code))]

mod parse;
mod profile_cache;
mod request_tracking;

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

use crate::{inline_bytes::InlineBytes, transfer_slot::TransferSlot};
use parse::{Sections, parse_profile, parse_self_identity, scan};
use profile_cache::{clear_profile, copy_profile, previous_body, publish_profile};
#[cfg(test)]
use request_tracking::{DARPC_ORIGIN_TTL_MS, IN_FLIGHT_TIMEOUT_MS, USER_ORIGIN_TTL_MS};
use request_tracking::{
    ORIGIN_DARPC, ORIGIN_USER, Origin, Pending, ResponseKind, enqueue, pop_pending, prune_origins,
    push_origin, take_internal_origin, take_origin,
};

const RESPONSE_OPCODE: u8 = 0x34;
const REQUEST_OPCODE: u8 = 0x43;
const REQUEST_SUBTYPE: u8 = 1;
const BODY_CAPACITY: usize = u16::MAX as usize;
const PROFILE_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 4;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;
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
static SUBMITTING_SELF_LOOK: AtomicBool = AtomicBool::new(false);
static CLIENT_SELF_LOOK_PENDING: AtomicBool = AtomicBool::new(false);
static CLIENT_SELF_LOOK_RESPONSE_PENDING: AtomicBool = AtomicBool::new(false);
static CLIENT_SELF_LOOK_TICK: AtomicU32 = AtomicU32::new(0);
const CLIENT_SELF_LOOK_GRACE_MS: u32 = 5_000;
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

type RawText = InlineBytes<{ u8::MAX as usize }>;

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

static IDENTITY_EVENTS: [TransferSlot<RawIdentityEvent>; 4] = [const { TransferSlot::new() }; 4];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedCharacterProfile(u8);

pub(crate) fn reset() {
    SUBMITTING.store(false, Ordering::Release);
    SUBMIT_ID.store(0, Ordering::Release);
    SUBMIT_TRIGGER.store(0, Ordering::Release);
    SUBMIT_COMMAND_ID.store(0, Ordering::Release);
    SUBMIT_OBSERVED.store(false, Ordering::Release);
    SUBMITTING_SELF_LOOK.store(false, Ordering::Release);
    CLIENT_SELF_LOOK_PENDING.store(false, Ordering::Release);
    CLIENT_SELF_LOOK_RESPONSE_PENDING.store(false, Ordering::Release);
    CLIENT_SELF_LOOK_TICK.store(0, Ordering::Release);
    INTERCEPT_PENDING.store(false, Ordering::Release);
    request_tracking::reset();
    profile_cache::reset();
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
    IDENTITY.available.store(false, Ordering::Release);
    IDENTITY.sequence.store(0, Ordering::Release);
    for slot in &IDENTITY_EVENTS {
        slot.reset();
    }
}

pub(crate) fn appeared(player: RawWorldObject) {
    let RawWorldObject::Player { id, .. } = player else {
        return;
    };
    clear_profile(id);
    if CLIENT_SELF_LOOK_PENDING.load(Ordering::Acquire) && crate::state::self_id() == Some(id) {
        CLIENT_SELF_LOOK_PENDING.store(false, Ordering::Release);
        return;
    }
    enqueue(Pending {
        id,
        trigger: PlayerInspectionTrigger::Appeared,
        command_id: 0,
    });
}

pub(crate) fn refresh_self(id: u32) {
    enqueue(Pending {
        id,
        trigger: PlayerInspectionTrigger::Appeared,
        command_id: 0,
    });
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "production submits through the network action")
)]
pub(crate) fn request_self_look(tick_ms: u32) {
    let Some(id) = crate::state::self_id() else {
        return;
    };
    submit_self_look(
        Pending {
            id,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
        },
        tick_ms,
    );
}

fn submit_self_look(pending: Pending, tick_ms: u32) {
    if track_self_look(pending, tick_ms) {
        #[cfg(all(windows, not(test)))]
        {
            SUBMITTING_SELF_LOOK.store(true, Ordering::Release);
            let result = crate::actions::network::submit(&[0x2D]);
            SUBMITTING_SELF_LOOK.store(false, Ordering::Release);
            if result.is_err() {
                cancel_self_look(pending.id, tick_ms);
            }
        }
    }
}

pub(crate) fn observe_client_self_look(tick_ms: u32) {
    observe_client_self_look_with_self_id(crate::state::self_id(), tick_ms);
}

fn observe_client_self_look_with_self_id(self_id: Option<u32>, tick_ms: u32) {
    if SUBMITTING_SELF_LOOK.load(Ordering::Acquire) {
        return;
    }
    CLIENT_SELF_LOOK_TICK.store(tick_ms, Ordering::Release);
    CLIENT_SELF_LOOK_RESPONSE_PENDING.store(self_id.is_some(), Ordering::Release);
    CLIENT_SELF_LOOK_PENDING.store(true, Ordering::Release);
}

fn track_self_look(pending: Pending, tick_ms: u32) -> bool {
    if !request_tracking::ready_for_next(tick_ms) {
        return false;
    }
    push_origin(Origin {
        kind: ORIGIN_DARPC,
        response: ResponseKind::SelfLook,
        id: pending.id,
        trigger: pending.trigger,
        command_id: pending.command_id,
        tick_ms,
    });
    request_tracking::mark_in_flight(pending.id, tick_ms);
    true
}

#[cfg(all(windows, not(test)))]
fn cancel_self_look(id: u32, tick_ms: u32) {
    let _ = take_internal_origin(ResponseKind::SelfLook, id, tick_ms);
    request_tracking::complete(id);
}

#[cfg(not(test))]
pub(crate) fn request(command_id: u32, id: u32) -> Result<(), darpc_protocol::CommandFailure> {
    if crate::state::observed_player(id).is_none() {
        return Err(darpc_protocol::CommandFailure::InvalidTarget);
    }
    if request_tracking::upgrade(id, command_id) {
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
    request_tracking::remove(id);
}

pub(crate) fn cleared() {
    profile_cache::clear();
    request_tracking::clear_pending();
}

pub(crate) fn observe_tick(tick_ms: u32) {
    prune_origins(tick_ms);
    if crate::state::map_transition_pending() {
        return;
    }
    if reconcile_client_self_look(crate::state::self_id(), tick_ms) {
        return;
    }
    if !request_tracking::ready_for_next(tick_ms) {
        return;
    }
    let Some(pending) = pop_pending() else {
        return;
    };
    if crate::state::observed_player(pending.id).is_none() {
        return;
    }
    if pending_response_kind(pending, crate::state::self_id()) == ResponseKind::SelfLook {
        submit_self_look(pending, tick_ms);
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
        request_tracking::mark_in_flight(pending.id, tick_ms);
    }
}

fn pending_response_kind(pending: Pending, self_id: Option<u32>) -> ResponseKind {
    if self_id == Some(pending.id) {
        ResponseKind::SelfLook
    } else {
        ResponseKind::ObjectInfo
    }
}

fn reconcile_client_self_look(self_id: Option<u32>, tick_ms: u32) -> bool {
    if !CLIENT_SELF_LOOK_PENDING.load(Ordering::Acquire) {
        return false;
    }
    if tick_ms.wrapping_sub(CLIENT_SELF_LOOK_TICK.load(Ordering::Acquire))
        > CLIENT_SELF_LOOK_GRACE_MS
    {
        CLIENT_SELF_LOOK_PENDING.store(false, Ordering::Release);
        return false;
    }
    let Some(id) = self_id else {
        return request_tracking::next_is_automatic();
    };
    if request_tracking::remove_automatic(id) {
        CLIENT_SELF_LOOK_PENDING.store(false, Ordering::Release);
    }
    false
}

pub(crate) fn observe_request(id: u32, tick_ms: u32) {
    let internal = SUBMITTING.load(Ordering::Acquire) && SUBMIT_ID.load(Ordering::Acquire) == id;
    let origin = if internal {
        SUBMIT_OBSERVED.store(true, Ordering::Release);
        Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id,
            trigger: trigger_from_raw(SUBMIT_TRIGGER.load(Ordering::Acquire)),
            command_id: SUBMIT_COMMAND_ID.load(Ordering::Acquire),
            tick_ms,
        }
    } else {
        Origin {
            kind: ORIGIN_USER,
            response: ResponseKind::ObjectInfo,
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
    let Some(origin) = take_origin(ResponseKind::ObjectInfo, id, tick_ms) else {
        return false;
    };
    if origin.kind != ORIGIN_DARPC {
        return false;
    }
    request_tracking::complete(id);
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
    observe_self_look_with_self_id(crate::state::self_id(), body, tick_ms);
}

fn observe_self_look_with_self_id(self_id: Option<u32>, body: &[u8], tick_ms: u32) {
    let Some(current) = parse_self_identity(body) else {
        return;
    };
    CLIENT_SELF_LOOK_RESPONSE_PENDING.store(false, Ordering::Release);
    if let Some(id) = self_id {
        let _ = complete_self_request(id, tick_ms);
    }
    observe_self_identity(current, tick_ms);
}

pub(crate) fn intercept_self_response(body: &[u8], tick_ms: u32) -> bool {
    intercept_self_response_with_self_id(crate::state::self_id(), body, tick_ms)
}

fn intercept_self_response_with_self_id(self_id: Option<u32>, body: &[u8], tick_ms: u32) -> bool {
    // The first login snapshot can still be in transition when the local
    // player's 0x33 draw queues its automatic inspection. The resulting 0x39
    // response has no target ID, so use that one active request until the
    // state cache has captured the local character ID.
    let Some(id) = self_id.or_else(request_tracking::in_flight_id) else {
        return false;
    };
    intercept_self_response_for(id, body, tick_ms)
}

fn intercept_self_response_for(id: u32, body: &[u8], tick_ms: u32) -> bool {
    let Some(current) = parse_self_identity(body) else {
        return false;
    };
    // A user request must refresh the client's native self-profile cache. The
    // normal post-dispatch observer will still complete any internal request.
    if CLIENT_SELF_LOOK_RESPONSE_PENDING.swap(false, Ordering::AcqRel)
        && tick_ms.wrapping_sub(CLIENT_SELF_LOOK_TICK.load(Ordering::Acquire))
            <= CLIENT_SELF_LOOK_GRACE_MS
    {
        return false;
    }
    if !complete_self_request(id, tick_ms) {
        return false;
    }
    observe_self_identity(current, tick_ms);
    crate::group::observe_packet(body, tick_ms);
    true
}

fn observe_self_identity(current: RawIdentity, tick_ms: u32) {
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

fn complete_self_request(id: u32, tick_ms: u32) -> bool {
    request_tracking::remove(id);
    // The server answers an object-info request for the local player with a
    // self-look response. This occurs for the automatic inspection queued by
    // the local player's 0x33 login draw, so accept either tracked response
    // kind for the same entity while retaining ID-scoped isolation.
    let Some(origin) = take_internal_origin(ResponseKind::SelfLook, id, tick_ms)
        .or_else(|| take_internal_origin(ResponseKind::ObjectInfo, id, tick_ms))
    else {
        return false;
    };
    request_tracking::complete(id);
    if origin.command_id != 0 {
        crate::commands::fail_player(origin.command_id);
    }
    let _ = origin.command_id;
    true
}

pub(crate) fn self_identity() -> Option<PlayerIdentity> {
    raw_identity().and_then(identity_model)
}

pub(crate) fn take_identity(queued: QueuedCharacterProfile) -> Option<CharacterProfileUpdate> {
    let slot = IDENTITY_EVENTS.get(usize::from(queued.0))?;
    let raw = slot.try_take()?;
    Some(CharacterProfileUpdate {
        previous: raw.previous.and_then(identity_model),
        current: identity_model(raw.current)?,
    })
}

pub(crate) fn release_identity(queued: QueuedCharacterProfile) {
    if let Some(slot) = IDENTITY_EVENTS.get(usize::from(queued.0)) {
        slot.discard();
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
    let Some((index, _)) = IDENTITY_EVENTS
        .iter()
        .enumerate()
        .find(|(_, slot)| slot.try_write(update))
    else {
        crate::state::mark_resync_required();
        return;
    };
    let queued = QueuedCharacterProfile(u8::try_from(index).expect("identity event index fits u8"));
    if !crate::state::observe_character_profile(queued, tick_ms) {
        release_identity(queued);
    }
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

fn decode(value: &[u8]) -> Option<String> {
    crate::client_text::decode_or_empty(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn automatic_pending(id: u32) -> Pending {
        Pending {
            id,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
        }
    }

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

    fn self_look() -> Vec<u8> {
        let mut body = vec![0x39, 1, 4];
        body.extend_from_slice(b"Rank");
        body.extend_from_slice(&[5]);
        body.extend_from_slice(b"Title");
        let roster = b"Group members\n  Eidolon\n* ZiLo\nTotal 2";
        body.push(u8::try_from(roster.len()).unwrap());
        body.extend_from_slice(roster);
        body.extend_from_slice(&[1, 1]);
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
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::User,
            command_id: 0,
            tick_ms: 10,
        });
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
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
    fn self_look_response_consumes_same_target_object_info_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        request_tracking::mark_in_flight(7, 10);

        assert!(complete_self_request(7, 11));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(!intercept_response(&object_info(), 12));
    }

    #[test]
    fn login_self_look_uses_in_flight_id_before_initial_snapshot() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        crate::group::reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        request_tracking::mark_in_flight(7, 10);
        observe_client_self_look_with_self_id(None, 10);

        assert!(intercept_self_response_with_self_id(None, &self_look(), 11));
        assert!(request_tracking::ready_for_next(11));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
        assert_eq!(self_identity().unwrap().display_class, "Summoner");
    }

    #[test]
    fn client_login_self_look_defers_and_removes_automatic_self_inspection() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        observe_client_self_look(10);
        enqueue(Pending {
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
        });

        assert!(reconcile_client_self_look(None, 11));
        assert!(!reconcile_client_self_look(Some(7), 12));
        assert!(pop_pending().is_none());
        assert!(!CLIENT_SELF_LOOK_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn client_login_self_look_does_not_remove_manual_inspection() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        observe_client_self_look(10);
        enqueue(Pending {
            id: 7,
            trigger: PlayerInspectionTrigger::Manual,
            command_id: 42,
        });

        assert!(!reconcile_client_self_look(None, 11));
        assert!(!reconcile_client_self_look(Some(7), 12));
        let pending = pop_pending().expect("manual inspection remains queued");
        assert_eq!(pending.id, 7);
        assert_eq!(pending.trigger, PlayerInspectionTrigger::Manual);
        assert_eq!(pending.command_id, 42);
    }

    #[test]
    fn expired_client_self_look_does_not_block_automatic_inspection() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        observe_client_self_look(10);
        enqueue(Pending {
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
        });

        assert!(!reconcile_client_self_look(
            None,
            10 + CLIENT_SELF_LOOK_GRACE_MS + 1
        ));
        assert_eq!(pop_pending().expect("inspection remains queued").id, 7);
    }

    #[test]
    fn pre_snapshot_self_look_without_internal_request_fails_open() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();

        assert!(!intercept_self_response_with_self_id(
            None,
            &self_look(),
            11
        ));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn self_look_response_does_not_consume_other_target_object_info_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        request_tracking::mark_in_flight(7, 10);

        assert!(!complete_self_request(8, 11));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(intercept_response(&object_info(), 12));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn object_info_response_does_not_consume_self_look_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        assert!(track_self_look(automatic_pending(7), 10));

        assert!(!intercept_response(&object_info(), 11));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(complete_self_request(7, 12));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn routes_only_the_local_player_through_self_look() {
        assert!(matches!(
            pending_response_kind(automatic_pending(7), Some(7)),
            ResponseKind::SelfLook
        ));
        assert!(matches!(
            pending_response_kind(automatic_pending(8), Some(7)),
            ResponseKind::ObjectInfo
        ));
        assert!(matches!(
            pending_response_kind(automatic_pending(7), None),
            ResponseKind::ObjectInfo
        ));
    }

    #[test]
    fn expired_internal_origin_fails_open() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        assert!(!intercept_response(
            &object_info(),
            10 + DARPC_ORIGIN_TTL_MS + 1
        ));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn player_origin_expires_before_extended_internal_origin() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_USER,
            response: ResponseKind::ObjectInfo,
            id: 8,
            trigger: PlayerInspectionTrigger::User,
            command_id: 0,
            tick_ms: 10,
        });
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });

        let delayed_tick = 10 + USER_ORIGIN_TTL_MS + 1;
        prune_origins(delayed_tick);
        assert!(take_origin(ResponseKind::ObjectInfo, 8, delayed_tick).is_none());
        assert!(intercept_response(&object_info(), delayed_tick));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn delayed_internal_response_stays_suppressed_after_queue_timeout() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });
        request_tracking::mark_in_flight(7, 10);

        let delayed_tick = 10 + IN_FLIGHT_TIMEOUT_MS + 1;
        assert!(request_tracking::ready_for_next(delayed_tick));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(intercept_response(&object_info(), delayed_tick));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn minute_late_internal_response_stays_suppressed() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        push_origin(Origin {
            kind: ORIGIN_DARPC,
            response: ResponseKind::ObjectInfo,
            id: 7,
            trigger: PlayerInspectionTrigger::Appeared,
            command_id: 0,
            tick_ms: 10,
        });

        let delayed_tick = 10 + 60_000;
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(intercept_response(&object_info(), delayed_tick));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
    }

    #[test]
    fn full_correlation_capacity_preserves_oldest_internal_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        for offset in 0..128_u32 {
            push_origin(Origin {
                kind: ORIGIN_DARPC,
                response: ResponseKind::ObjectInfo,
                id: 7 + offset,
                trigger: PlayerInspectionTrigger::Appeared,
                command_id: 0,
                tick_ms: 10,
            });
        }

        assert!(intercept_response(&object_info(), 11));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));
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

    #[test]
    fn internal_self_look_is_observed_and_suppressed() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        crate::group::reset();
        assert!(track_self_look(automatic_pending(7), 10));
        assert!(!track_self_look(automatic_pending(7), 10));

        let body = self_look();

        assert!(intercept_self_response_for(7, &body, 11));

        assert!(request_tracking::ready_for_next(11));
        assert!(pop_pending().is_none());
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
        assert_eq!(self_identity().unwrap().display_class, "Summoner");
        assert_eq!(crate::group::member_count(), 2);
        assert!(!intercept_self_response_for(7, &body, 12));
    }

    #[test]
    fn user_self_look_response_precedes_internal_suppression() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        crate::group::reset();
        assert!(track_self_look(automatic_pending(7), 10));
        observe_client_self_look_with_self_id(Some(7), 11);

        let body = self_look();
        assert!(!intercept_self_response_for(7, &body, 12));
        assert!(INTERCEPT_PENDING.load(Ordering::Acquire));

        observe_self_look_with_self_id(Some(7), &body, 12);
        assert!(request_tracking::ready_for_next(12));
        assert!(!INTERCEPT_PENDING.load(Ordering::Acquire));
        assert!(!CLIENT_SELF_LOOK_RESPONSE_PENDING.load(Ordering::Acquire));
    }
}
