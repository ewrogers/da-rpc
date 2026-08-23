use darpc_model::TilePosition;
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

#[cfg(all(windows, not(test)))]
use darpc_protocol::CommandFailure;

const PENDING_CAPACITY: usize = crate::commands::COMMAND_CAPACITY;
const RESPONSE_TIMEOUT_MS: u32 = 1_000;
const RESPONSE_QUIET_MS: u32 = 1_000;

static PHYSICAL_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);
static SUBMISSION_COUNT: AtomicU32 = AtomicU32::new(0);
static COMPLETION_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMEOUT_COUNT: AtomicU32 = AtomicU32::new(0);
static SUBMISSION_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);
static COORDINATOR: MainThreadCoordinator = MainThreadCoordinator::new();

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResyncHealth {
    pub(crate) submission_count: u32,
    pub(crate) completion_count: u32,
    pub(crate) timeout_count: u32,
    pub(crate) submission_failure_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Origin {
    Command(u32),
    Physical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Idle,
    Quiescing {
        origin: Origin,
        expected_commit: TilePosition,
    },
    Submitting {
        origin: Origin,
    },
    AwaitingResponse {
        origin: Origin,
        resync_id: u32,
        deadline_tick_ms: u32,
    },
    CoolingOff {
        deadline_tick_ms: u32,
    },
}

impl Phase {
    const fn origin(self) -> Option<Origin> {
        match self {
            Self::Idle => None,
            Self::Quiescing { origin, .. }
            | Self::Submitting { origin }
            | Self::AwaitingResponse { origin, .. } => Some(origin),
            Self::CoolingOff { .. } => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Coordinator {
    phase: Phase,
    pending: [Option<Origin>; PENDING_CAPACITY],
    head: usize,
    len: usize,
}

impl Coordinator {
    const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            pending: [None; PENDING_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    const fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    const fn has_pending(&self) -> bool {
        self.len != 0
    }

    fn enqueue(&mut self, origin: Origin) -> bool {
        if origin == Origin::Physical && self.contains_physical() {
            return true;
        }
        if self.len == self.pending.len() {
            return false;
        }
        let tail = (self.head + self.len) % self.pending.len();
        self.pending[tail] = Some(origin);
        self.len += 1;
        true
    }

    fn peek(&self) -> Option<Origin> {
        self.pending.get(self.head).copied().flatten()
    }

    fn pop(&mut self) -> Option<Origin> {
        let origin = self.peek()?;
        self.pending[self.head] = None;
        self.head = (self.head + 1) % self.pending.len();
        self.len -= 1;
        Some(origin)
    }

    fn begin(&mut self, origin: Origin, transition: MovementTransition) {
        debug_assert!(self.is_idle());
        self.phase = Phase::Quiescing {
            origin,
            expected_commit: transition.expected_commit(),
        };
    }

    fn observe_transition(&mut self, transition: MovementTransition) -> Option<Origin> {
        let Phase::Quiescing {
            origin,
            expected_commit,
        } = &mut self.phase
        else {
            return None;
        };
        if transition.active {
            *expected_commit = transition.staged;
            return None;
        }
        if transition.committed != *expected_commit {
            *expected_commit = transition.committed;
            return None;
        }
        let origin = *origin;
        self.phase = Phase::Submitting { origin };
        Some(origin)
    }

    fn observe_outgoing(&mut self, resync_id: u32, tick_ms: u32) -> bool {
        let Phase::Submitting { origin } = self.phase else {
            return false;
        };
        self.phase = Phase::AwaitingResponse {
            origin,
            resync_id,
            deadline_tick_ms: tick_ms.wrapping_add(RESPONSE_TIMEOUT_MS),
        };
        true
    }

    fn observe_timeout(&mut self, tick_ms: u32) -> Option<u32> {
        let Phase::AwaitingResponse {
            resync_id,
            deadline_tick_ms,
            ..
        } = self.phase
        else {
            return None;
        };
        if !crate::wrapping_time::deadline_reached(tick_ms, deadline_tick_ms) {
            return None;
        }
        self.phase = Phase::CoolingOff {
            deadline_tick_ms: tick_ms.wrapping_add(RESPONSE_QUIET_MS),
        };
        Some(resync_id)
    }

    fn observe_unmatched_completion(&mut self, tick_ms: u32) {
        if matches!(self.phase, Phase::CoolingOff { .. }) {
            self.phase = Phase::CoolingOff {
                deadline_tick_ms: tick_ms.wrapping_add(RESPONSE_QUIET_MS),
            };
        }
    }

    fn finish_cooldown(&mut self, tick_ms: u32) -> bool {
        let Phase::CoolingOff { deadline_tick_ms } = self.phase else {
            return false;
        };
        if !crate::wrapping_time::deadline_reached(tick_ms, deadline_tick_ms) {
            return false;
        }
        self.phase = Phase::Idle;
        true
    }

    fn observe_completed(&mut self, resync_id: u32) -> bool {
        if matches!(
            self.phase,
            Phase::AwaitingResponse {
                resync_id: expected,
                ..
            } if expected == resync_id
        ) {
            self.phase = Phase::Idle;
            return true;
        }
        false
    }

    fn submission_failed(&mut self, origin: Origin) -> bool {
        if self.phase == (Phase::Submitting { origin }) {
            self.phase = Phase::Idle;
            return true;
        }
        false
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn contains_physical(&self) -> bool {
        if self.phase.origin() == Some(Origin::Physical) {
            return true;
        }
        (0..self.len).any(|offset| {
            let index = (self.head + offset) % self.pending.len();
            self.pending[index] == Some(Origin::Physical)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MovementTransition {
    committed: TilePosition,
    staged: TilePosition,
    active: bool,
}

impl MovementTransition {
    const fn expected_commit(self) -> TilePosition {
        if self.active {
            self.staged
        } else {
            self.committed
        }
    }
}

#[cfg(all(windows, not(test)))]
impl From<crate::movement_transition::LocalMovementTransition> for MovementTransition {
    fn from(transition: crate::movement_transition::LocalMovementTransition) -> Self {
        Self {
            committed: transition.committed_position(),
            staged: transition.staged_position(),
            active: transition.is_active(),
        }
    }
}

struct MainThreadCoordinator(UnsafeCell<Coordinator>);

// SAFETY: coordinator mutation is restricted to the client main thread except
// during reset, which runs only while its producer hooks are absent.
unsafe impl Sync for MainThreadCoordinator {}

impl MainThreadCoordinator {
    const fn new() -> Self {
        Self(UnsafeCell::new(Coordinator::new()))
    }

    unsafe fn with<R>(&self, action: impl FnOnce(&mut Coordinator) -> R) -> R {
        // SAFETY: the caller guarantees exclusive main-thread or lifecycle access.
        action(unsafe { &mut *self.0.get() })
    }
}

#[cfg(windows)]
pub(crate) fn request_physical() {
    PHYSICAL_REFRESH_REQUESTED.store(true, Ordering::Release);
}

#[cfg(all(windows, not(test)))]
pub(crate) fn request_command(command_id: u32) -> Result<(), CommandFailure> {
    // SAFETY: command execution runs on the client main thread.
    let start_now = unsafe {
        COORDINATOR.with(|coordinator| coordinator.is_idle() && !coordinator.has_pending())
    };
    if start_now {
        let transition = crate::actions::begin_resync_transition()?;
        // SAFETY: command execution runs on the client main thread.
        unsafe {
            COORDINATOR.with(|coordinator| {
                coordinator.begin(Origin::Command(command_id), transition.into());
            });
        }
        return Ok(());
    }
    // SAFETY: command execution runs on the client main thread.
    unsafe { COORDINATOR.with(|coordinator| coordinator.enqueue(Origin::Command(command_id))) }
        .then_some(())
        .ok_or(CommandFailure::Rejected)
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe_tick(tick_ms: u32) {
    if PHYSICAL_REFRESH_REQUESTED.swap(false, Ordering::AcqRel) {
        // SAFETY: tick observation runs on the client main thread.
        let _ = unsafe { COORDINATOR.with(|coordinator| coordinator.enqueue(Origin::Physical)) };
    }

    // SAFETY: tick observation runs on the client main thread.
    let timed_out = unsafe { COORDINATOR.with(|coordinator| coordinator.observe_timeout(tick_ms)) };
    if let Some(resync_id) = timed_out {
        TIMEOUT_COUNT.fetch_add(1, Ordering::Relaxed);
        crate::state::observe_resync_timed_out(resync_id, tick_ms);
    }

    // SAFETY: tick observation runs on the client main thread.
    unsafe {
        COORDINATOR.with(|coordinator| {
            coordinator.finish_cooldown(tick_ms);
        });
    }
    begin_pending();

    // SAFETY: tick observation runs on the client main thread.
    let quiescing = unsafe {
        COORDINATOR.with(|coordinator| matches!(coordinator.phase, Phase::Quiescing { .. }))
    };
    if !quiescing {
        return;
    }
    let Ok(transition) = crate::actions::resync_transition() else {
        return;
    };
    // SAFETY: tick observation runs on the client main thread.
    let origin = unsafe {
        COORDINATOR.with(|coordinator| coordinator.observe_transition(transition.into()))
    };
    let Some(origin) = origin else {
        return;
    };

    let command_id = match origin {
        Origin::Command(command_id) => command_id,
        Origin::Physical => 0,
    };
    if command_id != 0 {
        crate::commands::begin_resync_submission(command_id);
    }
    let result = crate::actions::submit_resync_packet();
    if command_id != 0 {
        crate::commands::end_resync_submission(command_id);
    }
    if result.is_err() {
        // SAFETY: tick observation runs on the client main thread.
        let failed =
            unsafe { COORDINATOR.with(|coordinator| coordinator.submission_failed(origin)) };
        if failed {
            SUBMISSION_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(all(windows, not(test)))]
fn begin_pending() {
    // SAFETY: tick observation runs on the client main thread.
    let origin = unsafe {
        COORDINATOR.with(|coordinator| coordinator.is_idle().then(|| coordinator.peek()).flatten())
    };
    let Some(origin) = origin else {
        return;
    };
    let Ok(transition) = crate::actions::begin_resync_transition() else {
        return;
    };
    // SAFETY: tick observation runs on the client main thread.
    unsafe {
        COORDINATOR.with(|coordinator| {
            debug_assert_eq!(coordinator.pop(), Some(origin));
            coordinator.begin(origin, transition.into());
        });
    }
}

pub(crate) fn observe_outgoing(resync_id: u32, tick_ms: u32) {
    // SAFETY: outgoing observation runs on the client main thread.
    let submitted =
        unsafe { COORDINATOR.with(|coordinator| coordinator.observe_outgoing(resync_id, tick_ms)) };
    if submitted {
        SUBMISSION_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn observe_completed(resync_id: u32) {
    // SAFETY: decoded server events run on the client main thread.
    let completed =
        unsafe { COORDINATOR.with(|coordinator| coordinator.observe_completed(resync_id)) };
    if completed {
        COMPLETION_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn observe_unmatched_completion(tick_ms: u32) {
    // SAFETY: decoded server events run on the client main thread.
    unsafe {
        COORDINATOR.with(|coordinator| coordinator.observe_unmatched_completion(tick_ms));
    }
}

#[cfg(windows)]
pub(crate) fn health() -> ResyncHealth {
    ResyncHealth {
        submission_count: SUBMISSION_COUNT.load(Ordering::Acquire),
        completion_count: COMPLETION_COUNT.load(Ordering::Acquire),
        timeout_count: TIMEOUT_COUNT.load(Ordering::Acquire),
        submission_failure_count: SUBMISSION_FAILURE_COUNT.load(Ordering::Acquire),
    }
}

pub(crate) fn reset() {
    PHYSICAL_REFRESH_REQUESTED.store(false, Ordering::Release);
    SUBMISSION_COUNT.store(0, Ordering::Release);
    COMPLETION_COUNT.store(0, Ordering::Release);
    TIMEOUT_COUNT.store(0, Ordering::Release);
    SUBMISSION_FAILURE_COUNT.store(0, Ordering::Release);
    // SAFETY: lifecycle reset runs while the producer hooks are absent.
    unsafe {
        COORDINATOR.with(Coordinator::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::{Coordinator, MovementTransition, Origin, Phase};
    use darpc_model::TilePosition;

    const COMMITTED: TilePosition = TilePosition { x: 10, y: 20 };
    const STAGED: TilePosition = TilePosition { x: 11, y: 20 };

    const fn transition(
        committed: TilePosition,
        staged: TilePosition,
        active: bool,
    ) -> MovementTransition {
        MovementTransition {
            committed,
            staged,
            active,
        }
    }

    #[test]
    fn active_step_must_commit_before_refresh_submission() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(7), transition(COMMITTED, STAGED, true));

        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, STAGED, true)),
            None
        );
        assert_eq!(
            coordinator.observe_transition(transition(STAGED, STAGED, false)),
            Some(Origin::Command(7))
        );
        assert_eq!(
            coordinator.phase,
            Phase::Submitting {
                origin: Origin::Command(7)
            }
        );
    }

    #[test]
    fn changed_committed_position_must_be_stable_for_one_tick() {
        let corrected = TilePosition { x: 9, y: 20 };
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Physical, transition(COMMITTED, STAGED, true));

        assert_eq!(
            coordinator.observe_transition(transition(corrected, corrected, false)),
            None
        );
        assert_eq!(
            coordinator.observe_transition(transition(corrected, corrected, false)),
            Some(Origin::Physical)
        );
    }

    #[test]
    fn physical_refreshes_coalesce_while_commands_remain_ordered() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(1), transition(COMMITTED, COMMITTED, false));

        assert!(coordinator.enqueue(Origin::Physical));
        assert!(coordinator.enqueue(Origin::Physical));
        assert!(coordinator.enqueue(Origin::Command(2)));
        assert!(coordinator.has_pending());
        assert_eq!(coordinator.len, 2);
        assert_eq!(coordinator.pop(), Some(Origin::Physical));
        assert_eq!(coordinator.pop(), Some(Origin::Command(2)));
    }

    #[test]
    fn only_matching_response_releases_the_next_refresh() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(12), transition(COMMITTED, COMMITTED, false));
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(12))
        );
        assert!(coordinator.observe_outgoing(12, 100));

        assert!(!coordinator.observe_completed(99));
        assert!(!coordinator.is_idle());
        assert!(coordinator.observe_completed(12));
        assert!(coordinator.is_idle());
    }

    #[test]
    fn missing_response_times_out_and_waits_for_a_quiet_second() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(12), transition(COMMITTED, COMMITTED, false));
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(12))
        );
        assert!(coordinator.observe_outgoing(12, 100));

        assert_eq!(coordinator.observe_timeout(1_099), None);
        assert_eq!(coordinator.observe_timeout(1_100), Some(12));
        assert!(!coordinator.finish_cooldown(2_099));

        coordinator.observe_unmatched_completion(1_500);
        assert!(!coordinator.finish_cooldown(2_499));
        assert!(coordinator.finish_cooldown(2_500));
        assert!(coordinator.is_idle());
    }

    #[test]
    fn failed_submission_releases_the_coordinator() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Physical, transition(COMMITTED, COMMITTED, false));
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Physical)
        );

        assert!(coordinator.submission_failed(Origin::Physical));
        assert!(coordinator.is_idle());
    }
}
