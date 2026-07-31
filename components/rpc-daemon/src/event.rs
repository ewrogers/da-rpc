use crate::registry::ConnectionEvent;

pub(crate) enum DaemonEvent {
    #[cfg(windows)]
    Connection(ConnectionEvent),
    Status(ConnectionEvent),
    Track(u32),
}
