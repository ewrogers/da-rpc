#[cfg(windows)]
use crate::lifecycle::{LifecycleOutcome, ManagementError};
use crate::registry::ConnectionEvent;

pub(crate) enum DaemonEvent {
    #[cfg(windows)]
    Connection(ConnectionEvent),
    #[cfg(windows)]
    HookBudgetExceeded {
        pid: u32,
        timing: darpc_protocol::HookTimingRecord,
        delta: u64,
    },
    #[cfg(windows)]
    TickRateChanged {
        pid: u32,
        degraded: bool,
        tick_delta: u32,
        sample_ms: u64,
        rate_hz: u32,
        threshold_hz: u32,
    },
    #[cfg(windows)]
    AutoLoadFinished {
        pid: u32,
        attempt: u64,
        result: Result<LifecycleOutcome, ManagementError>,
    },
    Status(ConnectionEvent),
    Track(u32),
    CommandsReady,
    ManagedShutdown(std::io::Result<()>),
}
