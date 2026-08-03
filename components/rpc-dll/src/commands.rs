use darpc_model::Direction;
use darpc_protocol::{
    CommandFailure, CommandKind, CommandOperation, CommandResult, CommandState, CommandStatus,
    MAX_SPELL_INPUT_LEN, SkillSlot, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget,
    WalkTarget,
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
    argument_bytes: [AtomicU8; MAX_SPELL_INPUT_LEN],
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
            argument_bytes: [const { AtomicU8::new(0) }; MAX_SPELL_INPUT_LEN],
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
        let input = input.as_ref().map_or(&[][..], SpellInput::as_bytes);
        self.argument_length.store(
            u8::try_from(input.len()).expect("spell input limit fits u8"),
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
        let mut input = [0; MAX_SPELL_INPUT_LEN];
        for (destination, source) in input.iter_mut().zip(&self.argument_bytes).take(length) {
            *destination = source.load(Ordering::Relaxed);
        }
        kind_from_value(
            self.kind.load(Ordering::Relaxed),
            self.argument_x.load(Ordering::Relaxed),
            self.argument_y.load(Ordering::Relaxed),
            self.argument_z.load(Ordering::Relaxed),
            &input[..length.min(MAX_SPELL_INPUT_LEN)],
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
        let _ =
            slot.state
                .compare_exchange(ACCEPTED, CANCELLED, Ordering::Release, Ordering::Relaxed);
    }
}

fn submit(kind: CommandKind, timeout_ms: u16) -> Option<CommandStatus> {
    let now = now_tick_ms();
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
    let _ = slot
        .state
        .compare_exchange(ACCEPTED, CANCELLED, Ordering::Release, Ordering::Relaxed);
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
    let result =
        panic::catch_unwind(|| execute_command(kind)).unwrap_or(Err(CommandFailure::Internal));
    let execution_us = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
    slot.execution_us.store(execution_us, Ordering::Relaxed);
    slot.has_execution_us.store(true, Ordering::Relaxed);
    slot.completed_tick_ms
        .store(now_tick_ms(), Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    complete_execution(slot, result);
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

fn expire_if_due(slot: &CommandSlot, now: u32) {
    if !has_reached(now, slot.deadline_tick_ms.load(Ordering::Relaxed)) {
        return;
    }
    slot.completed_tick_ms.store(now, Ordering::Relaxed);
    slot.has_completed_tick_ms.store(true, Ordering::Relaxed);
    let _ = slot
        .state
        .compare_exchange(ACCEPTED, TIMED_OUT, Ordering::Release, Ordering::Relaxed);
}

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

fn stored_kind(kind: CommandKind) -> (u8, u32, u32, u32, Option<SpellInput>) {
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
        }) => (8, slot.get() as u32, 0, 0, Some(input)),
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
        _ => CommandKind::Diagnostic,
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
