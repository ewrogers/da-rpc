/// Returns whether `now` has reached `deadline` within the unambiguous half of
/// the wrapping 32-bit tick range.
pub(crate) const fn deadline_reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::deadline_reached;

    #[test]
    fn compares_deadlines_across_wraparound() {
        let deadline = u32::MAX - 100;
        assert!(!deadline_reached(deadline.wrapping_sub(1), deadline));
        assert!(deadline_reached(deadline, deadline));
        assert!(deadline_reached(deadline.wrapping_add(200), deadline));

        let wrapped_deadline = 100;
        assert!(!deadline_reached(u32::MAX - 100, wrapped_deadline));
        assert!(deadline_reached(wrapped_deadline, wrapped_deadline));
    }
}
