use darpc_model::SequenceNumber;
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) fn next_nonzero(counter: &AtomicU32) -> u32 {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            Some(SequenceNumber::new(value).next().get())
        })
        .map(|previous| SequenceNumber::new(previous).next().get())
        .expect("sequence counter update cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_zero_at_wraparound() {
        let counter = AtomicU32::new(u32::MAX);

        assert_eq!(next_nonzero(&counter), 1);
        assert_eq!(counter.load(Ordering::Acquire), 1);
    }
}
