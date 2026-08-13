#![cfg_attr(not(windows), allow(dead_code))]

use darpc_game_client::{
    GROUP_INVITATION_CAPACITY, GROUP_NAME_BYTES, RawGroupInvitation, RawGroupMember, RawGroupState,
};
use darpc_model::{
    GroupInvitation, GroupInvitationCloseReason, GroupMember, GroupState, GroupUpdate, MessageKind,
};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{inline_bytes::InlineBytes, transfer_slot::TransferSlot};

const EVENT_SLOT_COUNT: usize = 16;
const GROUP_MEMBERS_HEADER: &[u8] = b"Group Members";
const GROUP_TOTAL_HEADER: &[u8] = b"Total ";
const ADVENTURING_ALONE: &[u8] = b"Adventuring alone";
const GROUP_DISBANDED_MESSAGE: &[u8] = b"Group disbanded.";
const GROUP_JOINED_SUFFIX: &[u8] = b" is joining this group.";

static NEXT_INVITATION_ID: AtomicU32 = AtomicU32::new(1);
static TRACKER: TrackerCell = TrackerCell(UnsafeCell::new(RawGroupState::empty()));
static EVENTS: [TransferSlot<RawGroupUpdate>; EVENT_SLOT_COUNT] =
    [const { TransferSlot::new() }; EVENT_SLOT_COUNT];

struct TrackerCell(UnsafeCell<RawGroupState>);

// SAFETY: only the client main thread accesses the tracker. Snapshot capture,
// packet observation, and command execution are all serialized by the tick.
unsafe impl Sync for TrackerCell {}

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

type RawName = InlineBytes<GROUP_NAME_BYTES>;

