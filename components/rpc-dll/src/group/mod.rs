#![cfg_attr(not(windows), allow(dead_code))]

use darpc_game_client::{
    GROUP_INVITATION_CAPACITY, GROUP_NAME_BYTES, RawGroupInvitation, RawGroupMember, RawGroupState,
};
use darpc_model::{
    GroupInvitation, GroupInvitationCloseReason, GroupMember, GroupState, GroupUpdate,
};
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

const EVENT_SLOT_COUNT: usize = 16;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

static NEXT_INVITATION_ID: AtomicU32 = AtomicU32::new(1);
static TRACKER: TrackerCell = TrackerCell(UnsafeCell::new(RawGroupState::empty()));
static EVENTS: [EventSlot; EVENT_SLOT_COUNT] = [const { EventSlot::new() }; EVENT_SLOT_COUNT];

struct TrackerCell(UnsafeCell<RawGroupState>);

// SAFETY: only the client main thread accesses the tracker. Snapshot capture,
// packet observation, and command execution are all serialized by the tick.
unsafe impl Sync for TrackerCell {}

struct EventSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<RawGroupUpdate>>,
}

// SAFETY: each slot's atomic state transfers exclusive ownership from the
// client main-thread producer to the IPC consumer.
unsafe impl Sync for EventSlot {}

impl EventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedGroup(u8);

#[derive(Clone, Copy)]
struct RawGroupUpdate {
    kind: RawGroupUpdateKind,
    state: RawGroupState,
}

#[derive(Clone, Copy)]
#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
enum RawGroupUpdateKind {
    SettingsChanged,
    InvitationSent(RawName),
    InvitationReceived(RawGroupInvitation),
    InvitationClosed(RawGroupInvitation, GroupInvitationCloseReason),
    Joined,
    MemberJoined(RawGroupMember),
    MemberLeft(RawGroupMember),
    Disbanded,
}

#[derive(Clone, Copy)]
struct RawName {
    bytes: [u8; GROUP_NAME_BYTES],
    length: u8,
}

