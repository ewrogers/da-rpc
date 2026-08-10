use darpc_model::{Direction, EquipmentSlot};
use darpc_protocol::{
    ChantText, CommandFailure, CommandKind, CommandOperation, CommandResult, CommandState,
    CommandStatus, DialogAction, DialogCommand, DialogText, ExchangeCommand, GoldTransfer,
    GroupCommand, GroupInvitationAction, GroupText, ItemSlot, ItemTransfer, MAX_DIALOG_INPUT_LEN,
    RawPacket, RawPacketDirection, SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput,
    SpellSlot, SpellTarget, TilePosition, TransferTarget, WalkTarget,
};
use std::{
    panic,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(all(windows, not(test)))]
use darpc_win32::pipe::sender_tick_ms;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

pub(crate) const COMMAND_CAPACITY: usize = 64;
pub(crate) const COMMANDS_PER_TICK: usize = 1;

const TERMINAL_RETENTION_MS: u32 = 30_000;
const RESPONSE_COALESCE_MS: u32 = 1_000;
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
static SLOTS: [CommandSlot; COMMAND_CAPACITY] = [const { CommandSlot::new() }; COMMAND_CAPACITY];
static QUEUE: CommandQueue = CommandQueue::new();

struct CommandSlot {
    state: AtomicU8,
    queued: AtomicBool,
    command_id: AtomicU32,
    kind: AtomicU8,
    argument_x: AtomicU32,
    argument_y: AtomicU32,
    argument_z: AtomicU32,
    argument_length: AtomicU8,
    argument_bytes: [AtomicU8; MAX_DIALOG_INPUT_LEN],
    enqueued_tick_ms: AtomicU32,
    deadline_tick_ms: AtomicU32,
    started_tick_ms: AtomicU32,
    has_started_tick_ms: AtomicBool,
    completed_tick_ms: AtomicU32,
    has_completed_tick_ms: AtomicBool,
    execution_us: AtomicU32,
    has_execution_us: AtomicBool,
    main_thread_id: AtomicU32,
    has_main_thread_id: AtomicBool,
    failure: AtomicU8,
}