pub(crate) fn reset() {
    NEXT_INVITATION_ID.store(1, Ordering::Relaxed);
    // SAFETY: lifecycle reset runs without an installed producer or consumer.
    unsafe { *TRACKER.0.get() = RawGroupState::empty() };
    for slot in &EVENTS {
        slot.reset();
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
    group.member_count = tracked.member_count;
    group.members = tracked.members;
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

pub(crate) fn observe_message(kind: MessageKind, text: &[u8], tick_ms: u32) {
    if !is_roster_confirmation(kind, text) {
        return;
    }
    #[cfg(all(windows, not(test)))]
    crate::actions::group::schedule_roster_refresh(tick_ms);
    #[cfg(any(not(windows), test))]
    let _ = tick_ms;
}

pub(crate) fn observe_pending(name: &[u8], received_tick_ms: Option<u32>, tick_ms: u32) {
    let Some(raw_name) = raw_name(name) else {
        return;
    };
    // SAFETY: UI observation runs on the sole producer thread.
    let state = unsafe { &mut *TRACKER.0.get() };
    let count = usize::from(state.invitation_count);
    if state.invitations[..count].iter().any(|invitation| {
        usize::from(invitation.inviter_len) == raw_name.len()
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
        inviter: raw_name.into_array(),
        inviter_len: raw_name.len_u8().expect("group name length fits u8"),
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
    let raw = slot.try_take()?;
    let state = model_state(&raw.state);
    Some(match raw.kind {
        RawGroupUpdateKind::SettingsChanged => GroupUpdate::SettingsChanged { state },
        RawGroupUpdateKind::InvitationSent(target) => GroupUpdate::InvitationSent {
            target: decode(target.as_bytes())?,
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
        slot.discard();
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
            return;
        }
        // SAFETY: a visible prompt establishes that automatic acceptance is off.
        unsafe { (*TRACKER.0.get()).auto_accept = Some(false) };
    }
    observe_pending(name, Some(tick_ms), tick_ms);
}

fn is_roster_confirmation(kind: MessageKind, text: &[u8]) -> bool {
    if kind != MessageKind::System {
        return false;
    }
    if text.eq_ignore_ascii_case(GROUP_DISBANDED_MESSAGE) {
        return true;
    }
    let Some(name_length) = text.len().checked_sub(GROUP_JOINED_SUFFIX.len()) else {
        return false;
    };
    let (name, suffix) = text.split_at(name_length);
    suffix.eq_ignore_ascii_case(GROUP_JOINED_SUFFIX) && raw_name(name).is_some()
}

fn observe_self_look(body: &[u8], tick_ms: u32) {
    let Some(mut captured) = parse_self_look(body) else {
        return;
    };
    // SAFETY: packet observation runs on the sole producer thread.
    let current = unsafe { &mut *TRACKER.0.get() };
    captured.invitations = current.invitations;
    captured.invitation_count = current.invitation_count;
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

fn parse_self_look(body: &[u8]) -> Option<RawGroupState> {
    if body.first() != Some(&0x39) {
        return None;
    }
    let mut offset = 2;
    take_string(body, &mut offset)?;
    take_string(body, &mut offset)?;
    let roster = take_string(body, &mut offset)?;
    let mut state = RawGroupState::empty();
    state.is_group_open = Some(*body.get(offset)? != 0);
    parse_roster(roster, &mut state)?;
    Some(state)
}

fn parse_roster(value: &[u8], state: &mut RawGroupState) -> Option<()> {
    let value = trim_ascii(value);
    if value.is_empty() || find_ascii_case_insensitive(value, ADVENTURING_ALONE).is_some() {
        return Some(());
    }

    let header = find_ascii_case_insensitive(value, GROUP_MEMBERS_HEADER)?;
    let roster = trim_ascii(value.get(header + GROUP_MEMBERS_HEADER.len()..)?);
    let roster = roster.strip_prefix(b":").map_or(roster, trim_ascii);
    let lines = roster.split(|byte| matches!(*byte, b'\r' | b'\n'));
    let mut leader_seen = false;
    let mut total_seen = false;
    for line in lines {
        let mut name = trim_ascii(line);
        if let Some(total) = parse_total(name) {
            if total_seen || total != state.member_count {
                return None;
            }
            total_seen = true;
            continue;
        }
        let is_leader = if let Some(remainder) = name.strip_prefix(b"*") {
            name = trim_ascii(remainder);
            if leader_seen {
                return None;
            }
            leader_seen = true;
            true
        } else {
            false
        };
        if name.is_empty() {
            continue;
        }
        if total_seen {
            return None;
        }
        let index = usize::from(state.member_count);
        let member = state.members.get_mut(index)?;
        let raw = raw_name(name)?;
        member.name[..raw.len()].copy_from_slice(raw.as_bytes());
        member.name_len = u8::try_from(raw.len()).expect("group name length fits u8");
        member.is_leader = is_leader;
        state.member_count = state.member_count.checked_add(1)?;
    }
    (state.member_count != 0).then_some(())
}

fn parse_total(value: &[u8]) -> Option<u8> {
    let (header, digits) = value.split_at_checked(GROUP_TOTAL_HEADER.len())?;
    if !header.eq_ignore_ascii_case(GROUP_TOTAL_HEADER) || digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0_u8, |total, byte| {
        let digit = byte.checked_sub(b'0').filter(|digit| *digit <= 9)?;
        total.checked_mul(10)?.checked_add(digit)
    })
}

fn find_ascii_case_insensitive(value: &[u8], needle: &[u8]) -> Option<usize> {
    value
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn take_string<'a>(body: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let length = usize::from(*body.get(*offset)?);
    *offset = offset.checked_add(1)?;
    let end = offset.checked_add(length)?;
    let value = body.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
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
    let Some((index, _)) = EVENTS
        .iter()
        .enumerate()
        .find(|(_, slot)| slot.try_write(update))
    else {
        crate::state::mark_resync_required();
        return;
    };
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
    RawName::try_nonempty(value)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn self_look(roster: &[u8], is_group_open: bool) -> Vec<u8> {
        let mut body = vec![0x39, 1, 0, 0];
        body.push(u8::try_from(roster.len()).expect("test roster fits string8"));
        body.extend_from_slice(roster);
        body.push(u8::from(is_group_open));
        body
    }

    #[test]
    fn parses_group_members_and_leader_from_self_look() {
        let state = parse_self_look(&self_look(
            b"Group members\n  Eidolon\n* ZiLo\nTotal 2",
            true,
        ))
        .expect("valid self look");

        assert_eq!(state.is_group_open, Some(true));
        assert_eq!(state.member_count, 2);
        assert_eq!(member_model(state.members[0]).unwrap().name, "Eidolon");
        assert!(!state.members[0].is_leader);
        assert_eq!(member_model(state.members[1]).unwrap().name, "ZiLo");
        assert!(state.members[1].is_leader);
    }

    #[test]
    fn parses_crlf_group_members_without_a_leader_marker() {
        let state = parse_self_look(&self_look(b"Group Members:\r\nZiLo\r\nEidolon\r\n", false))
            .expect("valid self look");

        assert_eq!(state.is_group_open, Some(false));
        assert_eq!(state.member_count, 2);
        assert!(!state.members[0].is_leader);
        assert!(!state.members[1].is_leader);
    }

    #[test]
    fn parses_control_prefixed_roster_with_cr_line_endings() {
        let state = parse_self_look(&self_look(b"\x01Group Members:\r*ZiLo\rEidolon\r", false))
            .expect("valid self look");

        assert_eq!(state.member_count, 2);
        assert_eq!(member_model(state.members[0]).unwrap().name, "ZiLo");
        assert!(state.members[0].is_leader);
        assert_eq!(member_model(state.members[1]).unwrap().name, "Eidolon");
    }

    #[test]
    fn parses_adventuring_alone_as_an_empty_roster() {
        let state =
            parse_self_look(&self_look(b"Adventuring alone", true)).expect("valid solo self look");

        assert_eq!(state.member_count, 0);
        assert_eq!(state.is_group_open, Some(true));
    }

    #[test]
    fn rejects_unknown_roster_text_and_multiple_leaders() {
        assert!(parse_self_look(&self_look(b"Unknown", true)).is_none());
        assert!(parse_self_look(&self_look(b"Group Members:\n*ZiLo\n*Eidolon", true)).is_none());
        assert!(parse_self_look(&self_look(b"Group Members\nZiLo\nTotal 2", true)).is_none());
    }

    #[test]
    fn recognizes_server_confirmed_group_changes() {
        assert!(is_roster_confirmation(
            MessageKind::System,
            b"Group disbanded."
        ));
        assert!(is_roster_confirmation(
            MessageKind::System,
            b"ZiLo is joining this group."
        ));
        assert!(!is_roster_confirmation(
            MessageKind::Group,
            b"ZiLo is joining this group."
        ));
        assert!(!is_roster_confirmation(
            MessageKind::System,
            b" is joining this group."
        ));
        assert!(!is_roster_confirmation(
            MessageKind::System,
            b"ZiLo joined some other group."
        ));
    }
}
