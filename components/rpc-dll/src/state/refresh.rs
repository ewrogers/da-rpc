//! Owns the complete DLL-local user refresh transaction.
//!
//! Callers report refresh requests, outgoing packets, authoritative redraw
//! activity, and completion packets through this module. It alone sequences
//! movement gating, object reconciliation, completion, fallback, and deferred
//! snapshot recovery on the client main thread.

use super::{
    OBJECTS, QueuedStateUpdate, mark_resync_required, mark_resync_required_after_events, push_event,
};
use darpc_model::{ActionUpdate, TilePosition};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

#[cfg(all(windows, not(test)))]
use darpc_protocol::CommandFailure;

const RESPONSE_TIMEOUT_MS: u32 = 1_000;

static PHYSICAL_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);
static SUBMISSION_COUNT: AtomicU32 = AtomicU32::new(0);
static COMPLETION_COUNT: AtomicU32 = AtomicU32::new(0);
static FALLBACK_COUNT: AtomicU32 = AtomicU32::new(0);
static SUBMISSION_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);
static COORDINATOR: MainThreadCoordinator = MainThreadCoordinator::new();

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResyncHealth {
    pub(crate) submission_count: u32,
    pub(crate) completion_count: u32,
    pub(crate) fallback_count: u32,
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
        resync_id: u32,
        deadline_tick_ms: u32,
        response_observed: bool,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct Coordinator {
    phase: Phase,
    deferred_snapshot: bool,
}

impl Coordinator {
    const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            deferred_snapshot: false,
        }
    }

    const fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
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
        match self.phase {
            Phase::Submitting { .. } | Phase::Idle => {}
            Phase::Quiescing { .. } | Phase::AwaitingResponse { .. } => return false,
        }
        self.phase = Phase::AwaitingResponse {
            resync_id,
            deadline_tick_ms: tick_ms.wrapping_add(RESPONSE_TIMEOUT_MS),
            response_observed: false,
        };
        true
    }

    fn observe_response_activity(&mut self) {
        if let Phase::AwaitingResponse {
            response_observed, ..
        } = &mut self.phase
        {
            *response_observed = true;
        }
    }

    fn defer_snapshot(&mut self) -> bool {
        if self.is_idle() {
            return false;
        }
        self.deferred_snapshot = true;
        true
    }

    fn take_deferred_snapshot(&mut self) -> bool {
        core::mem::take(&mut self.deferred_snapshot)
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
        self.phase = Phase::Idle;
        Some(resync_id)
    }

    fn observe_completed(&mut self) -> Option<u32> {
        let Phase::AwaitingResponse {
            resync_id,
            response_observed: true,
            ..
        } = self.phase
        else {
            return None;
        };
        self.phase = Phase::Idle;
        Some(resync_id)
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
    let idle = unsafe { COORDINATOR.with(|coordinator| coordinator.is_idle()) };
    if !idle {
        return Err(CommandFailure::Rejected);
    }
    let transition = crate::actions::begin_resync_transition()?;
    // SAFETY: command execution runs on the client main thread.
    unsafe {
        COORDINATOR.with(|coordinator| {
            coordinator.begin(Origin::Command(command_id), transition.into());
        });
    }
    Ok(())
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe_tick(tick_ms: u32) {
    if PHYSICAL_REFRESH_REQUESTED.swap(false, Ordering::AcqRel) {
        // SAFETY: tick observation runs on the client main thread.
        let idle = unsafe { COORDINATOR.with(|coordinator| coordinator.is_idle()) };
        if idle && let Ok(transition) = crate::actions::begin_resync_transition() {
            // SAFETY: tick observation runs on the client main thread.
            unsafe {
                COORDINATOR.with(|coordinator| {
                    coordinator.begin(Origin::Physical, transition.into());
                });
            }
        }
    }

    // SAFETY: tick observation runs on the client main thread.
    let timed_out = unsafe { COORDINATOR.with(|coordinator| coordinator.observe_timeout(tick_ms)) };
    if let Some(resync_id) = timed_out {
        FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
        COMPLETION_COUNT.fetch_add(1, Ordering::Relaxed);
        complete_transaction(resync_id, tick_ms);
    }

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
            if take_deferred_snapshot() {
                mark_resync_required();
            }
        }
    }
}