impl CommandSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            queued: AtomicBool::new(false),
            command_id: AtomicU32::new(0),
            kind: AtomicU8::new(0),
            argument_x: AtomicU32::new(0),
            argument_y: AtomicU32::new(0),
            argument_z: AtomicU32::new(0),
            argument_length: AtomicU8::new(0),
            argument_bytes: [const { AtomicU8::new(0) }; MAX_DIALOG_INPUT_LEN],
            enqueued_tick_ms: AtomicU32::new(0),
            deadline_tick_ms: AtomicU32::new(0),
            started_tick_ms: AtomicU32::new(0),
            has_started_tick_ms: AtomicBool::new(false),
            completed_tick_ms: AtomicU32::new(0),
            has_completed_tick_ms: AtomicBool::new(false),
            execution_us: AtomicU32::new(0),
            has_execution_us: AtomicBool::new(false),
            main_thread_id: AtomicU32::new(0),
            has_main_thread_id: AtomicBool::new(false),
            failure: AtomicU8::new(0),
        }
    }

    fn initialize(&self, command_id: u32, kind: CommandKind, now: u32, timeout_ms: u16) {
        let (kind, argument_x, argument_y, argument_z, input) = stored_kind(kind);
        self.command_id.store(command_id, Ordering::Relaxed);
        self.kind.store(kind, Ordering::Relaxed);
        self.argument_x.store(argument_x, Ordering::Relaxed);
        self.argument_y.store(argument_y, Ordering::Relaxed);
        self.argument_z.store(argument_z, Ordering::Relaxed);
        let input = input.as_ref().map_or(&[][..], StoredInput::as_bytes);
        self.argument_length.store(
            u8::try_from(input.len()).expect("command input limit fits u8"),
            Ordering::Relaxed,
        );
        for (index, byte) in self.argument_bytes.iter().enumerate() {
            byte.store(
                input.get(index).copied().unwrap_or_default(),
                Ordering::Relaxed,
            );
        }
        self.enqueued_tick_ms.store(now, Ordering::Relaxed);
        self.deadline_tick_ms
            .store(now.wrapping_add(u32::from(timeout_ms)), Ordering::Relaxed);
        self.started_tick_ms.store(0, Ordering::Relaxed);
        self.has_started_tick_ms.store(false, Ordering::Relaxed);
        self.completed_tick_ms.store(0, Ordering::Relaxed);
        self.has_completed_tick_ms.store(false, Ordering::Relaxed);
        self.execution_us.store(0, Ordering::Relaxed);
        self.has_execution_us.store(false, Ordering::Relaxed);
        self.main_thread_id.store(0, Ordering::Relaxed);
        self.has_main_thread_id.store(false, Ordering::Relaxed);
        self.failure.store(0, Ordering::Relaxed);
        self.queued.store(true, Ordering::Relaxed);
        self.state.store(ACCEPTED, Ordering::Release);
    }

    fn status(&self, expected_id: u32) -> Option<CommandStatus> {
        loop {
            let state = self.state.load(Ordering::Acquire);
            if matches!(state, EMPTY | RESERVED)
                || self.command_id.load(Ordering::Relaxed) != expected_id
            {
                return None;
            }

            let status = CommandStatus {
                command_id: expected_id,
                kind: self.kind(),
                state: public_state(state),
                enqueued_tick_ms: self.enqueued_tick_ms.load(Ordering::Relaxed),
                deadline_tick_ms: self.deadline_tick_ms.load(Ordering::Relaxed),
                started_tick_ms: matches!(state, EXECUTING | EXECUTED | FAILED)
                    .then(|| optional_atomic(&self.started_tick_ms, &self.has_started_tick_ms))
                    .flatten(),
                completed_tick_ms: is_terminal_value(state)
                    .then(|| optional_atomic(&self.completed_tick_ms, &self.has_completed_tick_ms))
                    .flatten(),
                execution_us: matches!(state, EXECUTED | FAILED)
                    .then(|| optional_atomic(&self.execution_us, &self.has_execution_us))
                    .flatten(),
                main_thread_id: matches!(state, EXECUTING | EXECUTED | FAILED)
                    .then(|| optional_atomic(&self.main_thread_id, &self.has_main_thread_id))
                    .flatten(),
                failure: (state == FAILED)
                    .then(|| failure_from_value(self.failure.load(Ordering::Relaxed)))
                    .flatten(),
            };

            if self.state.load(Ordering::Acquire) == state
                && self.command_id.load(Ordering::Acquire) == expected_id
            {
                return Some(status);
            }
        }
    }

    fn clear(&self) {
        self.queued.store(false, Ordering::Relaxed);
        self.command_id.store(0, Ordering::Relaxed);
        self.state.store(EMPTY, Ordering::Release);
    }

    fn kind(&self) -> CommandKind {
        let length = usize::from(self.argument_length.load(Ordering::Relaxed));
        let mut input = [0; MAX_DIALOG_INPUT_LEN];
        for (destination, source) in input.iter_mut().zip(&self.argument_bytes).take(length) {
            *destination = source.load(Ordering::Relaxed);
        }
        kind_from_value(
            self.kind.load(Ordering::Relaxed),
            self.argument_x.load(Ordering::Relaxed),
            self.argument_y.load(Ordering::Relaxed),
            self.argument_z.load(Ordering::Relaxed),
            &input[..length.min(MAX_DIALOG_INPUT_LEN)],
        )
    }
}

