const TIMEOUT_MS: u32 = 5_000;
const DELAYS_MS: [u32; 5] = [250, 350, 500, 750, 1_000];

pub(crate) fn deadline(start_tick: u32) -> u32 {
    start_tick.wrapping_add(TIMEOUT_MS)
}

pub(crate) fn delay_ms(attempt: u32) -> u32 {
    usize::try_from(attempt)
        .ok()
        .and_then(|index| DELAYS_MS.get(index))
        .copied()
        .unwrap_or(DELAYS_MS[DELAYS_MS.len() - 1])
}

pub(crate) fn tick_reached(now: u32, target: u32) -> bool {
    now.wrapping_sub(target) < (1 << 31)
}

#[cfg(test)]
mod tests {
    use super::{deadline, delay_ms, tick_reached};

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
        let target = u32::MAX - 100;
        assert!(!tick_reached(target.wrapping_sub(1), target));
        assert!(tick_reached(target, target));
        assert!(tick_reached(target.wrapping_add(200), target));

        let wrapped_target = 100;
        assert!(!tick_reached(u32::MAX - 100, wrapped_target));
        assert!(tick_reached(wrapped_target, wrapped_target));
        assert_eq!(deadline(u32::MAX - 2_000), 2_999);
    }
}
