#[cfg(windows)]
use crate::management::{LifecycleOutcome, ManagementError};
use crate::registry::ConnectionEvent;

pub(crate) enum DaemonEvent {
    #[cfg(windows)]
    Connection(ConnectionEvent),
    #[cfg(windows)]
    AutoLoadFinished {
        pid: u32,
        attempt: u64,
        result: Result<LifecycleOutcome, ManagementError>,
    },
    Status(ConnectionEvent),
    Track(u32),
}
