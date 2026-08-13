use super::*;

pub(super) struct CommandSlot {
    pub(super) state: AtomicU8,
    pub(super) queued: AtomicBool,
    pub(super) command_id: AtomicU32,
    pub(super) kind: AtomicU8,
    pub(super) argument_x: AtomicU32,
    pub(super) argument_y: AtomicU32,
    pub(super) argument_z: AtomicU32,
    pub(super) argument_length: AtomicU16,
    pub(super) argument_bytes: [AtomicU8; MAX_COMMAND_TILE_BYTES],
    pub(super) enqueued_tick_ms: AtomicU32,
    pub(super) deadline_tick_ms: AtomicU32,
    pub(super) started_tick_ms: AtomicU32,
    pub(super) has_started_tick_ms: AtomicBool,
    pub(super) completed_tick_ms: AtomicU32,
    pub(super) has_completed_tick_ms: AtomicBool,
    pub(super) execution_us: AtomicU32,
    pub(super) has_execution_us: AtomicBool,
    pub(super) main_thread_id: AtomicU32,
    pub(super) has_main_thread_id: AtomicBool,
    pub(super) failure: AtomicU8,
}

impl CommandSlot {
    pub(super) const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            queued: AtomicBool::new(false),
            command_id: AtomicU32::new(0),
            kind: AtomicU8::new(0),
            argument_x: AtomicU32::new(0),
            argument_y: AtomicU32::new(0),
            argument_z: AtomicU32::new(0),
            argument_length: AtomicU16::new(0),
            argument_bytes: [const { AtomicU8::new(0) }; MAX_COMMAND_TILE_BYTES],
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

    pub(super) fn initialize(&self, command_id: u32, kind: CommandKind, now: u32, timeout_ms: u16) {
        let (kind, argument_x, argument_y, argument_z, input) = stored_kind(kind);
        self.command_id.store(command_id, Ordering::Relaxed);
        self.kind.store(kind, Ordering::Relaxed);
        self.argument_x.store(argument_x, Ordering::Relaxed);
        self.argument_y.store(argument_y, Ordering::Relaxed);
        self.argument_z.store(argument_z, Ordering::Relaxed);
        let input = input.as_ref().map_or(&[][..], StoredInput::as_bytes);
        self.argument_length.store(
            u16::try_from(input.len()).expect("command input limit fits u16"),
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

    pub(super) fn status(&self, expected_id: u32) -> Option<CommandStatus> {
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

    pub(super) fn clear(&self) {
        self.queued.store(false, Ordering::Relaxed);
        self.command_id.store(0, Ordering::Relaxed);
        self.state.store(EMPTY, Ordering::Release);
    }

    pub(super) fn kind(&self) -> CommandKind {
        let length = usize::from(self.argument_length.load(Ordering::Relaxed));
        let mut input = [0; MAX_COMMAND_TILE_BYTES];
        for (destination, source) in input.iter_mut().zip(&self.argument_bytes).take(length) {
            *destination = source.load(Ordering::Relaxed);
        }
        kind_from_value(
            self.kind.load(Ordering::Relaxed),
            self.argument_x.load(Ordering::Relaxed),
            self.argument_y.load(Ordering::Relaxed),
            self.argument_z.load(Ordering::Relaxed),
            &input[..length.min(MAX_COMMAND_TILE_BYTES)],
        )
    }
}

pub(super) struct CommandQueue {
    pub(super) entries: [AtomicUsize; COMMAND_CAPACITY],
    pub(super) head: AtomicUsize,
    pub(super) tail: AtomicUsize,
}

impl CommandQueue {
    pub(super) const fn new() -> Self {
        Self {
            entries: [const { AtomicUsize::new(0) }; COMMAND_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub(super) fn push(&self, slot_index: usize) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= COMMAND_CAPACITY {
            return false;
        }
        self.entries[tail % COMMAND_CAPACITY].store(slot_index, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    pub(super) fn pop(&self) -> Option<usize> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let slot_index = self.entries[head % COMMAND_CAPACITY].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(slot_index)
    }

    pub(super) fn reset(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        for entry in &self.entries {
            entry.store(0, Ordering::Relaxed);
        }
    }
}
