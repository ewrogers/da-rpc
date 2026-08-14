const TIMEOUT_MS: u32 = 5_000;
const STALL_TIMEOUT_MS: u32 = 1_000;
const STEP_RETRY_DELAY_MS: u32 = 1_000;
const OBSERVATION_INTERVAL_MS: u32 = 50;
const DELAYS_MS: [u32; 5] = [250, 350, 500, 750, 1_000];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StepFailureAction {
    Retry { failures: u32, due_tick: u32 },
    Replan,
}

pub(crate) fn deadline(start_tick: u32) -> u32 {
    start_tick.wrapping_add(TIMEOUT_MS)
}

pub(crate) fn deadline_after_progress(replan_pending: bool, tick: u32) -> Option<u32> {
    replan_pending.then(|| deadline(tick))
}

pub(crate) fn delay_ms(attempt: u32) -> u32 {
    usize::try_from(attempt)
        .ok()
        .and_then(|index| DELAYS_MS.get(index))
        .copied()
        .unwrap_or(DELAYS_MS[DELAYS_MS.len() - 1])
}

pub(crate) fn stalled(now: u32, last_progress_tick: u32) -> bool {
    crate::wrapping_time::deadline_reached(now, last_progress_tick.wrapping_add(STALL_TIMEOUT_MS))
}

pub(crate) fn step_retry_due_tick(tick: u32) -> u32 {
    tick.wrapping_add(STEP_RETRY_DELAY_MS)
}

pub(crate) fn observation_due_tick(tick: u32) -> u32 {
    tick.wrapping_add(OBSERVATION_INTERVAL_MS)
}

pub(crate) fn after_step_failure(failures: u32, tick: u32) -> StepFailureAction {
    if failures >= 2 {
        StepFailureAction::Replan
    } else {
        StepFailureAction::Retry {
            failures: failures + 1,
            due_tick: step_retry_due_tick(tick),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        StepFailureAction, after_step_failure, deadline, deadline_after_progress, delay_ms,
        observation_due_tick, stalled, step_retry_due_tick,
    };

    #[test]
    fn delay_increases_and_caps() {
        assert_eq!(delay_ms(0), 250);
        assert_eq!(delay_ms(1), 350);
        assert_eq!(delay_ms(2), 500);
        assert_eq!(delay_ms(3), 750);
        assert_eq!(delay_ms(4), 1_000);
        assert_eq!(delay_ms(20), 1_000);
    }

    #[test]
    fn tick_deadlines_handle_counter_wraparound() {
        assert_eq!(deadline(u32::MAX - 2_000), 2_999);
        assert!(!stalled(999, 0));
        assert!(stalled(1_000, 0));
        assert!(stalled(500, u32::MAX - 499));
        assert_eq!(step_retry_due_tick(u32::MAX - 499), 500);
        assert_eq!(observation_due_tick(u32::MAX - 24), 25);
        assert_eq!(deadline_after_progress(true, 100), Some(5_100));
        assert_eq!(deadline_after_progress(false, 100), None);
    }

    #[test]
    fn retries_a_step_twice_before_replanning() {
        assert_eq!(
            after_step_failure(0, 0),
            StepFailureAction::Retry {
                failures: 1,
                due_tick: 1_000,
            }
        );
        assert_eq!(
            after_step_failure(1, 1_000),
            StepFailureAction::Retry {
                failures: 2,
                due_tick: 2_000,
            }
        );
        assert_eq!(after_step_failure(2, 2_000), StepFailureAction::Replan);
    }
}
