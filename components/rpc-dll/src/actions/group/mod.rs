use super::{module_base, network, read};
use darpc_game_client::{
    EVENT_DISPATCHER_POINTER_RVA, GROUP_INVITATION_CAPACITY, GROUP_NAME_BYTES,
};
use darpc_model::GroupInvitationCloseReason;
use darpc_protocol::{CommandFailure, GroupCommand, GroupInvitationAction};
use std::{
    ffi::c_void,
    mem,
    sync::atomic::{AtomicU32, Ordering},
};

const GROUP_ALERT_COL_RVA: usize = 0x002A_81C4;
const ALERT_ACTION_RVA: usize = 0x0004_8770;
const ENTRIES_OFFSET: usize = 0x64;
const COUNT_OFFSET: usize = 0x68;
const CAPACITY_OFFSET: usize = 0x6C;
const ENTRY_SIZE: usize = 0x0C;
const VISIBLE_OFFSET: usize = 0x130;
const REGISTRATION_OFFSET: usize = 0x188;
const REGISTERED: u8 = 0x02;
const REQUESTER_OFFSET: usize = 0x634;
const REQUESTER_LENGTH_OFFSET: usize = 0x638;
const MAX_PANES: i32 = 1_024;
const ALERT_SCAN_INTERVAL_MS: u32 = 100;
const ROSTER_REFRESH_INTERVAL_MS: u32 = 2_000;
const INVITATION_REFRESH_WINDOW_MS: u32 = 30_000;

static NEXT_ROSTER_REFRESH: AtomicU32 = AtomicU32::new(0);
static REFRESH_UNTIL: AtomicU32 = AtomicU32::new(0);
static LAST_ALERT_SCAN: AtomicU32 = AtomicU32::new(0);

struct OpenAlerts {
    names: [[u8; GROUP_NAME_BYTES]; GROUP_INVITATION_CAPACITY],
    lengths: [u8; GROUP_INVITATION_CAPACITY],
    count: usize,
}

impl OpenAlerts {
    const fn new() -> Self {
        Self {
            names: [[0; GROUP_NAME_BYTES]; GROUP_INVITATION_CAPACITY],
            lengths: [0; GROUP_INVITATION_CAPACITY],
            count: 0,
        }
    }

    fn push(&mut self, name: &[u8]) {
        if self.count == GROUP_INVITATION_CAPACITY || name.len() > GROUP_NAME_BYTES {
            return;
        }
        self.names[self.count][..name.len()].copy_from_slice(name);
        self.lengths[self.count] = u8::try_from(name.len()).expect("group name length fits u8");
        self.count += 1;
    }

    fn contains(&self, name: &[u8]) -> bool {
        self.names
            .iter()
            .zip(self.lengths)
            .take(self.count)
            .any(|(candidate, length)| candidate[..usize::from(length)].eq_ignore_ascii_case(name))
    }
}

type AlertActionFn = unsafe extern "thiscall" fn(*mut c_void, i32, u8) -> u32;

pub(super) fn submit(command: GroupCommand) -> Result<(), CommandFailure> {
    match command {
        GroupCommand::Toggle => toggle(),
        GroupCommand::Invite(target) => invite(target.as_bytes()),
        GroupCommand::Respond {
            invitation_id,
            action,
        } => respond(invitation_id, action),
    }
}

fn toggle() -> Result<(), CommandFailure> {
    network::submit(&[0x2F])
}

pub(crate) fn observe_tick(tick_ms: u32) {
    refresh_roster(tick_ms);
    let last = LAST_ALERT_SCAN.load(Ordering::Relaxed);
    if !alert_scan_due(last, tick_ms) {
        return;
    }
    LAST_ALERT_SCAN.store(tick_ms, Ordering::Relaxed);

    let mut open = OpenAlerts::new();
    for_each_open(|_, name| {
        open.push(name);
        crate::group::observe_pending(name, None, tick_ms);
    });
    crate::group::reconcile_invitations(|name| open.contains(name), tick_ms);
}