pub(crate) fn reset() {
    NEXT_INVITATION_ID.store(1, Ordering::Relaxed);
    // SAFETY: lifecycle reset runs without an installed producer or consumer.
    unsafe { *TRACKER.0.get() = RawGroupState::empty() };
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn merge_snapshot(group: &mut RawGroupState, available: bool) {
    if !available {
        // SAFETY: snapshot capture runs on the sole producer thread.
        unsafe { *TRACKER.0.get() = RawGroupState::empty() };
        return;
    }
    // SAFETY: snapshot capture runs on the sole producer thread.
    let tracked = unsafe { &*TRACKER.0.get() };
    group.invitation_count = tracked.invitation_count;
    group.invitations = tracked.invitations;
    group.is_group_open = tracked.is_group_open.or(group.is_group_open);
    group.auto_accept = tracked.auto_accept;
    // SAFETY: snapshot capture runs on the sole producer thread.
    unsafe { *TRACKER.0.get() = *group };
}

pub(crate) fn model_state(raw: &RawGroupState) -> GroupState {
    GroupState {
        members: raw
            .members
            .iter()
            .copied()
            .take(usize::from(raw.member_count))
            .filter_map(member_model)
            .collect(),
        invitations: raw
            .invitations
            .iter()
            .copied()
            .take(usize::from(raw.invitation_count))
            .filter_map(invitation_model)
            .collect(),
        is_group_open: raw.is_group_open,
        auto_accept: raw.auto_accept,
    }
}

pub(crate) fn observe_packet(body: &[u8], tick_ms: u32) {
    match body.first().copied() {
        Some(0x63) => observe_invitation_packet(body, tick_ms),
        Some(0x39) => {
            crate::legend::observe_self_look(body, tick_ms);
            observe_self_look(body, tick_ms);
        }
        _ => {}
    }
}

pub(crate) fn observe_pending(name: &[u8], received_tick_ms: Option<u32>, tick_ms: u32) {
    let Some(raw_name) = raw_name(name) else {
        return;
    };
    // SAFETY: UI observation runs on the sole producer thread.
    let state = unsafe { &mut *TRACKER.0.get() };
    let count = usize::from(state.invitation_count);
    if state.invitations[..count].iter().any(|invitation| {
        invitation.inviter_len == raw_name.length
            && invitation.inviter[..usize::from(invitation.inviter_len)].eq_ignore_ascii_case(name)
    }) {
        return;
    }
    if count == GROUP_INVITATION_CAPACITY {
        crate::state::mark_resync_required();
        return;
    }
    let invitation = RawGroupInvitation {
        id: next_invitation_id(),
        inviter: raw_name.bytes,
        inviter_len: raw_name.length,
        received_tick_ms,
    };
    state.invitations[count] = invitation;
    state.invitation_count += 1;
    queue(
        RawGroupUpdate {
            kind: RawGroupUpdateKind::InvitationReceived(invitation),
            state: *state,
        },
        tick_ms,
    );
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn reconcile_invitations(mut is_open: impl FnMut(&[u8]) -> bool, tick_ms: u32) {
    loop {
        // SAFETY: UI observation runs on the sole producer thread.
        let state = unsafe { &*TRACKER.0.get() };
        let closed = state
            .invitations
            .iter()
            .copied()
            .take(usize::from(state.invitation_count))
            .find(|invitation| {
                !is_open(&invitation.inviter[..usize::from(invitation.inviter_len)])
            });
        let Some(closed) = closed else { break };
        close_invitation(closed.id, GroupInvitationCloseReason::Dismissed, tick_ms);
    }
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn observe_sent(target: &[u8], tick_ms: u32) {
    let Some(target) = raw_name(target) else {
        return;
    };
    // SAFETY: command execution runs on the sole producer thread.
    let state = unsafe { *TRACKER.0.get() };
    queue(
        RawGroupUpdate {
            kind: RawGroupUpdateKind::InvitationSent(target),
            state,
        },
        tick_ms,
    );
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn invitation(id: u32) -> Option<RawGroupInvitation> {
    // SAFETY: command execution runs on the sole producer thread.
    let state = unsafe { &*TRACKER.0.get() };
    state
        .invitations
        .iter()
        .copied()
        .take(usize::from(state.invitation_count))
        .find(|invitation| invitation.id == id)
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn member_count() -> u8 {
    // SAFETY: roster refresh runs on the sole producer thread.
    unsafe { (*TRACKER.0.get()).member_count }
}

#[cfg_attr(
    test,
    expect(dead_code, reason = "called by the production-only actions module")
)]
pub(crate) fn close_invitation(id: u32, reason: GroupInvitationCloseReason, tick_ms: u32) -> bool {
    // SAFETY: command execution and packet observation run on the sole producer thread.
    let state = unsafe { &mut *TRACKER.0.get() };
    let count = usize::from(state.invitation_count);
    let Some(index) = state.invitations[..count]
        .iter()
        .position(|invitation| invitation.id == id)
    else {
        return false;
    };
    let invitation = state.invitations[index];
    state.invitations.copy_within(index + 1..count, index);
    state.invitations[count - 1] = RawGroupInvitation::empty();
    state.invitation_count -= 1;
    queue(
        RawGroupUpdate {
            kind: RawGroupUpdateKind::InvitationClosed(invitation, reason),
            state: *state,
        },
        tick_ms,
    );
    true
}

pub(crate) fn take(queued: QueuedGroup) -> Option<GroupUpdate> {
    let slot = EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive ownership of the value.
    let raw = unsafe { (*slot.value.get()).assume_init_read() };
    slot.state.store(EMPTY, Ordering::Release);
    let state = model_state(&raw.state);
    Some(match raw.kind {
        RawGroupUpdateKind::SettingsChanged => GroupUpdate::SettingsChanged { state },
        RawGroupUpdateKind::InvitationSent(target) => GroupUpdate::InvitationSent {
            target: decode(&target.bytes[..usize::from(target.length)])?,
        },
        RawGroupUpdateKind::InvitationReceived(invitation) => GroupUpdate::InvitationReceived {
            invitation: invitation_model(invitation)?,
            state,
        },
        RawGroupUpdateKind::InvitationClosed(invitation, reason) => GroupUpdate::InvitationClosed {
            invitation: invitation_model(invitation)?,
            reason,
            state,
        },
        RawGroupUpdateKind::Joined => GroupUpdate::Joined { state },
        RawGroupUpdateKind::MemberJoined(member) => GroupUpdate::MemberJoined {
            member: member_model(member)?,
            state,
        },
        RawGroupUpdateKind::MemberLeft(member) => GroupUpdate::MemberLeft {
            member: member_model(member)?,
            state,
        },
        RawGroupUpdateKind::Disbanded => GroupUpdate::Disbanded { state },
    })
}

pub(crate) fn release(queued: QueuedGroup) {
    if let Some(slot) = EVENTS.get(usize::from(queued.0)) {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

fn observe_invitation_packet(body: &[u8], tick_ms: u32) {
    if body.get(1) != Some(&1) {
        return;
    }
    let Some(length) = body.get(2).copied().map(usize::from) else {
        return;
    };
    let Some(name) = body.get(3..3 + length) else {
        return;
    };
    #[cfg(all(windows, not(test)))]
    {
        // The central event hook runs after the native handler, so absence of
        // a prompt means GroupAnswer consumed the invitation automatically.
        if !crate::actions::group::is_open(name) {
            // SAFETY: packet observation runs on the sole producer thread.
            unsafe { (*TRACKER.0.get()).auto_accept = Some(true) };
            crate::actions::group::schedule_roster_refresh(tick_ms);
            return;
        }
        // SAFETY: a visible prompt establishes that automatic acceptance is off.
        unsafe { (*TRACKER.0.get()).auto_accept = Some(false) };
    }
    observe_pending(name, Some(tick_ms), tick_ms);
}

fn observe_self_look(body: &[u8], tick_ms: u32) {
    #[cfg(not(windows))]
    {
        let _ = (body, tick_ms);
    }
    #[cfg(windows)]
    {
        let is_group_open = parse_self_look_open(body);
        let mut captured = RawGroupState::empty();
        if crate::snapshot::capture_group(&mut captured).is_err() {
            crate::state::mark_resync_required();
            return;
        }
        // SAFETY: packet observation runs on the sole producer thread.
        let current = unsafe { &mut *TRACKER.0.get() };
        captured.invitations = current.invitations;
        captured.invitation_count = current.invitation_count;
        captured.is_group_open = is_group_open.or(current.is_group_open);
        captured.auto_accept = current.auto_accept;
        let previous = *current;
        *current = captured;

        let old_count = usize::from(previous.member_count);
        let new_count = usize::from(captured.member_count);
        if old_count == 0 && new_count != 0 {
            queue(
                RawGroupUpdate {
                    kind: RawGroupUpdateKind::Joined,
                    state: captured,
                },
                tick_ms,
            );
        } else if old_count != 0 && new_count == 0 {
            queue(
                RawGroupUpdate {
                    kind: RawGroupUpdateKind::Disbanded,
                    state: captured,
                },
                tick_ms,
            );
        } else {
            for member in captured.members.iter().copied().take(new_count) {
                if !contains_member(&previous, member) {
                    queue(
                        RawGroupUpdate {
                            kind: RawGroupUpdateKind::MemberJoined(member),
                            state: captured,
                        },
                        tick_ms,
                    );
                }
            }
            for member in previous.members.iter().copied().take(old_count) {
                if !contains_member(&captured, member) {
                    queue(
                        RawGroupUpdate {
                            kind: RawGroupUpdateKind::MemberLeft(member),
                            state: captured,
                        },
                        tick_ms,
                    );
                }
            }
        }
        if previous.is_group_open != captured.is_group_open {
            queue(
                RawGroupUpdate {
                    kind: RawGroupUpdateKind::SettingsChanged,
                    state: captured,
                },
                tick_ms,
            );
        }
    }
}

fn parse_self_look_open(body: &[u8]) -> Option<bool> {
    if body.first() != Some(&0x39) {
        return None;
    }
    let mut offset = 2;
    for _ in 0..3 {
        let length = usize::from(*body.get(offset)?);
        offset = offset.checked_add(1 + length)?;
    }
    body.get(offset).map(|value| *value != 0)
}

fn contains_member(state: &RawGroupState, wanted: RawGroupMember) -> bool {
    state
        .members
        .iter()
        .take(usize::from(state.member_count))
        .any(|member| {
            member.name_len == wanted.name_len
                && member.name[..usize::from(member.name_len)]
                    .eq_ignore_ascii_case(&wanted.name[..usize::from(wanted.name_len)])
        })
}

fn queue(update: RawGroupUpdate, tick_ms: u32) {
    let Some((index, slot)) = EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }) else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives this producer exclusive ownership of the slot.
    unsafe { (*slot.value.get()).write(update) };
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedGroup(u8::try_from(index).expect("group event slot index fits u8"));
    if !crate::state::observe_group(queued, tick_ms) {
        release(queued);
    }
}

fn next_invitation_id() -> u32 {
    loop {
        let id = NEXT_INVITATION_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

fn raw_name(value: &[u8]) -> Option<RawName> {
    if value.is_empty() || value.len() > GROUP_NAME_BYTES {
        return None;
    }
    let mut bytes = [0; GROUP_NAME_BYTES];
    bytes[..value.len()].copy_from_slice(value);
    Some(RawName {
        bytes,
        length: u8::try_from(value.len()).ok()?,
    })
}

fn member_model(raw: RawGroupMember) -> Option<GroupMember> {
    Some(GroupMember {
        name: decode(&raw.name[..usize::from(raw.name_len)])?,
        is_leader: raw.is_leader,
    })
}

fn invitation_model(raw: RawGroupInvitation) -> Option<GroupInvitation> {
    (raw.id != 0).then(|| GroupInvitation {
        id: raw.id,
        inviter: decode(&raw.inviter[..usize::from(raw.inviter_len)]).unwrap_or_default(),
        received_tick_ms: raw.received_tick_ms,
    })
}

#[cfg(windows)]
fn decode(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}
