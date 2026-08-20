use darpc_protocol::ExactRouteInvalidState;
use darpc_protocol::{
    DiagnosticsMode, DiagnosticsOperation, DiagnosticsResponse, HOOK_TIMING_STAGE_COUNT,
    HookTimingRecord, HookTimingStage,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

const STAGES: [HookTimingStage; HOOK_TIMING_STAGE_COUNT] = [
    HookTimingStage::Tick,
    HookTimingStage::Movement,
    HookTimingStage::Commands,
    HookTimingStage::Player,
    HookTimingStage::State,
    HookTimingStage::Snapshot,
    HookTimingStage::Event,
];
const BUDGETS_US: [u32; HOOK_TIMING_STAGE_COUNT] =
    [10_000, 5_000, 5_000, 5_000, 5_000, 5_000, 5_000];

struct Counters {
    call_count: AtomicU64,
    total_duration_us: AtomicU64,
    maximum_duration_us: AtomicU32,
    over_budget_count: AtomicU64,
    last_duration_us: AtomicU32,
}

impl Counters {
    const fn new() -> Self {
        Self {
            call_count: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
            maximum_duration_us: AtomicU32::new(0),
            over_budget_count: AtomicU64::new(0),
            last_duration_us: AtomicU32::new(0),
        }
    }

    fn reset(&self) {
        self.call_count.store(0, Ordering::Relaxed);
        self.total_duration_us.store(0, Ordering::Relaxed);
        self.maximum_duration_us.store(0, Ordering::Relaxed);
        self.over_budget_count.store(0, Ordering::Relaxed);
        self.last_duration_us.store(0, Ordering::Relaxed);
    }
}

static MODE: AtomicU8 = AtomicU8::new(DiagnosticsMode::Disabled as u8);
static COUNTERS: [Counters; HOOK_TIMING_STAGE_COUNT] =
    [const { Counters::new() }; HOOK_TIMING_STAGE_COUNT];

thread_local! {
    static EXACT_ROUTE_INVALID_STATE: Cell<Option<ExactRouteInvalidState>> = const { Cell::new(None) };
}

pub(crate) fn initialize(hook_timing: bool) {
    reset();
    set_mode(if hook_timing {
        DiagnosticsMode::HookTiming
    } else {
        DiagnosticsMode::Disabled
    });
}

pub(crate) fn disable() {
    set_mode(DiagnosticsMode::Disabled);
}

pub(crate) fn clear_invalid_exact_route_state() {
    EXACT_ROUTE_INVALID_STATE.set(None);
}

pub(crate) fn observe_invalid_exact_route_state(diagnostic: ExactRouteInvalidState) {
    EXACT_ROUTE_INVALID_STATE.set(Some(diagnostic));
}

pub(crate) fn take_invalid_exact_route_state() -> Option<ExactRouteInvalidState> {
    EXACT_ROUTE_INVALID_STATE.take()
}

#[inline]
pub(crate) fn hook_timing_enabled() -> bool {
    MODE.load(Ordering::Relaxed) == DiagnosticsMode::HookTiming as u8
}

#[inline]
pub(crate) fn measure<T>(stage: HookTimingStage, operation: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = operation();
    record(stage, elapsed_us(started));
    result
}

#[inline]
fn elapsed_us(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX)
}

#[inline]
fn record(stage: HookTimingStage, duration_us: u32) {
    let index = stage as usize;
    let counters = &COUNTERS[index];
    counters.call_count.fetch_add(1, Ordering::Relaxed);
    counters
        .total_duration_us
        .fetch_add(u64::from(duration_us), Ordering::Relaxed);
    counters
        .maximum_duration_us
        .fetch_max(duration_us, Ordering::Relaxed);
    counters
        .last_duration_us
        .store(duration_us, Ordering::Relaxed);
    if duration_us > BUDGETS_US[index] {
        counters.over_budget_count.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn handle(request_id: u32, operation: DiagnosticsOperation) -> DiagnosticsResponse {
    match operation {
        DiagnosticsOperation::Query => {}
        DiagnosticsOperation::EnableHookTiming => set_mode(DiagnosticsMode::HookTiming),
        DiagnosticsOperation::Disable => set_mode(DiagnosticsMode::Disabled),
        DiagnosticsOperation::Reset => reset(),
    }
    snapshot(request_id)
}

fn set_mode(mode: DiagnosticsMode) {
    MODE.store(mode as u8, Ordering::Release);
}

fn reset() {
    for counters in &COUNTERS {
        counters.reset();
    }
}

fn snapshot(request_id: u32) -> DiagnosticsResponse {
    let hook_timings = std::array::from_fn(|index| {
        let counters = &COUNTERS[index];
        HookTimingRecord {
            stage: STAGES[index],
            budget_us: BUDGETS_US[index],
            call_count: counters.call_count.load(Ordering::Relaxed),
            total_duration_us: counters.total_duration_us.load(Ordering::Relaxed),
            maximum_duration_us: counters.maximum_duration_us.load(Ordering::Relaxed),
            over_budget_count: counters.over_budget_count.load(Ordering::Relaxed),
            last_duration_us: counters.last_duration_us.load(Ordering::Relaxed),
        }
    });
    let mode = if hook_timing_enabled() {
        DiagnosticsMode::HookTiming
    } else {
        DiagnosticsMode::Disabled
    };
    DiagnosticsResponse {
        request_id,
        mode,
        hook_timings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_protocol::ExactRouteInvalidStateReason;

    #[test]
    fn runtime_control_resets_and_preserves_mode() {
        initialize(false);
        assert_eq!(
            handle(1, DiagnosticsOperation::Query).mode,
            DiagnosticsMode::Disabled
        );
        assert_eq!(
            handle(2, DiagnosticsOperation::EnableHookTiming).mode,
            DiagnosticsMode::HookTiming
        );
        measure(HookTimingStage::Commands, || {});
        assert_eq!(
            handle(3, DiagnosticsOperation::Query).hook_timings[2].call_count,
            1
        );
        let reset = handle(4, DiagnosticsOperation::Reset);
        assert_eq!(reset.mode, DiagnosticsMode::HookTiming);
        assert_eq!(reset.hook_timings[2].call_count, 0);
        disable();
    }

    #[test]
    fn exact_route_invalid_state_is_scoped_to_the_executing_thread() {
        let diagnostics = ExactRouteInvalidState {
            reason: ExactRouteInvalidStateReason::NativeMapMismatch,
            route_map_id: 500,
            packet_map_id: Some(500),
            native_map_id: Some(501),
            packet_position: None,
            native_position: None,
            staged_position: None,
            transition_active: None,
            route_mode: None,
            current_destination: None,
        };

        clear_invalid_exact_route_state();
        observe_invalid_exact_route_state(diagnostics);
        assert_eq!(take_invalid_exact_route_state(), Some(diagnostics));
        assert_eq!(take_invalid_exact_route_state(), None);
    }
}
