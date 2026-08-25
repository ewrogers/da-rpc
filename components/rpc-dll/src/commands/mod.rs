mod queue;
mod storage;

#[cfg(test)]
use darpc_model::LookResultTarget;
use darpc_model::{Direction, EquipmentSlot, LookTarget, MessageKind};
use darpc_protocol::{
    ChantText, CharacterStat, CommandFailure, CommandKind, CommandOperation, CommandResult,
    CommandState, CommandStatus, DialogAction, DialogCommand, DialogText, ExactRouteInvalidState,
    ExactRouteInvalidStateReason, ExchangeCommand, FieldMapSelectionCommand, GoldTransfer,
    GroupCommand, GroupInvitationAction, GroupText, ItemSlot, ItemTransfer,
    MAX_MESSAGE_CONTENT_LEN, MAX_MESSAGE_RECIPIENT_LEN, MAX_WALK_ROUTE_TILES, MessageCommand,
    MessageContent, MessageDialogCommand, MessageRecipient, RawPacket, RawPacketDirection,
    RouteTile, SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget,
    TilePosition, TransferTarget, WalkRoute, WalkTarget,
};
use std::{
    num::NonZeroU32,
    panic,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(all(windows, not(test)))]
use darpc_win32::pipe::sender_tick_ms;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

use queue::{CommandQueue, CommandSlot};
use storage::{StoredInput, kind_from_value, stored_kind};

pub(crate) const COMMAND_CAPACITY: usize = 64;
pub(crate) const COMMANDS_PER_TICK: usize = 1;
const MAX_COMMAND_TILE_BYTES: usize = MAX_WALK_ROUTE_TILES * 4;

const TERMINAL_RETENTION_MS: u32 = 30_000;
const RESPONSE_COALESCE_MS: u32 = 1_000;
const CAST_RESPONSE_WINDOW_MS: u32 = 500;
const INSUFFICIENT_MANA_MESSAGE: &[u8] = b"Your Will is too weak.";
const RESIST_MESSAGE: &[u8] = b"The magic has been deflected.";
const INVALID_TARGET_MESSAGE: &[u8] = b"No target.";
const NOT_ALLOWED_MESSAGE: &[u8] = b"That doesn't work here.";
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(1);

const EMPTY: u8 = 0;
const RESERVED: u8 = 1;
const ACCEPTED: u8 = 2;
const EXECUTING: u8 = 3;
const EXECUTED: u8 = 4;
const FAILED: u8 = 5;
const CANCELLED: u8 = 6;
const TIMED_OUT: u8 = 7;

static NEXT_COMMAND_ID: AtomicU32 = AtomicU32::new(1);
static SUBMITTING_RESYNC_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static PENDING_CAST_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static PENDING_CAST_DEADLINE_TICK_MS: AtomicU32 = AtomicU32::new(0);
static SLOTS: [CommandSlot; COMMAND_CAPACITY] = [const { CommandSlot::new() }; COMMAND_CAPACITY];
static QUEUE: CommandQueue = CommandQueue::new();

pub(crate) fn reset() {
    #[cfg(windows)]
    crate::who::reset();
    crate::state::refresh::reset();
    crate::look::reset();
    QUEUE.reset();
    NEXT_COMMAND_ID.store(1, Ordering::Relaxed);
    SUBMITTING_RESYNC_COMMAND_ID.store(0, Ordering::Relaxed);
    PENDING_CAST_COMMAND_ID.store(0, Ordering::Relaxed);
    PENDING_CAST_DEADLINE_TICK_MS.store(0, Ordering::Relaxed);
    for slot in &SLOTS {
        slot.clear();
    }
}