const fn alert_scan_due(last: u32, now: u32) -> bool {
    last == 0 || now.wrapping_sub(last) >= ALERT_SCAN_INTERVAL_MS
}

pub(crate) fn reset() {
    NEXT_ROSTER_REFRESH.store(0, Ordering::Relaxed);
    REFRESH_UNTIL.store(0, Ordering::Relaxed);
    LAST_ALERT_SCAN.store(0, Ordering::Relaxed);
}

pub(crate) fn is_open(name: &[u8]) -> bool {
    find(name).is_some()
}

fn invite(target: &[u8]) -> Result<(), CommandFailure> {
    let length = u8::try_from(target.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let mut body = [0_u8; 31];
    body[0] = 0x2E;
    body[1] = 0x02;
    body[2] = length;
    body[3..3 + target.len()].copy_from_slice(target);
    network::submit(&body[..3 + target.len()])?;
    crate::group::observe_sent(target, now());
    Ok(())
}

pub(crate) fn schedule_roster_refresh(tick_ms: u32) {
    NEXT_ROSTER_REFRESH.store(
        tick_ms.wrapping_add(ROSTER_REFRESH_INTERVAL_MS),
        Ordering::Relaxed,
    );
    REFRESH_UNTIL.store(
        tick_ms.wrapping_add(INVITATION_REFRESH_WINDOW_MS),
        Ordering::Relaxed,
    );
}

fn refresh_roster(tick_ms: u32) {
    let grouped = crate::group::member_count() != 0;
    let next = NEXT_ROSTER_REFRESH.load(Ordering::Relaxed);
    if next == 0 {
        if grouped {
            NEXT_ROSTER_REFRESH.store(
                tick_ms.wrapping_add(ROSTER_REFRESH_INTERVAL_MS),
                Ordering::Relaxed,
            );
        }
        return;
    }
    if tick_ms.wrapping_sub(next) >= i32::MAX as u32 {
        return;
    }
    let until = REFRESH_UNTIL.load(Ordering::Relaxed);
    if !grouped && (until == 0 || tick_ms.wrapping_sub(until) < i32::MAX as u32) {
        NEXT_ROSTER_REFRESH.store(0, Ordering::Relaxed);
        REFRESH_UNTIL.store(0, Ordering::Relaxed);
        return;
    }
    crate::player::request_self_look(tick_ms);
    NEXT_ROSTER_REFRESH.store(
        tick_ms.wrapping_add(ROSTER_REFRESH_INTERVAL_MS),
        Ordering::Relaxed,
    );
}

fn respond(id: u32, action: GroupInvitationAction) -> Result<(), CommandFailure> {
    let invitation = crate::group::invitation(id).ok_or(CommandFailure::InvalidTarget)?;
    let name = &invitation.inviter[..usize::from(invitation.inviter_len)];
    let pane = find(name).ok_or(CommandFailure::InvalidState)?;
    if action == GroupInvitationAction::Accept {
        let mut body = [0_u8; 32];
        body[0] = 0x2E;
        body[1] = 0x03;
        body[2] = invitation.inviter_len;
        body[3..3 + name.len()].copy_from_slice(name);
        network::submit(&body[..4 + name.len()])?;
    }
    let module_base = module_base()?;
    let address = module_base
        .checked_add(ALERT_ACTION_RVA)
        .ok_or(CommandFailure::Internal)?;
    // Action 2 is the alert's local dismissal path. Accept is submitted above
    // so the server request does not depend on the pane's callback internals.
    let action_id = 2;
    // SAFETY: exact RTTI, visibility, registration, requester identity, the
    // supported executable fingerprint, and the main-thread command boundary
    // establish a live GroupAlertPane and the validated native ABI.
    unsafe {
        let function: AlertActionFn = mem::transmute(address);
        function(pane as *mut c_void, action_id, 0);
    }
    let reason = match action {
        GroupInvitationAction::Accept => GroupInvitationCloseReason::AcceptRequested,
        GroupInvitationAction::Decline => GroupInvitationCloseReason::Declined,
    };
    crate::group::close_invitation(id, reason, now());
    Ok(())
}

fn find(name: &[u8]) -> Option<usize> {
    let mut found = None;
    for_each_open(|pane, current| {
        if found.is_none() && current.eq_ignore_ascii_case(name) {
            found = Some(pane);
        }
    });
    found
}

fn for_each_open(mut visit: impl FnMut(usize, &[u8])) {
    let Ok(module_base) = module_base() else {
        return;
    };
    let Some(dispatcher) = read::<u32>(module_base + EVENT_DISPATCHER_POINTER_RVA)
        .filter(|value| *value != 0)
        .map(|value| value as usize)
    else {
        return;
    };
    let Some(entries) = read::<u32>(dispatcher + ENTRIES_OFFSET).map(|value| value as usize) else {
        return;
    };
    let Some(count) = read::<i32>(dispatcher + COUNT_OFFSET) else {
        return;
    };
    let Some(capacity) = read::<i32>(dispatcher + CAPACITY_OFFSET) else {
        return;
    };
    if count < 0 || count > capacity || capacity > MAX_PANES || (count != 0 && entries == 0) {
        return;
    }
    for index in 0..count as usize {
        let Some(pane) = read::<u32>(entries + index * ENTRY_SIZE)
            .filter(|value| *value != 0)
            .map(|value| value as usize)
        else {
            continue;
        };
        let Some(vtable) = read::<u32>(pane).map(|value| value as usize) else {
            continue;
        };
        let Some(locator) = vtable.checked_sub(4).and_then(read::<u32>) else {
            continue;
        };
        if locator as usize != module_base + GROUP_ALERT_COL_RVA
            || read::<u8>(pane + VISIBLE_OFFSET) != Some(1)
            || read::<u8>(pane + REGISTRATION_OFFSET).unwrap_or(0) & REGISTERED == 0
        {
            continue;
        }
        let Some(length) = read::<u32>(pane + REQUESTER_LENGTH_OFFSET)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|length| (1..=28).contains(length))
        else {
            continue;
        };
        let Some(requester) = read::<u32>(pane + REQUESTER_OFFSET)
            .filter(|value| *value != 0)
            .map(|value| value as usize)
        else {
            continue;
        };
        let mut name = [0_u8; 28];
        let mut readable = true;
        for (offset, byte) in name.iter_mut().take(length).enumerate() {
            let Some(value) = read::<u8>(requester + offset) else {
                readable = false;
                break;
            };
            *byte = value;
        }
        if readable {
            visit(pane, &name[..length]);
        }
    }
}

fn now() -> u32 {
    darpc_win32::pipe::sender_tick_ms()
}

#[cfg(test)]
mod tests {
    use super::{ALERT_ACTION_RVA, GROUP_ALERT_COL_RVA, OpenAlerts, alert_scan_due};

    #[test]
    fn group_alert_contract_is_stable() {
        assert_eq!(GROUP_ALERT_COL_RVA, 0x002A_81C4);
        assert_eq!(ALERT_ACTION_RVA, 0x0004_8770);
    }

    #[test]
    fn open_alert_names_are_bounded_and_case_insensitive() {
        let mut open = OpenAlerts::new();
        open.push(b"ZiLo");
        assert!(open.contains(b"zilo"));
        assert!(!open.contains(b"Eidolon"));
    }

    #[test]
    fn alert_scan_interval_handles_tick_wrapping() {
        assert!(alert_scan_due(0, 1));
        assert!(!alert_scan_due(1_000, 1_099));
        assert!(alert_scan_due(1_000, 1_100));
        assert!(alert_scan_due(u32::MAX - 50, 49));
    }
}