pub(super) fn observe_outgoing(resync_id: u32, tick_ms: u32) {
    // SAFETY: outgoing observation runs on the client main thread.
    let submitted =
        unsafe { COORDINATOR.with(|coordinator| coordinator.observe_outgoing(resync_id, tick_ms)) };
    if !submitted {
        return;
    }
    SUBMISSION_COUNT.fetch_add(1, Ordering::Relaxed);

    // SAFETY: outgoing packet observation runs on the client main thread,
    // which is the sole owner of the object cache.
    unsafe { OBJECTS.begin_reconciliation() };
    push_event(
        QueuedStateUpdate::Action(ActionUpdate::Resync { resync_id }),
        tick_ms,
    );
}

pub(super) fn observe_authoritative_activity(tick_ms: u32) {
    // SAFETY: decoded server events run on the client main thread.
    unsafe { COORDINATOR.with(Coordinator::observe_response_activity) };
    // SAFETY: decoded server events run on the client main thread, which is
    // the sole owner of the object cache.
    unsafe { OBJECTS.observe_reconciliation_activity(tick_ms) };
}

pub(super) fn snapshot_required() {
    // SAFETY: decoded server events run on the client main thread.
    let deferred = unsafe { COORDINATOR.with(Coordinator::defer_snapshot) };
    if !deferred {
        mark_resync_required();
    }
}

pub(super) fn observe_completed(tick_ms: u32) {
    // SAFETY: decoded server events run on the client main thread.
    let completed = unsafe { COORDINATOR.with(Coordinator::observe_completed) };
    let Some(resync_id) = completed else {
        return;
    };
    COMPLETION_COUNT.fetch_add(1, Ordering::Relaxed);
    complete_transaction(resync_id, tick_ms);
}

fn complete_transaction(resync_id: u32, tick_ms: u32) {
    // SAFETY: refresh completion runs on the client main thread, which is the
    // sole owner of the object cache.
    unsafe {
        OBJECTS.complete_reconciliation(|update| {
            crate::player::removed(update.object_id());
            push_event(QueuedStateUpdate::Object(update), tick_ms);
        });
    }
    push_event(
        QueuedStateUpdate::Action(ActionUpdate::ResyncCompleted { resync_id }),
        tick_ms,
    );
    if take_deferred_snapshot() {
        mark_resync_required_after_events();
    }
}

fn take_deferred_snapshot() -> bool {
    // SAFETY: refresh completion runs on the client main thread.
    unsafe { COORDINATOR.with(Coordinator::take_deferred_snapshot) }
}

#[cfg(windows)]
pub(crate) fn health() -> ResyncHealth {
    ResyncHealth {
        submission_count: SUBMISSION_COUNT.load(Ordering::Acquire),
        completion_count: COMPLETION_COUNT.load(Ordering::Acquire),
        fallback_count: FALLBACK_COUNT.load(Ordering::Acquire),
        submission_failure_count: SUBMISSION_FAILURE_COUNT.load(Ordering::Acquire),
    }
}