pub(crate) fn outgoing_resync_id() -> u32 {
    let command_id = SUBMITTING_RESYNC_COMMAND_ID.swap(0, Ordering::AcqRel);
    if command_id == 0 {
        next_command_id()
    } else {
        command_id
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) fn begin_resync_submission(command_id: u32) {
    SUBMITTING_RESYNC_COMMAND_ID.store(command_id, Ordering::Release);
}

#[cfg(all(windows, not(test)))]
pub(crate) fn end_resync_submission(command_id: u32) {
    let _ = SUBMITTING_RESYNC_COMMAND_ID.compare_exchange(
        command_id,
        0,
        Ordering::AcqRel,
        Ordering::Relaxed,
    );
}

pub(crate) fn handle(operation: CommandOperation) -> CommandResult {
    match operation {
        CommandOperation::Submit {
            kind,
            timeout_ms,
            wait_ms,
        } => match submit(kind, timeout_ms) {
            Some(status) => wait_for(status.command_id, wait_ms),
            None => CommandResult::Busy,
        },
        CommandOperation::Query {
            command_id,
            wait_ms,
        } => wait_for(command_id, wait_ms),
        CommandOperation::Cancel { command_id } => cancel(command_id),
    }
}

pub(crate) fn observe_tick() {
    let tick_ms = now_tick_ms();
    #[cfg(all(windows, not(test)))]
    crate::state::refresh::observe_tick(tick_ms);
    complete_pending_cast_if_due(tick_ms);
    expire_pending_look_if_due(tick_ms);
    for _ in 0..COMMANDS_PER_TICK {
        let Some(slot_index) = QUEUE.pop() else {
            break;
        };
        execute(slot_index);
    }
}

pub(crate) fn cancel_pending() {
    PENDING_CAST_COMMAND_ID.store(0, Ordering::Release);
    let now = now_tick_ms();
    for slot in &SLOTS {
        slot.completed_tick_ms.store(now, Ordering::Relaxed);
        slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
        let cancelled = cancel_state(slot, ACCEPTED) || cancel_state(slot, EXECUTING);
        if cancelled && matches!(slot.kind(), CommandKind::Who) {
            cancel_who(slot.command_id.load(Ordering::Relaxed));
        }
        if cancelled && matches!(slot.kind(), CommandKind::Look(_)) {
            crate::look::cancel(slot.command_id.load(Ordering::Relaxed));
        }
    }
}

fn submit(kind: CommandKind, timeout_ms: u16) -> Option<CommandStatus> {
    let now = now_tick_ms();
    if matches!(
        kind,
        CommandKind::Who | CommandKind::Legend | CommandKind::InspectPlayer(_)
    ) && let Some(status) = coalesced_response(kind, now)
    {
        return Some(status);
    }
    reclaim_terminal_slots(now);
    let (slot_index, slot) = claim_slot(now)?;
    let command_id = next_command_id();
    slot.initialize(command_id, kind, now, timeout_ms);
    if !QUEUE.push(slot_index) {
        slot.clear();
        return None;
    }
    slot.status(command_id)
}

fn claim_slot(now: u32) -> Option<(usize, &'static CommandSlot)> {
    if let Some(slot) = claim_matching_slot(|state, queued| state == EMPTY && !queued) {
        return Some(slot);
    }

    let oldest = SLOTS
        .iter()
        .enumerate()
        .filter(|(_, slot)| {
            is_terminal_value(slot.state.load(Ordering::Acquire))
                && !slot.queued.load(Ordering::Acquire)
        })
        .max_by_key(|(_, slot)| now.wrapping_sub(slot.completed_tick_ms.load(Ordering::Relaxed)))
        .map(|(index, _)| index)?;
    let slot = &SLOTS[oldest];
    let state = slot.state.load(Ordering::Acquire);
    if is_terminal_value(state)
        && slot
            .state
            .compare_exchange(state, RESERVED, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        Some((oldest, slot))
    } else {
        claim_matching_slot(|state, queued| state == EMPTY && !queued)
    }
}

fn claim_matching_slot(
    mut predicate: impl FnMut(u8, bool) -> bool,
) -> Option<(usize, &'static CommandSlot)> {
    SLOTS.iter().enumerate().find(|(_, slot)| {
        let state = slot.state.load(Ordering::Acquire);
        let queued = slot.queued.load(Ordering::Acquire);
        predicate(state, queued)
            && slot
                .state
                .compare_exchange(state, RESERVED, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
    })
}

fn wait_for(command_id: u32, wait_ms: u16) -> CommandResult {
    let deadline = Instant::now() + Duration::from_millis(u64::from(wait_ms));
    loop {
        let Some(slot) = find_slot(command_id) else {
            return CommandResult::NotFound;
        };
        expire_if_due(slot, now_tick_ms());
        let Some(status) = slot.status(command_id) else {
            return CommandResult::NotFound;
        };
        if status.state == CommandState::Executed && matches!(status.kind, CommandKind::Who) {
            return who_result(command_id).map_or(CommandResult::Status(status), |list| {
                CommandResult::Who { status, list }
            });
        }
        if status.state == CommandState::Executed && matches!(status.kind, CommandKind::Legend) {
            return CommandResult::Legend {
                status,
                marks: crate::legend::current(),
            };
        }
        if status.state == CommandState::Executed
            && let CommandKind::InspectPlayer(id) = status.kind
            && let Some(player) = crate::player::inspected_player(id.get())
        {
            return CommandResult::Player {
                status,
                player: Box::new(player),
            };
        }
        if status.state == CommandState::Failed
            && let Some(diagnostics) = slot.exact_route_diagnostic()
        {
            return CommandResult::ExactRouteInvalidState {
                status,
                diagnostics,
            };
        }
        if status.state.is_terminal() || wait_ms == 0 || Instant::now() >= deadline {
            return CommandResult::Status(status);
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
}

fn cancel(command_id: u32) -> CommandResult {
    let Some(slot) = find_slot(command_id) else {
        return CommandResult::NotFound;
    };
    let now = now_tick_ms();
    slot.completed_tick_ms.store(now, Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    let cancelled = cancel_state(slot, ACCEPTED) || cancel_state(slot, EXECUTING);
    if cancelled {
        clear_pending_cast(command_id);
    }
    if cancelled && matches!(slot.kind(), CommandKind::Who) {
        cancel_who(command_id);
    }
    if cancelled && matches!(slot.kind(), CommandKind::Look(_)) {
        crate::look::cancel(command_id);
    }
    slot.status(command_id)
        .map_or(CommandResult::NotFound, CommandResult::Status)
}

fn execute(slot_index: usize) {
    let Some(slot) = SLOTS.get(slot_index) else {
        return;
    };
    let now = now_tick_ms();
    expire_if_due(slot, now);
    if slot
        .state
        .compare_exchange(ACCEPTED, EXECUTING, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        slot.queued.store(false, Ordering::Release);
        return;
    }

    slot.started_tick_ms.store(now, Ordering::Relaxed);
    slot.has_started_tick_ms.store(true, Ordering::Relaxed);
    slot.main_thread_id
        .store(current_thread_id(), Ordering::Relaxed);
    slot.has_main_thread_id.store(true, Ordering::Relaxed);
    let started = Instant::now();
    let kind = slot.kind();
    let is_exact_route = matches!(kind, CommandKind::Walk(WalkTarget::Route(_)));
    let is_who = matches!(kind, CommandKind::Who);
    let is_cast = matches!(kind, CommandKind::CastSpell(_));
    let command_id = slot.command_id.load(Ordering::Relaxed);
    let waits_for_response = matches!(
        kind,
        CommandKind::Who
            | CommandKind::Legend
            | CommandKind::InspectPlayer(_)
            | CommandKind::Look(_)
    );
    if is_exact_route {
        crate::diagnostics::clear_invalid_exact_route_state();
    }
    let result = panic::catch_unwind(|| {
        if is_who {
            execute_who(command_id)
        } else if let CommandKind::InspectPlayer(id) = kind {
            execute_player(command_id, id.get())
        } else if let CommandKind::Look(target) = kind {
            execute_look(command_id, target)
        } else if matches!(kind, CommandKind::Resync) {
            execute_resync(command_id)
        } else {
            execute_command(kind)
        }
    })
    .unwrap_or(Err(CommandFailure::Internal));
    if is_exact_route
        && result == Err(CommandFailure::InvalidState)
        && let Some(diagnostics) = crate::diagnostics::take_invalid_exact_route_state()
    {
        slot.store_exact_route_diagnostic(diagnostics);
    }
    let execution_us = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
    slot.execution_us.store(execution_us, Ordering::Relaxed);
    slot.has_execution_us.store(true, Ordering::Relaxed);
    if is_cast && result.is_ok() {
        begin_pending_cast(slot, now_tick_ms());
    } else if waits_for_response && result.is_ok() {
        // The matching server response completes this command.
        if slot.state.load(Ordering::Acquire) != EXECUTING && matches!(kind, CommandKind::Look(_)) {
            crate::look::cancel(command_id);
        }
    } else {
        if is_who {
            cancel_who(slot.command_id.load(Ordering::Relaxed));
        }
        slot.completed_tick_ms
            .store(now_tick_ms(), Ordering::Relaxed);
        slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
        complete_execution(slot, result);
    }
    slot.queued.store(false, Ordering::Release);
}

#[cfg(all(windows, not(test)))]
fn execute_player(command_id: u32, id: u32) -> Result<(), CommandFailure> {
    crate::player::request(command_id, id)
}

#[cfg(all(windows, not(test)))]
fn execute_look(command_id: u32, target: LookTarget) -> Result<(), CommandFailure> {
    crate::look::request(command_id, target)
}

#[cfg(test)]
fn execute_look(command_id: u32, target: LookTarget) -> Result<(), CommandFailure> {
    let target = match target {
        LookTarget::Ahead => LookResultTarget::Ahead { x: 0, y: 0 },
        LookTarget::Tile { x, y } => LookResultTarget::Tile { x, y },
    };
    crate::look::begin(command_id, target)
}

#[cfg(test)]
const fn execute_player(_command_id: u32, _id: u32) -> Result<(), CommandFailure> {
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn execute_resync(command_id: u32) -> Result<(), CommandFailure> {
    crate::state::refresh::request_command(command_id)
}

#[cfg(test)]
const fn execute_resync(_command_id: u32) -> Result<(), CommandFailure> {
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn execute_command(kind: CommandKind) -> Result<(), CommandFailure> {
    crate::actions::execute(kind)
}

#[cfg(test)]
const fn execute_command(_kind: CommandKind) -> Result<(), CommandFailure> {
    Ok(())
}

fn complete_execution(slot: &CommandSlot, result: Result<(), CommandFailure>) {
    match result {
        Ok(()) => slot.state.store(EXECUTED, Ordering::Release),
        Err(failure) => {
            slot.failure
                .store(failure_value(failure), Ordering::Relaxed);
            slot.state.store(FAILED, Ordering::Release);
        }
    }
}

fn begin_pending_cast(slot: &CommandSlot, now: u32) {
    let command_id = slot.command_id.load(Ordering::Relaxed);
    PENDING_CAST_DEADLINE_TICK_MS
        .store(now.wrapping_add(CAST_RESPONSE_WINDOW_MS), Ordering::Relaxed);
    let previous = PENDING_CAST_COMMAND_ID.swap(command_id, Ordering::AcqRel);
    if previous != 0 && previous != command_id {
        complete_pending_cast_with(previous, Ok(()));
    }
}

fn complete_pending_cast_if_due(now: u32) {
    let command_id = PENDING_CAST_COMMAND_ID.load(Ordering::Acquire);
    if command_id == 0
        || !crate::wrapping_time::deadline_reached(
            now,
            PENDING_CAST_DEADLINE_TICK_MS.load(Ordering::Relaxed),
        )
        || PENDING_CAST_COMMAND_ID
            .compare_exchange(command_id, 0, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    complete_pending_cast_with(command_id, Ok(()));
}

pub(crate) fn observe_message(kind: MessageKind, text: &[u8]) {
    if kind != MessageKind::System {
        return;
    }
    let failure = match text {
        INSUFFICIENT_MANA_MESSAGE => CommandFailure::InsufficientMana,
        RESIST_MESSAGE => CommandFailure::Resist,
        INVALID_TARGET_MESSAGE => CommandFailure::InvalidTarget,
        NOT_ALLOWED_MESSAGE => CommandFailure::NotAllowed,
        _ => return,
    };
    let command_id = PENDING_CAST_COMMAND_ID.swap(0, Ordering::AcqRel);
    if command_id != 0 {
        complete_pending_cast_with(command_id, Err(failure));
    }
}

fn complete_pending_cast_with(command_id: u32, result: Result<(), CommandFailure>) {
    let Some(slot) = find_slot(command_id) else {
        return;
    };
    if slot.state.load(Ordering::Acquire) != EXECUTING
        || !matches!(slot.kind(), CommandKind::CastSpell(_))
    {
        return;
    }
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, result);
}

fn clear_pending_cast(command_id: u32) {
    let _ = PENDING_CAST_COMMAND_ID.compare_exchange(
        command_id,
        0,
        Ordering::AcqRel,
        Ordering::Relaxed,
    );
}

#[cfg(all(windows, not(test)))]
fn execute_who(command_id: u32) -> Result<(), CommandFailure> {
    crate::who::request(command_id)
}

#[cfg(test)]
const fn execute_who(_command_id: u32) -> Result<(), CommandFailure> {
    Ok(())
}

#[cfg(windows)]
fn who_result(command_id: u32) -> Option<darpc_model::WhoList> {
    crate::who::result(command_id)
}

#[cfg(not(windows))]
const fn who_result(_command_id: u32) -> Option<darpc_model::WhoList> {
    None
}

fn expire_if_due(slot: &CommandSlot, now: u32) {
    if !crate::wrapping_time::deadline_reached(now, slot.deadline_tick_ms.load(Ordering::Relaxed)) {
        return;
    }
    slot.completed_tick_ms.store(now, Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    let expired = expire_state(slot, ACCEPTED) || expire_state(slot, EXECUTING);
    if expired {
        clear_pending_cast(slot.command_id.load(Ordering::Relaxed));
    }
    if expired && matches!(slot.kind(), CommandKind::Who) {
        cancel_who(slot.command_id.load(Ordering::Relaxed));
    }
    if expired && matches!(slot.kind(), CommandKind::Look(_)) {
        crate::look::cancel(slot.command_id.load(Ordering::Relaxed));
    }
}

fn expire_pending_look_if_due(now: u32) {
    let command_id = crate::look::INTERCEPT_COMMAND_ID.load(Ordering::Acquire);
    if command_id == 0 {
        return;
    }
    let Some(slot) = find_slot(command_id) else {
        crate::look::cancel(command_id);
        return;
    };
    expire_if_due(slot, now);
}

#[cfg(windows)]
pub(crate) fn complete_who(command_id: u32) {
    complete_who_with(command_id, Ok(()));
}

#[cfg(windows)]
pub(crate) fn fail_who(command_id: u32) {
    complete_who_with(command_id, Err(CommandFailure::Internal));
}

#[cfg(windows)]
fn complete_who_with(command_id: u32, result: Result<(), CommandFailure>) {
    let Some(slot) = find_slot(command_id) else {
        return;
    };
    if slot.state.load(Ordering::Acquire) != EXECUTING {
        return;
    }
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, result);
}

#[cfg(windows)]
pub(crate) fn complete_player(command_id: u32) {
    complete_player_with(command_id, Ok(()));
}

#[cfg(windows)]
pub(crate) fn fail_player(command_id: u32) {
    complete_player_with(command_id, Err(CommandFailure::Internal));
}

#[cfg(any(windows, test))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn complete_look(command_id: u32, result: Result<(), CommandFailure>) {
    let Some(slot) = find_slot(command_id) else {
        return;
    };
    if slot.state.load(Ordering::Acquire) != EXECUTING
        || !matches!(slot.kind(), CommandKind::Look(_))
    {
        return;
    }
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, result);
}

#[cfg(not(windows))]
pub(crate) const fn complete_player(_command_id: u32) {}

#[cfg(not(windows))]
pub(crate) const fn fail_player(_command_id: u32) {}

#[cfg(windows)]
fn complete_player_with(command_id: u32, result: Result<(), CommandFailure>) {
    let Some(slot) = find_slot(command_id) else {
        return;
    };
    if slot.state.load(Ordering::Acquire) != EXECUTING {
        return;
    }
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, result);
}

fn coalesced_response(kind: CommandKind, now: u32) -> Option<CommandStatus> {
    SLOTS.iter().find_map(|slot| {
        let status = slot.status(slot.command_id.load(Ordering::Acquire))?;
        if status.kind != kind {
            return None;
        }
        if status.state == CommandState::Accepted {
            return Some(status);
        }
        if status.state == CommandState::Executed
            && status
                .completed_tick_ms
                .is_some_and(|completed| now.wrapping_sub(completed) <= RESPONSE_COALESCE_MS)
        {
            return Some(status);
        }
        None
    })
}

pub(crate) fn complete_legend() {
    let Some(slot) = SLOTS.iter().find(|slot| {
        slot.state.load(Ordering::Acquire) == EXECUTING
            && matches!(slot.kind(), CommandKind::Legend)
    }) else {
        return;
    };
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, Ok(()));
}

fn cancel_state(slot: &CommandSlot, state: u8) -> bool {
    slot.state
        .compare_exchange(state, CANCELLED, Ordering::Release, Ordering::Relaxed)
        .is_ok()
}

fn expire_state(slot: &CommandSlot, state: u8) -> bool {
    slot.state
        .compare_exchange(state, TIMED_OUT, Ordering::Release, Ordering::Relaxed)
        .is_ok()
}

#[cfg(windows)]
fn cancel_who(command_id: u32) {
    crate::who::cancel(command_id);
}

#[cfg(not(windows))]
const fn cancel_who(_command_id: u32) {}

fn reclaim_terminal_slots(now: u32) {
    for slot in &SLOTS {
        let state = slot.state.load(Ordering::Acquire);
        if !is_terminal_value(state) || slot.queued.load(Ordering::Acquire) {
            continue;
        }
        let completed = slot.completed_tick_ms.load(Ordering::Relaxed);
        if now.wrapping_sub(completed) < TERMINAL_RETENTION_MS {
            continue;
        }
        if slot
            .state
            .compare_exchange(state, RESERVED, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            slot.clear();
        }
    }
}

fn find_slot(command_id: u32) -> Option<&'static CommandSlot> {
    SLOTS
        .iter()
        .find(|slot| slot.command_id.load(Ordering::Acquire) == command_id)
}

fn next_command_id() -> u32 {
    loop {
        let command_id = NEXT_COMMAND_ID.fetch_add(1, Ordering::Relaxed);
        if command_id != 0 && find_slot(command_id).is_none() {
            return command_id;
        }
    }
}

const fn public_state(value: u8) -> CommandState {
    match value {
        ACCEPTED | EXECUTING => CommandState::Accepted,
        EXECUTED => CommandState::Executed,
        FAILED => CommandState::Failed,
        CANCELLED => CommandState::Cancelled,
        TIMED_OUT => CommandState::TimedOut,
        _ => CommandState::Failed,
    }
}

const fn is_terminal_value(value: u8) -> bool {
    matches!(value, EXECUTED | FAILED | CANCELLED | TIMED_OUT)
}

fn optional_atomic(value: &AtomicU32, present: &AtomicBool) -> Option<u32> {
    present
        .load(Ordering::Relaxed)
        .then(|| value.load(Ordering::Relaxed))
}

const fn failure_from_value(value: u8) -> Option<CommandFailure> {
    match value {
        0 => None,
        1 => Some(CommandFailure::Internal),
        2 => Some(CommandFailure::InvalidState),
        3 => Some(CommandFailure::InvalidDestination),
        4 => Some(CommandFailure::Rejected),
        5 => Some(CommandFailure::NoPath),
        6 => Some(CommandFailure::InvalidSkill),
        7 => Some(CommandFailure::InvalidSpell),
        8 => Some(CommandFailure::InvalidArguments),
        9 => Some(CommandFailure::InvalidTarget),
        10 => Some(CommandFailure::InsufficientMana),
        11 => Some(CommandFailure::Resist),
        12 => Some(CommandFailure::NotAllowed),
        _ => Some(CommandFailure::Internal),
    }
}

const fn failure_value(failure: CommandFailure) -> u8 {
    match failure {
        CommandFailure::Internal => 1,
        CommandFailure::InvalidState => 2,
        CommandFailure::InvalidDestination => 3,
        CommandFailure::Rejected => 4,
        CommandFailure::NoPath => 5,
        CommandFailure::InvalidSkill => 6,
        CommandFailure::InvalidSpell => 7,
        CommandFailure::InvalidArguments => 8,
        CommandFailure::InvalidTarget => 9,
        CommandFailure::InsufficientMana => 10,
        CommandFailure::Resist => 11,
        CommandFailure::NotAllowed => 12,
    }
}

#[cfg(all(windows, not(test)))]
fn now_tick_ms() -> u32 {
    sender_tick_ms()
}

#[cfg(test)]
fn now_tick_ms() -> u32 {
    TEST_TICK_MS.load(Ordering::Relaxed)
}

#[cfg(windows)]
fn current_thread_id() -> u32 {
    // SAFETY: GetCurrentThreadId has no preconditions.
    unsafe { GetCurrentThreadId() }
}

#[cfg(not(windows))]
const fn current_thread_id() -> u32 {
    7
}

#[cfg(test)]
static TEST_TICK_MS: AtomicU32 = AtomicU32::new(1);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn correlates_command_and_physical_resync_ids() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        SUBMITTING_RESYNC_COMMAND_ID.store(7, Ordering::Relaxed);

        assert_eq!(outgoing_resync_id(), 7);
        let physical_resync_id = outgoing_resync_id();
        assert_ne!(physical_resync_id, 0);
        assert_ne!(physical_resync_id, 7);
    }

    #[test]
    fn executes_one_diagnostic_per_tick() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let first = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: 1_000,
            wait_ms: 0,
        }));
        let second = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: 1_000,
            wait_ms: 0,
        }));

        let expected_thread_id = current_thread_id();
        observe_tick();
        assert_eq!(status(first).state, CommandState::Executed);
        assert_eq!(status(first).main_thread_id, Some(expected_thread_id));
        assert_eq!(status(second).state, CommandState::Accepted);
        observe_tick();
        assert_eq!(status(second).state, CommandState::Executed);
    }

    #[test]
    fn spell_result_messages_fail_the_pending_cast() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (message, expected) in [
            (INSUFFICIENT_MANA_MESSAGE, CommandFailure::InsufficientMana),
            (RESIST_MESSAGE, CommandFailure::Resist),
            (INVALID_TARGET_MESSAGE, CommandFailure::InvalidTarget),
            (NOT_ALLOWED_MESSAGE, CommandFailure::NotAllowed),
        ] {
            reset();
            TEST_TICK_MS.store(10, Ordering::Relaxed);
            let id = submitted_id(handle(CommandOperation::Submit {
                kind: CommandKind::CastSpell(SpellCast {
                    slot: SpellSlot::new(1).unwrap(),
                    arguments: SpellArguments::None,
                }),
                timeout_ms: 1_100,
                wait_ms: 0,
            }));

            observe_tick();
            assert_eq!(status(id).state, CommandState::Accepted);
            observe_message(MessageKind::System, message);

            let result = status(id);
            assert_eq!(result.state, CommandState::Failed);
            assert_eq!(result.failure, Some(expected));
        }
    }

    #[test]
    fn inexact_spell_result_message_does_not_complete_the_pending_cast() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let id = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::CastSpell(SpellCast {
                slot: SpellSlot::new(1).unwrap(),
                arguments: SpellArguments::None,
            }),
            timeout_ms: 1_100,
            wait_ms: 0,
        }));

        observe_tick();
        observe_message(MessageKind::System, b"No target");

        assert_eq!(status(id).state, CommandState::Accepted);
    }

    #[test]
    fn cast_succeeds_after_the_response_window() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let id = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::CastSpell(SpellCast {
                slot: SpellSlot::new(1).unwrap(),
                arguments: SpellArguments::None,
            }),
            timeout_ms: 1_100,
            wait_ms: 0,
        }));

        observe_tick();
        TEST_TICK_MS.store(509, Ordering::Relaxed);
        observe_tick();
        assert_eq!(status(id).state, CommandState::Accepted);
        TEST_TICK_MS.store(510, Ordering::Relaxed);
        observe_tick();
        assert_eq!(status(id).state, CommandState::Executed);
    }

    #[test]
    fn retains_typed_command_arguments_through_execution() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let commands = [
            CommandKind::Turn(Direction::West),
            CommandKind::Walk(WalkTarget::Direction(Direction::North)),
            CommandKind::Walk(WalkTarget::Destination { x: 120, y: 85 }),
            CommandKind::Walk(WalkTarget::Route(
                WalkRoute::new(
                    3001,
                    &[RouteTile { x: 11, y: 22 }, RouteTile { x: 12, y: 22 }],
                )
                .unwrap(),
            )),
            CommandKind::Walk(WalkTarget::Cancel),
            CommandKind::UseSkill(SkillSlot::new(7).unwrap()),
            CommandKind::Assail,
            CommandKind::Resync,
        ];

        for kind in commands {
            let id = submitted_id(handle(CommandOperation::Submit {
                kind,
                timeout_ms: 1_000,
                wait_ms: 0,
            }));
            assert_eq!(status(id).kind, kind);
            observe_tick();
            assert_eq!(status(id).kind, kind);
            assert_eq!(status(id).state, CommandState::Executed);
        }
    }

    #[test]
    fn chant_text_survives_bounded_queue_storage_verbatim() {
        let expected = CommandKind::Chant(ChantText::new("MiXeD, punctuation!  ").unwrap());
        let (value, argument_x, argument_y, argument_z, input) = stored_kind(expected);
        let input = input.as_ref().map_or(&[][..], StoredInput::as_bytes);
        assert_eq!(
            kind_from_value(value, argument_x, argument_y, argument_z, input),
            expected
        );
    }

    #[test]
    fn messages_survive_bounded_queue_storage_verbatim() {
        let content = MessageContent::new("MiXeD, punctuation!  ").unwrap();
        let recipient = MessageRecipient::new("Eidolon").unwrap();
        for message in [
            MessageCommand::Say(content),
            MessageCommand::Shout(content),
            MessageCommand::Whisper { recipient, content },
            MessageCommand::Guild(content),
            MessageCommand::Group(content),
        ] {
            let expected = CommandKind::Message(message);
            let (value, argument_x, argument_y, argument_z, input) = stored_kind(expected);
            let input = input.as_ref().map_or(&[][..], StoredInput::as_bytes);
            assert_eq!(
                kind_from_value(value, argument_x, argument_y, argument_z, input),
                expected
            );
        }
    }

    #[test]
    fn raw_packets_survive_bounded_queue_storage_verbatim() {
        for direction in [RawPacketDirection::Client, RawPacketDirection::Server] {
            let expected =
                CommandKind::Raw(RawPacket::new(direction, 0x7e, &[0x00, 0x03, 0x02]).unwrap());
            let (value, argument_x, argument_y, argument_z, input) = stored_kind(expected);
            let input = input.as_ref().map_or(&[][..], StoredInput::as_bytes);
            assert_eq!(
                kind_from_value(value, argument_x, argument_y, argument_z, input),
                expected
            );
        }
    }

    #[test]
    fn queue_full_is_busy_and_cancel_is_terminal() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let ids: Vec<_> = (0..COMMAND_CAPACITY)
            .map(|_| {
                submitted_id(handle(CommandOperation::Submit {
                    kind: CommandKind::Diagnostic,
                    timeout_ms: 1_000,
                    wait_ms: 0,
                }))
            })
            .collect();
        assert_eq!(
            handle(CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: 1_000,
                wait_ms: 0,
            }),
            CommandResult::Busy
        );
        assert_eq!(
            state(handle(CommandOperation::Cancel { command_id: ids[0] })),
            CommandState::Cancelled
        );
        observe_tick();
        assert_eq!(status(ids[0]).state, CommandState::Cancelled);
    }

    #[test]
    fn pending_commands_time_out_without_execution() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(u32::MAX - 5, Ordering::Relaxed);
        let id = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: 10,
            wait_ms: 0,
        }));
        TEST_TICK_MS.store(5, Ordering::Relaxed);
        assert_eq!(status(id).state, CommandState::TimedOut);
        observe_tick();
        assert_eq!(status(id).state, CommandState::TimedOut);
    }

    #[test]
    fn who_requests_coalesce_and_time_out_while_awaiting_the_server() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let first = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Who,
            timeout_ms: 3_000,
            wait_ms: 0,
        }));
        let second = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Who,
            timeout_ms: 3_000,
            wait_ms: 0,
        }));
        assert_eq!(first, second);
        observe_tick();
        assert_eq!(status(first).state, CommandState::Accepted);
        #[cfg(windows)]
        crate::who::INTERCEPT_COMMAND_ID.store(first, Ordering::Release);
        TEST_TICK_MS.store(3_010, Ordering::Relaxed);
        assert_eq!(status(first).state, CommandState::TimedOut);
        #[cfg(windows)]
        assert_eq!(crate::who::INTERCEPT_COMMAND_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn look_requests_expire_on_tick_without_a_follow_up_query() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let id = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Look(LookTarget::Tile { x: 40, y: 19 }),
            timeout_ms: 10,
            wait_ms: 0,
        }));
        observe_tick();
        assert_eq!(
            crate::look::INTERCEPT_COMMAND_ID.load(Ordering::Acquire),
            id
        );

        TEST_TICK_MS.store(21, Ordering::Relaxed);
        observe_tick();
        let status = find_slot(id).unwrap().status(id).unwrap();
        assert_eq!(status.state, CommandState::TimedOut);
        assert_eq!(crate::look::INTERCEPT_COMMAND_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn legend_requests_coalesce_for_one_second() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        crate::legend::reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let first = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Legend,
            timeout_ms: 3_000,
            wait_ms: 0,
        }));
        let pending = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Legend,
            timeout_ms: 3_000,
            wait_ms: 0,
        }));
        assert_eq!(pending, first);
        observe_tick();
        complete_legend();
        assert!(matches!(
            handle(CommandOperation::Query {
                command_id: first,
                wait_ms: 0,
            }),
            CommandResult::Legend { .. }
        ));

        TEST_TICK_MS.store(1_010, Ordering::Relaxed);
        let cached = match handle(CommandOperation::Submit {
            kind: CommandKind::Legend,
            timeout_ms: 3_000,
            wait_ms: 0,
        }) {
            CommandResult::Legend { status, .. } => status.command_id,
            result => panic!("expected cached legend, received {result:?}"),
        };
        assert_eq!(cached, first);

        TEST_TICK_MS.store(1_011, Ordering::Relaxed);
        let refreshed = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Legend,
            timeout_ms: 3_000,
            wait_ms: 0,
        }));
        assert_ne!(refreshed, first);
    }

    #[test]
    fn retained_terminal_results_do_not_consume_queue_capacity() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        for _ in 0..COMMAND_CAPACITY {
            let id = submitted_id(handle(CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: 1_000,
                wait_ms: 0,
            }));
            observe_tick();
            assert_eq!(status(id).state, CommandState::Executed);
        }
        assert!(matches!(
            handle(CommandOperation::Submit {
                kind: CommandKind::Diagnostic,
                timeout_ms: 1_000,
                wait_ms: 0,
            }),
            CommandResult::Status(_)
        ));
    }

    #[test]
    fn shutdown_cancels_pending_commands() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        TEST_TICK_MS.store(10, Ordering::Relaxed);
        let id = submitted_id(handle(CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: 1_000,
            wait_ms: 0,
        }));
        cancel_pending();
        assert_eq!(status(id).state, CommandState::Cancelled);
    }

    #[test]
    fn executor_failures_are_terminal_and_observable() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        let slot = &SLOTS[0];
        slot.state.store(EXECUTING, Ordering::Relaxed);
        slot.command_id.store(1, Ordering::Relaxed);
        slot.kind.store(0, Ordering::Relaxed);
        slot.enqueued_tick_ms.store(1, Ordering::Relaxed);
        slot.deadline_tick_ms.store(10, Ordering::Relaxed);
        slot.started_tick_ms.store(2, Ordering::Relaxed);
        slot.has_started_tick_ms.store(true, Ordering::Relaxed);
        slot.completed_tick_ms.store(3, Ordering::Relaxed);
        slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
        slot.execution_us.store(4, Ordering::Relaxed);
        slot.has_execution_us.store(true, Ordering::Relaxed);
        slot.main_thread_id.store(5, Ordering::Relaxed);
        slot.has_main_thread_id.store(true, Ordering::Relaxed);

        complete_execution(slot, Err(CommandFailure::Internal));

        let status = slot.status(1).unwrap();
        assert_eq!(status.state, CommandState::Failed);
        assert_eq!(status.failure, Some(CommandFailure::Internal));
    }

    fn submitted_id(result: CommandResult) -> u32 {
        match result {
            CommandResult::Status(status) => status.command_id,
            result => panic!("expected status, received {result:?}"),
        }
    }

    fn status(command_id: u32) -> CommandStatus {
        match handle(CommandOperation::Query {
            command_id,
            wait_ms: 0,
        }) {
            CommandResult::Status(status) => status,
            result => panic!("expected status, received {result:?}"),
        }
    }

    fn state(result: CommandResult) -> CommandState {
        match result {
            CommandResult::Status(status) => status.state,
            result => panic!("expected status, received {result:?}"),
        }
    }
}
