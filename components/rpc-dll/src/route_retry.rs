const TIMEOUT_MS: u32 = 5_000;
const STALL_TIMEOUT_MS: u32 = 1_200;
const DELAYS_MS: [u32; 5] = [250, 350, 500, 750, 1_000];

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

#[cfg_attr(test, allow(dead_code))]
pub(crate) const fn tick_reached(now: u32, target: u32) -> bool {
    crate::wrapping_time::deadline_reached(now, target)
}

pub(crate) fn stalled(now: u32, last_progress_tick: u32) -> bool {
    crate::wrapping_time::deadline_reached(now, last_progress_tick.wrapping_add(STALL_TIMEOUT_MS))
}

#[cfg(test)]
mod tests {
    use super::{deadline, deadline_after_progress, delay_ms, stalled};

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
        assert!(!stalled(1_199, 0));
        assert!(stalled(1_200, 0));
        assert!(stalled(500, u32::MAX - 699));
        assert_eq!(deadline_after_progress(true, 100), Some(5_100));
        assert_eq!(deadline_after_progress(false, 100), None);
    }
}
