use crate::wrapping_time::deadline_reached;

pub(super) const POLL_INTERVAL_MS: u32 = 25;
const START_WINDOW_MS: u32 = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CooldownWatch {
    Idle,
    Pending { next_poll_ms: u32, deadline_ms: u32 },
    Active { next_poll_ms: u32 },
}

impl CooldownWatch {
    pub(super) const fn start(now_ms: u32, settle_ms: u32) -> Self {
        Self::Pending {
            next_poll_ms: now_ms.wrapping_add(settle_ms),
            deadline_ms: now_ms.wrapping_add(START_WINDOW_MS),
        }
    }

    pub(super) const fn due(self, now_ms: u32) -> bool {
        match self {
            Self::Idle => false,
            Self::Pending { next_poll_ms, .. } | Self::Active { next_poll_ms } => {
                deadline_reached(now_ms, next_poll_ms)
            }
        }
    }

    pub(super) const fn observed(
        self,
        active: bool,
        exact_end_ms: Option<u32>,
        now_ms: u32,
    ) -> Self {
        if active {
            let next_poll_ms = match exact_end_ms {
                Some(end_ms) if !deadline_reached(now_ms, end_ms) => end_ms,
                _ => now_ms.wrapping_add(POLL_INTERVAL_MS),
            };
            return Self::Active { next_poll_ms };
        }

        match self {
            Self::Pending { deadline_ms, .. } if !deadline_reached(now_ms, deadline_ms) => {
                Self::Pending {
                    next_poll_ms: now_ms.wrapping_add(POLL_INTERVAL_MS),
                    deadline_ms,
                }
            }
            _ => Self::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_watch_polls_until_activation_or_timeout() {
        let watch = CooldownWatch::start(100, 5);
        assert!(!watch.due(104));
        assert!(watch.due(105));

        let watch = watch.observed(false, None, 105);
        assert!(!watch.due(129));
        assert!(watch.due(130));
        assert_eq!(watch.observed(false, None, 600), CooldownWatch::Idle);
    }

    #[test]
    fn exact_expiry_is_polled_once_at_its_deadline() {
        let watch = CooldownWatch::start(100, 5).observed(true, Some(1_000), 105);
        assert!(!watch.due(999));
        assert!(watch.due(1_000));
        assert_eq!(watch.observed(false, None, 1_000), CooldownWatch::Idle);
    }

    #[test]
    fn unknown_expiry_uses_bounded_polling_and_wraps() {
        let watch = CooldownWatch::start(u32::MAX - 10, 5).observed(true, None, u32::MAX - 5);
        assert!(!watch.due(18));
        assert!(watch.due(19));
    }
}