pub(crate) fn reset() {
    PHYSICAL_REFRESH_REQUESTED.store(false, Ordering::Release);
    SUBMISSION_COUNT.store(0, Ordering::Release);
    COMPLETION_COUNT.store(0, Ordering::Release);
    FALLBACK_COUNT.store(0, Ordering::Release);
    SUBMISSION_FAILURE_COUNT.store(0, Ordering::Release);
    // SAFETY: lifecycle reset runs while the producer hooks are absent.
    unsafe {
        COORDINATOR.with(Coordinator::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::{Coordinator, MovementTransition, Origin, Phase};
    use darpc_model::{ActionUpdate, StateUpdate, TilePosition};
    use darpc_protocol::EventPollResult;
    use std::{sync::Mutex, time::Duration};

    const COMMITTED: TilePosition = TilePosition { x: 10, y: 20 };
    const STAGED: TilePosition = TilePosition { x: 11, y: 20 };
    static INTERFACE_TEST: Mutex<()> = Mutex::new(());

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
    fn active_refresh_stays_single_until_it_completes() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(1), transition(COMMITTED, COMMITTED, false));
        assert!(!coordinator.is_idle());

        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(1))
        );
        assert!(coordinator.observe_outgoing(1, 100));
        assert!(!coordinator.is_idle());
    }

    #[test]
    fn response_activity_and_refresh_ok_complete_the_refresh() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(12), transition(COMMITTED, COMMITTED, false));
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(12))
        );
        assert!(coordinator.observe_outgoing(12, 100));

        assert_eq!(coordinator.observe_completed(), None);
        assert!(!coordinator.is_idle());
        coordinator.observe_response_activity();
        assert_eq!(coordinator.observe_completed(), Some(12));
        assert!(coordinator.is_idle());
    }

    #[test]
    fn refresh_defers_snapshot_recapture_until_completion() {
        let mut coordinator = Coordinator::new();
        assert!(!coordinator.defer_snapshot());

        coordinator.begin(Origin::Command(12), transition(COMMITTED, COMMITTED, false));
        assert!(coordinator.defer_snapshot());
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(12))
        );
        assert!(coordinator.observe_outgoing(12, 100));
        coordinator.observe_response_activity();
        assert_eq!(coordinator.observe_completed(), Some(12));
        assert!(coordinator.take_deferred_snapshot());
        assert!(!coordinator.take_deferred_snapshot());
    }

    #[test]
    fn an_uncoordinated_outgoing_refresh_starts_a_transaction() {
        let mut coordinator = Coordinator::new();

        assert!(coordinator.observe_outgoing(21, 100));
        coordinator.observe_response_activity();
        assert_eq!(coordinator.observe_completed(), Some(21));
        assert!(coordinator.is_idle());
    }

    #[test]
    fn refresh_falls_back_after_one_second() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Command(12), transition(COMMITTED, COMMITTED, false));
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Command(12))
        );
        assert!(coordinator.observe_outgoing(12, 100));

        assert_eq!(coordinator.observe_timeout(1_099), None);
        assert_eq!(coordinator.observe_timeout(1_100), Some(12));
        assert!(coordinator.is_idle());
    }

    #[test]
    fn failed_submission_releases_the_coordinator() {
        let mut coordinator = Coordinator::new();
        coordinator.begin(Origin::Physical, transition(COMMITTED, COMMITTED, false));
        assert!(coordinator.defer_snapshot());
        assert_eq!(
            coordinator.observe_transition(transition(COMMITTED, COMMITTED, false)),
            Some(Origin::Physical)
        );

        assert!(coordinator.submission_failed(Origin::Physical));
        assert!(coordinator.is_idle());
        assert!(coordinator.take_deferred_snapshot());
    }

    #[test]
    fn interface_orders_completion_before_deferred_snapshot_recovery() {
        let _guard = INTERFACE_TEST.lock().unwrap();
        super::super::reset();
        super::reset();

        super::observe_outgoing(12, 100);
        super::snapshot_required();
        super::observe_authoritative_activity(101);
        super::observe_completed(102);

        let EventPollResult::Events(events) = super::super::poll(0, 16, Duration::ZERO) else {
            panic!("refresh events must precede snapshot recovery");
        };
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].update,
            StateUpdate::Action(ActionUpdate::Resync { resync_id: 12 })
        );
        assert_eq!(
            events[1].update,
            StateUpdate::Action(ActionUpdate::ResyncCompleted { resync_id: 12 })
        );
        assert_eq!(
            super::super::poll(2, 16, Duration::ZERO),
            EventPollResult::ResyncRequired {
                missing_sequence: 3,
                latest_sequence: 3,
            }
        );

        super::super::reset();
        super::reset();
    }
}