struct CommandQueue {
    entries: [AtomicUsize; COMMAND_CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl CommandQueue {
    const fn new() -> Self {
        Self {
            entries: [const { AtomicUsize::new(0) }; COMMAND_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    fn push(&self, slot_index: usize) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= COMMAND_CAPACITY {
            return false;
        }
        self.entries[tail % COMMAND_CAPACITY].store(slot_index, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    fn pop(&self) -> Option<usize> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let slot_index = self.entries[head % COMMAND_CAPACITY].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(slot_index)
    }

    fn reset(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        for entry in &self.entries {
            entry.store(0, Ordering::Relaxed);
        }
    }
}

pub(crate) fn reset() {
    #[cfg(windows)]
    crate::who::reset();
    QUEUE.reset();
    NEXT_COMMAND_ID.store(1, Ordering::Relaxed);
    for slot in &SLOTS {
        slot.clear();
    }
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
    for _ in 0..COMMANDS_PER_TICK {
        let Some(slot_index) = QUEUE.pop() else {
            break;
        };
        execute(slot_index);
    }
}

pub(crate) fn cancel_pending() {
    let now = now_tick_ms();
    for slot in &SLOTS {
        slot.completed_tick_ms.store(now, Ordering::Relaxed);
        slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
        let cancelled = cancel_state(slot, ACCEPTED) || cancel_state(slot, EXECUTING);
        if cancelled && matches!(slot.kind(), CommandKind::Who) {
            cancel_who(slot.command_id.load(Ordering::Relaxed));
        }
    }
}

fn submit(kind: CommandKind, timeout_ms: u16) -> Option<CommandStatus> {
    let now = now_tick_ms();
    if matches!(kind, CommandKind::Who | CommandKind::Legend)
        && let Some(status) = coalesced_response(kind, now)
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
    if cancelled && matches!(slot.kind(), CommandKind::Who) {
        cancel_who(command_id);
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
    let is_who = matches!(kind, CommandKind::Who);
    let waits_for_response = matches!(kind, CommandKind::Who | CommandKind::Legend);
    let result = panic::catch_unwind(|| {
        if is_who {
            execute_who(slot.command_id.load(Ordering::Relaxed))
        } else {
            execute_command(kind)
        }
    })
    .unwrap_or(Err(CommandFailure::Internal));
    let execution_us = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
    slot.execution_us.store(execution_us, Ordering::Relaxed);
    slot.has_execution_us.store(true, Ordering::Relaxed);
    if waits_for_response && result.is_ok() {
        // The matching server response completes this command.
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
    if !has_reached(now, slot.deadline_tick_ms.load(Ordering::Relaxed)) {
        return;
    }
    slot.completed_tick_ms.store(now, Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    let expired = expire_state(slot, ACCEPTED) || expire_state(slot, EXECUTING);
    if expired && matches!(slot.kind(), CommandKind::Who) {
        cancel_who(slot.command_id.load(Ordering::Relaxed));
    }
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

#[derive(Clone, Copy)]
enum StoredInput {
    Spell(SpellInput),
    Dialog(DialogText),
    Group(GroupText),
    Chant(ChantText),
    Raw(RawPacket),
}

impl StoredInput {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Spell(value) => value.as_bytes(),
            Self::Dialog(value) => value.as_bytes(),
            Self::Group(value) => value.as_bytes(),
            Self::Chant(value) => value.as_bytes(),
            Self::Raw(value) => value.payload(),
        }
    }
}

fn stored_kind(kind: CommandKind) -> (u8, u32, u32, u32, Option<StoredInput>) {
    match kind {
        CommandKind::Diagnostic => (0, 0, 0, 0, None),
        CommandKind::Turn(direction) => (1, direction.raw() as u32, 0, 0, None),
        CommandKind::Walk(WalkTarget::Direction(direction)) => {
            (2, direction.raw() as u32, 0, 0, None)
        }
        CommandKind::Walk(WalkTarget::Destination { x, y }) => (3, x as u32, y as u32, 0, None),
        CommandKind::UseSkill(slot) => (4, slot.get() as u32, 0, 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::None,
        }) => (5, slot.get() as u32, 0, 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Target(SpellTarget::Object(id)),
        }) => (6, slot.get() as u32, id.get(), 0, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Target(SpellTarget::Tile { x, y }),
        }) => (7, slot.get() as u32, x as u32, y as u32, None),
        CommandKind::CastSpell(SpellCast {
            slot,
            arguments: SpellArguments::Input(input),
        }) => (8, slot.get() as u32, 0, 0, Some(StoredInput::Spell(input))),
        CommandKind::UseItem(slot) => (9, slot.get() as u32, 0, 0, None),
        CommandKind::DropItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Tile(position),
        }) => (10, slot.get() as u32, quantity, pack_tile(position), None),
        CommandKind::DropItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Object(id),
        }) => (11, slot.get() as u32, quantity, id.get(), None),
        CommandKind::DropGold(GoldTransfer {
            amount,
            target: TransferTarget::Tile(position),
        }) => (12, amount, pack_tile(position), 0, None),
        CommandKind::DropGold(GoldTransfer {
            amount,
            target: TransferTarget::Object(id),
        }) => (13, amount, id.get(), 0, None),
        CommandKind::PickupItem(position) => (14, position.x as u32, position.y as u32, 0, None),
        CommandKind::Unequip(slot) => (15, slot.raw() as u32, 0, 0, None),
        CommandKind::Emote(code) => (16, code as u32, 0, 0, None),
        CommandKind::GiveItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Tile(position),
        }) => (17, slot.get() as u32, quantity, pack_tile(position), None),
        CommandKind::GiveItem(ItemTransfer {
            slot,
            quantity,
            target: TransferTarget::Object(id),
        }) => (18, slot.get() as u32, quantity, id.get(), None),
        CommandKind::GiveGold(GoldTransfer {
            amount,
            target: TransferTarget::Tile(position),
        }) => (19, amount, pack_tile(position), 0, None),
        CommandKind::GiveGold(GoldTransfer {
            amount,
            target: TransferTarget::Object(id),
        }) => (20, amount, id.get(), 0, None),
        CommandKind::SwapSlots(SlotSwap::Inventory {
            source,
            destination,
        }) => (21, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::SwapSlots(SlotSwap::Spellbook {
            source,
            destination,
        }) => (22, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::SwapSlots(SlotSwap::Skillbook {
            source,
            destination,
        }) => (23, source.get() as u32, destination.get() as u32, 0, None),
        CommandKind::Interact(id) => (24, id.get(), 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Select { index, quantity },
        }) => (25, revision, u32::from(index), u32::from(quantity), None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Input(input),
        }) => (26, revision, 0, 0, Some(StoredInput::Dialog(input))),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Previous,
        }) => (27, revision, 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Next,
        }) => (28, revision, 0, 0, None),
        CommandKind::Dialog(DialogCommand {
            revision,
            action: DialogAction::Close,
        }) => (29, revision, 0, 0, None),
        CommandKind::Group(GroupCommand::Toggle) => (33, 0, 0, 0, None),
        CommandKind::Group(GroupCommand::Invite(target)) => {
            (30, 0, 0, 0, Some(StoredInput::Group(target)))
        }
        CommandKind::Group(GroupCommand::Respond {
            invitation_id,
            action: GroupInvitationAction::Accept,
        }) => (31, invitation_id, 0, 0, None),
        CommandKind::Group(GroupCommand::Respond {
            invitation_id,
            action: GroupInvitationAction::Decline,
        }) => (32, invitation_id, 0, 0, None),
        CommandKind::Who => (34, 0, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::AddItem { slot, quantity }) => {
            (35, u32::from(slot.get()), u32::from(quantity), 0, None)
        }
        CommandKind::Exchange(ExchangeCommand::SetGold(amount)) => (36, amount, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::Accept) => (37, 0, 0, 0, None),
        CommandKind::Exchange(ExchangeCommand::Cancel) => (38, 0, 0, 0, None),
        CommandKind::Chant(text) => (39, 0, 0, 0, Some(StoredInput::Chant(text))),
        CommandKind::Legend => (40, 0, 0, 0, None),
        CommandKind::Raw(packet) => (
            41,
            match packet.direction() {
                RawPacketDirection::Client => 0,
                RawPacketDirection::Server => 1,
            },
            u32::from(packet.command()),
            0,
            Some(StoredInput::Raw(packet)),
        ),
        CommandKind::Assail => (42, 0, 0, 0, None),
    }
}

