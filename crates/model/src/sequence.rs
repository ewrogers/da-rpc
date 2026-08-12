/// A wrapping 32-bit sequence whose zero value is reserved as an unset boundary.
///
/// Successors skip zero, and ordering is defined within half of the sequence
/// space so comparisons remain valid across `u32` wraparound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceNumber(u32);

impl SequenceNumber {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn next(self) -> Self {
        let next = self.0.wrapping_add(1);
        Self(if next == 0 { 1 } else { next })
    }

    pub const fn is_after(self, baseline: Self) -> bool {
        let distance = self.0.wrapping_sub(baseline.0);
        distance != 0 && distance < 0x8000_0000
    }
}

impl From<u32> for SequenceNumber {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<SequenceNumber> for u32 {
    fn from(value: SequenceNumber) -> Self {
        value.get()
    }
}

#[cfg(test)]
mod tests {
    use super::SequenceNumber;

    #[test]
    fn successor_skips_zero_at_wraparound() {
        assert_eq!(SequenceNumber::new(0).next().get(), 1);
        assert_eq!(SequenceNumber::new(u32::MAX).next().get(), 1);
    }

    #[test]
    fn half_range_ordering_handles_wraparound() {
        assert!(SequenceNumber::new(1).is_after(SequenceNumber::new(u32::MAX)));
        assert!(!SequenceNumber::new(u32::MAX).is_after(SequenceNumber::new(1)));
        assert!(!SequenceNumber::new(7).is_after(SequenceNumber::new(7)));
    }
}
