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
use request_tracking::ORIGIN_TTL_MS;
use request_tracking::{
    ORIGIN_DARPC, ORIGIN_USER, Origin, Pending, enqueue, pop_pending, prune_origins, push_origin,
    take_origin,
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
    if !request_tracking::ready_for_next(tick_ms) {
        return;
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
        request_tracking::mark_in_flight(pending.id, tick_ms);
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