fn kind_from_value(
    value: u8,
    argument_x: u32,
    argument_y: u32,
    argument_z: u32,
    input: &[u8],
) -> CommandKind {
    match value {
        1 => CommandKind::Turn(stored_direction(argument_x)),
        2 => CommandKind::Walk(WalkTarget::Direction(stored_direction(argument_x))),
        3 => CommandKind::Walk(WalkTarget::Destination {
            x: argument_x as i32,
            y: argument_y as i32,
        }),
        4 => match SkillSlot::new(argument_x as u8) {
            Some(slot) => CommandKind::UseSkill(slot),
            None => CommandKind::Diagnostic,
        },
        5..=8 => {
            let Some(slot) = SpellSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let arguments = match value {
                5 => SpellArguments::None,
                6 => match std::num::NonZeroU32::new(argument_y) {
                    Some(id) => SpellArguments::Target(SpellTarget::Object(id)),
                    None => return CommandKind::Diagnostic,
                },
                7 => SpellArguments::Target(SpellTarget::Tile {
                    x: argument_y as i32,
                    y: argument_z as i32,
                }),
                8 => {
                    let Ok(input) = std::str::from_utf8(input) else {
                        return CommandKind::Diagnostic;
                    };
                    let Some(input) = SpellInput::new(input) else {
                        return CommandKind::Diagnostic;
                    };
                    SpellArguments::Input(input)
                }
                _ => unreachable!(),
            };
            CommandKind::CastSpell(SpellCast { slot, arguments })
        }
        9 => ItemSlot::new(argument_x as u8)
            .map(CommandKind::UseItem)
            .unwrap_or(CommandKind::Diagnostic),
        10 | 11 => {
            let Some(slot) = ItemSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let target = if value == 10 {
                TransferTarget::Tile(unpack_tile(argument_z))
            } else {
                let Some(id) = std::num::NonZeroU32::new(argument_z) else {
                    return CommandKind::Diagnostic;
                };
                TransferTarget::Object(id)
            };
            CommandKind::DropItem(ItemTransfer {
                slot,
                quantity: argument_y,
                target,
            })
        }
        12 => CommandKind::DropGold(GoldTransfer {
            amount: argument_x,
            target: TransferTarget::Tile(unpack_tile(argument_y)),
        }),
        13 => match std::num::NonZeroU32::new(argument_y) {
            Some(id) => CommandKind::DropGold(GoldTransfer {
                amount: argument_x,
                target: TransferTarget::Object(id),
            }),
            None => CommandKind::Diagnostic,
        },
        14 => CommandKind::PickupItem(TilePosition {
            x: argument_x as i32,
            y: argument_y as i32,
        }),
        15 => EquipmentSlot::from_raw(argument_x as u8)
            .map(CommandKind::Unequip)
            .unwrap_or(CommandKind::Diagnostic),
        16 => CommandKind::Emote(argument_x as u8),
        17 | 18 => {
            let Some(slot) = ItemSlot::new(argument_x as u8) else {
                return CommandKind::Diagnostic;
            };
            let target = if value == 17 {
                TransferTarget::Tile(unpack_tile(argument_z))
            } else {
                let Some(id) = std::num::NonZeroU32::new(argument_z) else {
                    return CommandKind::Diagnostic;
                };
                TransferTarget::Object(id)
            };
            CommandKind::GiveItem(ItemTransfer {
                slot,
                quantity: argument_y,
                target,
            })
        }
        19 => CommandKind::GiveGold(GoldTransfer {
            amount: argument_x,
            target: TransferTarget::Tile(unpack_tile(argument_y)),
        }),
        20 => match std::num::NonZeroU32::new(argument_y) {
            Some(id) => CommandKind::GiveGold(GoldTransfer {
                amount: argument_x,
                target: TransferTarget::Object(id),
            }),
            None => CommandKind::Diagnostic,
        },
        21..=23 => {
            let swap = match value {
                21 => ItemSlot::new(argument_x as u8)
                    .zip(ItemSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Inventory {
                        source,
                        destination,
                    }),
                22 => SpellSlot::new(argument_x as u8)
                    .zip(SpellSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Spellbook {
                        source,
                        destination,
                    }),
                23 => SkillSlot::new(argument_x as u8)
                    .zip(SkillSlot::new(argument_y as u8))
                    .map(|(source, destination)| SlotSwap::Skillbook {
                        source,
                        destination,
                    }),
                _ => None,
            };
            swap.map(CommandKind::SwapSlots)
                .unwrap_or(CommandKind::Diagnostic)
        }
        24 => std::num::NonZeroU32::new(argument_x)
            .map(CommandKind::Interact)
            .unwrap_or(CommandKind::Diagnostic),
        25 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Select {
                index: argument_y as u16,
                quantity: argument_z as u8,
            },
        }),
        26 => {
            let Ok(text) = std::str::from_utf8(input) else {
                return CommandKind::Diagnostic;
            };
            let Some(text) = DialogText::new(text) else {
                return CommandKind::Diagnostic;
            };
            CommandKind::Dialog(DialogCommand {
                revision: argument_x,
                action: DialogAction::Input(text),
            })
        }
        27 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Previous,
        }),
        28 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Next,
        }),
        29 => CommandKind::Dialog(DialogCommand {
            revision: argument_x,
            action: DialogAction::Close,
        }),
        30 => {
            let Ok(target) = std::str::from_utf8(input) else {
                return CommandKind::Diagnostic;
            };
            GroupText::new(target)
                .map(|target| CommandKind::Group(GroupCommand::Invite(target)))
                .unwrap_or(CommandKind::Diagnostic)
        }
        31 | 32 => CommandKind::Group(GroupCommand::Respond {
            invitation_id: argument_x,
            action: if value == 31 {
                GroupInvitationAction::Accept
            } else {
                GroupInvitationAction::Decline
            },
        }),
        33 => CommandKind::Group(GroupCommand::Toggle),
        34 => CommandKind::Who,
        35 => ItemSlot::new(argument_x as u8)
            .map(|slot| {
                CommandKind::Exchange(ExchangeCommand::AddItem {
                    slot,
                    quantity: argument_y as u8,
                })
            })
            .unwrap_or(CommandKind::Diagnostic),
        36 => CommandKind::Exchange(ExchangeCommand::SetGold(argument_x)),
        37 => CommandKind::Exchange(ExchangeCommand::Accept),
        38 => CommandKind::Exchange(ExchangeCommand::Cancel),
        39 => std::str::from_utf8(input)
            .ok()
            .and_then(ChantText::new)
            .map(CommandKind::Chant)
            .unwrap_or(CommandKind::Diagnostic),
        40 => CommandKind::Legend,
        41 => {
            let direction = match argument_x {
                0 => RawPacketDirection::Client,
                1 => RawPacketDirection::Server,
                _ => return CommandKind::Diagnostic,
            };
            RawPacket::new(direction, argument_y as u8, input)
                .map(CommandKind::Raw)
                .unwrap_or(CommandKind::Diagnostic)
        }
        42 => CommandKind::Assail,
        _ => CommandKind::Diagnostic,
    }
}

fn pack_tile(position: TilePosition) -> u32 {
    let Ok(x) = u16::try_from(position.x) else {
        return u32::MAX;
    };
    let Ok(y) = u16::try_from(position.y) else {
        return u32::MAX;
    };
    u32::from(x) | (u32::from(y) << 16)
}

fn unpack_tile(value: u32) -> TilePosition {
    TilePosition {
        x: i32::from(value as u16),
        y: i32::from((value >> 16) as u16),
    }
}

const fn stored_direction(value: u32) -> Direction {
    match Direction::from_raw(value as u8) {
        Some(direction) => direction,
        None => Direction::North,
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
    }
}

const fn has_reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) < 0x8000_0000
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
            CommandKind::UseSkill(SkillSlot::new(7).unwrap()),
            CommandKind::Assail,
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
