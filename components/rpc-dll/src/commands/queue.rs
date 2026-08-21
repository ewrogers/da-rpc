use super::*;

const EXACT_ROUTE_DIAGNOSTIC_BYTES: usize = 48;

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
    exact_route_diagnostic: [AtomicU8; EXACT_ROUTE_DIAGNOSTIC_BYTES],
    has_exact_route_diagnostic: AtomicBool,
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
            exact_route_diagnostic: [const { AtomicU8::new(0) }; EXACT_ROUTE_DIAGNOSTIC_BYTES],
            has_exact_route_diagnostic: AtomicBool::new(false),
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
        self.has_exact_route_diagnostic
            .store(false, Ordering::Relaxed);
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

    pub(super) fn store_exact_route_diagnostic(&self, value: ExactRouteInvalidState) {
        let bytes = encode_exact_route_diagnostic(value);
        self.has_exact_route_diagnostic
            .store(false, Ordering::Relaxed);
        for (destination, source) in self.exact_route_diagnostic.iter().zip(bytes) {
            destination.store(source, Ordering::Relaxed);
        }
        self.has_exact_route_diagnostic
            .store(true, Ordering::Release);
    }

    pub(super) fn exact_route_diagnostic(&self) -> Option<ExactRouteInvalidState> {
        if !self.has_exact_route_diagnostic.load(Ordering::Acquire) {
            return None;
        }
        let mut bytes = [0; EXACT_ROUTE_DIAGNOSTIC_BYTES];
        for (destination, source) in bytes.iter_mut().zip(&self.exact_route_diagnostic) {
            *destination = source.load(Ordering::Relaxed);
        }
        Some(decode_exact_route_diagnostic(bytes))
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

fn encode_exact_route_diagnostic(
    value: ExactRouteInvalidState,
) -> [u8; EXACT_ROUTE_DIAGNOSTIC_BYTES] {
    let mut bytes = [0; EXACT_ROUTE_DIAGNOSTIC_BYTES];
    bytes[0] = match value.reason {
        ExactRouteInvalidStateReason::MapTransitionPending => 0,
        ExactRouteInvalidStateReason::NativeMapUnavailable => 1,
        ExactRouteInvalidStateReason::NativeMapMismatch => 2,
        ExactRouteInvalidStateReason::NativeTransitionUnavailable => 3,
        ExactRouteInvalidStateReason::ConfirmedMapMismatch => 4,
        ExactRouteInvalidStateReason::ConfirmedPositionMismatch => 5,
        ExactRouteInvalidStateReason::MapDimensionsUnavailable => 6,
    };
    write_u32(&mut bytes, 1, value.route_map_id);
    let mut flags = 0;
    write_optional_u32(&mut bytes, 6, value.packet_map_id, &mut flags, 0x01);
    write_optional_u32(&mut bytes, 10, value.native_map_id, &mut flags, 0x02);
    write_optional_position(&mut bytes, 14, value.packet_position, &mut flags, 0x04);
    write_optional_position(&mut bytes, 22, value.native_position, &mut flags, 0x08);
    write_optional_position(&mut bytes, 30, value.staged_position, &mut flags, 0x10);
    if let Some(active) = value.transition_active {
        flags |= 0x20;
        bytes[38] = u8::from(active);
    }
    if let Some(mode) = value.route_mode {
        flags |= 0x40;
        bytes[39] = match mode {
            darpc_model::WalkMode::NativeRoute => 0,
            darpc_model::WalkMode::ExactRoute => 1,
            darpc_model::WalkMode::Direct => 2,
            darpc_model::WalkMode::Pursuit => 3,
        };
    }
    write_optional_position(&mut bytes, 40, value.current_destination, &mut flags, 0x80);
    bytes[5] = flags;
    bytes
}

fn decode_exact_route_diagnostic(
    bytes: [u8; EXACT_ROUTE_DIAGNOSTIC_BYTES],
) -> ExactRouteInvalidState {
    let flags = bytes[5];
    ExactRouteInvalidState {
        reason: match bytes[0] {
            0 => ExactRouteInvalidStateReason::MapTransitionPending,
            1 => ExactRouteInvalidStateReason::NativeMapUnavailable,
            2 => ExactRouteInvalidStateReason::NativeMapMismatch,
            3 => ExactRouteInvalidStateReason::NativeTransitionUnavailable,
            4 => ExactRouteInvalidStateReason::ConfirmedMapMismatch,
            5 => ExactRouteInvalidStateReason::ConfirmedPositionMismatch,
            6 => ExactRouteInvalidStateReason::MapDimensionsUnavailable,
            _ => unreachable!("stored exact-route diagnostic reason is valid"),
        },
        route_map_id: read_u32(&bytes, 1),
        packet_map_id: read_optional_u32(&bytes, 6, flags, 0x01),
        native_map_id: read_optional_u32(&bytes, 10, flags, 0x02),
        packet_position: read_optional_position(&bytes, 14, flags, 0x04),
        native_position: read_optional_position(&bytes, 22, flags, 0x08),
        staged_position: read_optional_position(&bytes, 30, flags, 0x10),
        transition_active: (flags & 0x20 != 0).then(|| bytes[38] != 0),
        route_mode: (flags & 0x40 != 0).then(|| match bytes[39] {
            0 => darpc_model::WalkMode::NativeRoute,
            1 => darpc_model::WalkMode::ExactRoute,
            2 => darpc_model::WalkMode::Direct,
            3 => darpc_model::WalkMode::Pursuit,
            _ => unreachable!("stored exact-route diagnostic mode is valid"),
        }),
        current_destination: read_optional_position(&bytes, 40, flags, 0x80),
    }
}

fn write_optional_u32(
    bytes: &mut [u8],
    offset: usize,
    value: Option<u32>,
    flags: &mut u8,
    flag: u8,
) {
    if let Some(value) = value {
        *flags |= flag;
        write_u32(bytes, offset, value);
    }
}

fn read_optional_u32(bytes: &[u8], offset: usize, flags: u8, flag: u8) -> Option<u32> {
    (flags & flag != 0).then(|| read_u32(bytes, offset))
}

fn write_optional_position(
    bytes: &mut [u8],
    offset: usize,
    value: Option<darpc_model::TilePosition>,
    flags: &mut u8,
    flag: u8,
) {
    if let Some(value) = value {
        *flags |= flag;
        write_u32(bytes, offset, value.x as u32);
        write_u32(bytes, offset + 4, value.y as u32);
    }
}

fn read_optional_position(
    bytes: &[u8],
    offset: usize,
    flags: u8,
    flag: u8,
) -> Option<darpc_model::TilePosition> {
    (flags & flag != 0).then(|| darpc_model::TilePosition {
        x: read_u32(bytes, offset) as i32,
        y: read_u32(bytes, offset + 4) as i32,
    })
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("stored diagnostic contains four bytes"),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::WalkMode;

    #[test]
    fn retained_slot_preserves_exact_route_invalid_state_diagnostics() {
        let slot = CommandSlot::new();
        let diagnostics = ExactRouteInvalidState {
            reason: ExactRouteInvalidStateReason::ConfirmedPositionMismatch,
            route_map_id: 500,
            packet_map_id: Some(500),
            native_map_id: Some(500),
            packet_position: Some(darpc_model::TilePosition { x: 10, y: 20 }),
            native_position: Some(darpc_model::TilePosition { x: 10, y: 20 }),
            staged_position: Some(darpc_model::TilePosition { x: 11, y: 20 }),
            transition_active: Some(true),
            route_mode: Some(WalkMode::ExactRoute),
            current_destination: Some(darpc_model::TilePosition { x: 30, y: 20 }),
        };

        assert_eq!(slot.exact_route_diagnostic(), None);
        slot.store_exact_route_diagnostic(diagnostics);
        assert_eq!(slot.exact_route_diagnostic(), Some(diagnostics));
    }
}
