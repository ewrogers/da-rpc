//! Daemon diagnostics. Keep routine observations at DEBUG and log accepted
//! transitions only after the registry has committed them.

use tracing_subscriber::EnvFilter;

pub(crate) fn init() -> Result<(), String> {
    let filter = EnvFilter::builder()
        .with_regex(false)
        .with_default_directive(tracing::Level::INFO.into())
        .from_env()
        .map_err(|error| format!("invalid RUST_LOG filter: {error}"))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| format!("failed to initialize logging: {error}"))
}

#[cfg(any(windows, test))]
pub(crate) fn committed(change: &crate::registry::CommittedChange) {
    use crate::registry::CommittedChange;
    match change {
        CommittedChange::Connection(event) => connection(event),
        CommittedChange::Snapshot {
            pid,
            previous,
            current,
            ..
        } => {
            if previous.is_none() {
                tracing::info!(pid, revision = current.revision, lifecycle = ?current.lifecycle,
                    "client baseline ready");
            } else if let Some(previous) = previous
                && previous.lifecycle != current.lifecycle
            {
                tracing::info!(pid, previous = ?previous.lifecycle, current = ?current.lifecycle,
                    "client lifecycle changed");
            }
            tracing::debug!(pid, revision = current.revision, event_sequence = current.event_sequence,
                lifecycle = ?current.lifecycle, duration_us = current.capture_duration_us,
                "snapshot committed");
        }
        CommittedChange::StateEvents {
            pid,
            events,
            current,
            ..
        } => {
            for event in events {
                if let darpc_model::StateUpdate::Lifecycle(update) = &event.event.update {
                    tracing::info!(pid, previous = ?update.previous, current = ?update.current,
                        "client lifecycle changed");
                }
            }
            tracing::debug!(
                pid,
                count = events.len(),
                first_sequence = events.first().map_or(0, |event| event.event.sequence),
                last_sequence = current.event_sequence,
                revision = current.revision,
                "state events committed"
            );
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) fn connection(event: &crate::registry::ConnectionEvent) {
    use crate::registry::{ConnectionEvent, hex};
    match event {
        ConnectionEvent::Connecting { pid } => tracing::debug!(pid, "client connecting"),
        ConnectionEvent::Initializing { pid } => tracing::info!(pid, "client initializing"),
        ConnectionEvent::NotLoaded { pid } => tracing::info!(pid, "client not loaded"),
        ConnectionEvent::Busy { pid } => tracing::debug!(pid, "client pipe busy"),
        ConnectionEvent::Connected {
            pid,
            hello,
            selected_version,
        } => {
            tracing::info!(pid, instance = %hex(&hello.dll_instance_id),
                protocol_major = darpc_protocol::protocol_version_major(*selected_version),
                protocol_minor = darpc_protocol::protocol_version_minor(*selected_version),
                "client connected");
            tracing::debug!(pid, creation_time = hello.process_creation_time,
                architecture = ?hello.architecture, dll_version = ?hello.dll_version,
                fingerprint = %hex(&hello.executable_fingerprint), client_version = hello.client_version,
                "client handshake accepted");
        }
        ConnectionEvent::SnapshotUnavailable { pid, reason, .. } => {
            tracing::warn!(pid, reason, "client snapshot unavailable");
        }
        ConnectionEvent::Disconnected { pid, reason, .. } => {
            tracing::info!(pid, reason, "client disconnected");
        }
        ConnectionEvent::Incompatible { pid, reason, .. } => {
            tracing::warn!(pid, reason, "client incompatible");
        }
        ConnectionEvent::Snapshot { .. } | ConnectionEvent::StateEvents { .. } => {
            // Observation logging requires the accepted before/after state.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{committed, connection};
    use crate::registry::{ClientIdentity, CommittedChange, CommittedStateEvent, ConnectionEvent};
    use darpc_model::{ClientLifecycle, ClientSnapshot, LifecycleUpdate, StateEvent, StateUpdate};
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn capture(filter: &str, action: impl FnOnce()) -> String {
        let output = Output::default();
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, action);
        String::from_utf8(output.0.lock().unwrap().clone()).unwrap()
    }

    fn identity() -> ClientIdentity {
        ClientIdentity {
            pid: 42,
            process_creation_time: 1,
            dll_instance_id: [1; 16],
        }
    }

    fn snapshot(revision: u32, lifecycle: ClientLifecycle) -> Arc<ClientSnapshot> {
        Arc::new(ClientSnapshot {
            revision,
            event_sequence: 0,
            captured_tick_ms: 1,
            updated_tick_ms: 1,
            capture_duration_us: 100,
            world_generation: 1,
            lifecycle,
            character: None,
            objects: None,
            dialog: None,
            message_dialogs: Default::default(),
            active_field_map: None,
            active_bulletin: None,
            group: None,
            exchange: None,
            legend: None,
            planned_route: None,
        })
    }

    fn captures() {
        let first = snapshot(1, ClientLifecycle::InGame);
        committed(&CommittedChange::Snapshot {
            pid: 42,
            identity: identity(),
            previous: None,
            current: Arc::clone(&first),
        });
        committed(&CommittedChange::Snapshot {
            pid: 42,
            identity: identity(),
            previous: Some(first),
            current: snapshot(2, ClientLifecycle::InGame),
        });
    }

    #[test]
    fn connection_logs_keep_instance_identity_and_hide_handshake_details() {
        let output = capture("info", || {
            connection(&ConnectionEvent::Connected {
                pid: 42,
                hello: darpc_protocol::Hello {
                    protocol_versions: darpc_protocol::SUPPORTED_VERSIONS,
                    dll_instance_id: [1; 16],
                    process_id: 42,
                    process_creation_time: 1,
                    architecture: darpc_protocol::Architecture::X86,
                    dll_version: darpc_protocol::ComponentVersion {
                        major: 1,
                        minor: 0,
                        patch: 0,
                    },
                    executable_fingerprint: [0; 32],
                    client_version: 741,
                },
                selected_version: darpc_protocol::SUPPORTED_VERSIONS.max,
            });
        });
        assert!(
            output.contains("client connected pid=42 instance=01010101010101010101010101010101"),
            "{output}"
        );
        assert_eq!(output.lines().count(), 1, "{output}");
        assert!(!output.contains("fingerprint"));
    }

    #[test]
    fn info_keeps_the_first_baseline_and_hides_routine_captures() {
        let output = capture("info", captures);
        assert_eq!(output.lines().count(), 1, "{output}");
        assert!(
            output.contains("INFO darpcd::logging: client baseline ready pid=42"),
            "{output}"
        );
        assert!(!output.contains("snapshot committed"));
    }

    #[test]
    fn debug_exposes_capture_metadata_and_respects_module_filters() {
        let output = capture("info,darpcd::logging=debug", captures);
        assert_eq!(output.matches("snapshot committed").count(), 2, "{output}");
        assert!(output.contains("revision=2 event_sequence=0 lifecycle=InGame duration_us=100"));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn lifecycle_and_recovery_warnings_remain_visible_at_info() {
        let output = capture("info", || {
            committed(&CommittedChange::Snapshot {
                pid: 42,
                identity: identity(),
                previous: Some(snapshot(1, ClientLifecycle::Title)),
                current: snapshot(2, ClientLifecycle::InGame),
            });
            committed(&CommittedChange::StateEvents {
                pid: 42,
                identity: identity(),
                events: vec![CommittedStateEvent {
                    event: StateEvent {
                        sequence: 1,
                        revision: 3,
                        tick_ms: 2,
                        update: StateUpdate::Lifecycle(LifecycleUpdate {
                            previous: ClientLifecycle::InGame,
                            current: ClientLifecycle::Disconnected,
                        }),
                    },
                    replaced_players: Vec::new(),
                }],
                current: snapshot(3, ClientLifecycle::Disconnected),
            });
            connection(&ConnectionEvent::SnapshotUnavailable {
                pid: 42,
                identity: identity(),
                reason: "capture timed out".into(),
            });
        });
        assert_eq!(
            output.matches("client lifecycle changed").count(),
            2,
            "{output}"
        );
        assert!(
            output.contains("WARN darpcd::logging: client snapshot unavailable pid=42"),
            "{output}"
        );
        assert!(!output.contains("state events committed"));
    }
}
