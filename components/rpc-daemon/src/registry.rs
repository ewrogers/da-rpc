use darpc_protocol::{Architecture, Hello, protocol_version_major, protocol_version_minor};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ClientIdentity {
    pub(crate) pid: u32,
    pub(crate) process_creation_time: u64,
    pub(crate) dll_instance_id: [u8; 16],
}

impl ClientIdentity {
    #[must_use]
    pub(crate) const fn from_hello(hello: Hello) -> Self {
        Self {
            pid: hello.process_id,
            process_creation_time: hello.process_creation_time,
            dll_instance_id: hello.dll_instance_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionEvent {
    Connecting {
        pid: u32,
    },
    Connected {
        pid: u32,
        hello: Hello,
        selected_version: u16,
    },
    Busy {
        pid: u32,
    },
    Disconnected {
        pid: u32,
        identity: Option<ClientIdentity>,
        reason: String,
    },
    Incompatible {
        pid: u32,
        identity: Option<ClientIdentity>,
        reason: String,
    },
}

impl ConnectionEvent {
    #[must_use]
    pub(crate) const fn pid(&self) -> u32 {
        match self {
            Self::Connecting { pid }
            | Self::Connected { pid, .. }
            | Self::Busy { pid }
            | Self::Disconnected { pid, .. }
            | Self::Incompatible { pid, .. } => *pid,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TargetStatus {
    Connecting,
    Connected(ClientIdentity),
    Busy,
    Disconnected {
        identity: Option<ClientIdentity>,
        reason: String,
    },
    Incompatible {
        identity: Option<ClientIdentity>,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientRecord {
    hello: Hello,
    selected_version: u16,
    connected: bool,
}

#[derive(Default)]
pub(crate) struct Registry {
    targets: BTreeMap<u32, TargetStatus>,
    clients: BTreeMap<ClientIdentity, ClientRecord>,
}

impl Registry {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            targets: BTreeMap::new(),
            clients: BTreeMap::new(),
        }
    }

    pub(crate) fn apply(&mut self, event: &ConnectionEvent) -> bool {
        let pid = event.pid();
        let next = match event {
            ConnectionEvent::Connecting { .. } => TargetStatus::Connecting,
            ConnectionEvent::Busy { .. } => TargetStatus::Busy,
            ConnectionEvent::Connected {
                hello,
                selected_version,
                ..
            } => {
                let identity = ClientIdentity::from_hello(*hello);
                self.clients.retain(|existing, _| existing.pid != pid);
                self.clients.insert(
                    identity,
                    ClientRecord {
                        hello: *hello,
                        selected_version: *selected_version,
                        connected: true,
                    },
                );
                TargetStatus::Connected(identity)
            }
            ConnectionEvent::Disconnected {
                identity, reason, ..
            } => {
                if let Some(identity) = identity {
                    if let Some(client) = self.clients.get_mut(identity) {
                        client.connected = false;
                    }
                    if matches!(
                        self.targets.get(&pid),
                        Some(TargetStatus::Connected(current)) if current != identity
                    ) {
                        return false;
                    }
                }
                TargetStatus::Disconnected {
                    identity: *identity,
                    reason: reason.clone(),
                }
            }
            ConnectionEvent::Incompatible {
                identity, reason, ..
            } => TargetStatus::Incompatible {
                identity: *identity,
                reason: reason.clone(),
            },
        };

        if self.targets.get(&pid) == Some(&next) {
            return false;
        }
        self.targets.insert(pid, next);
        true
    }
}

pub(crate) fn render_event(event: &ConnectionEvent) -> String {
    match event {
        ConnectionEvent::Connecting { pid } => format!("client pid={pid} status=connecting"),
        ConnectionEvent::Busy { pid } => format!("client pid={pid} status=busy"),
        ConnectionEvent::Connected {
            pid,
            hello,
            selected_version,
        } => format!(
            concat!(
                "client pid={} status=connected creation_time={} instance={} protocol={}.{} ",
                "architecture={} dll_version={}.{}.{} fingerprint={} layout_id={}"
            ),
            pid,
            hello.process_creation_time,
            hex(&hello.dll_instance_id),
            protocol_version_major(*selected_version),
            protocol_version_minor(*selected_version),
            architecture(hello.architecture),
            hello.dll_version.major,
            hello.dll_version.minor,
            hello.dll_version.patch,
            hex(&hello.executable_fingerprint),
            hello.layout_id,
        ),
        ConnectionEvent::Disconnected {
            pid,
            identity,
            reason,
        } => format!(
            "client pid={pid} status=disconnected instance={} reason={reason:?}",
            optional_instance(*identity)
        ),
        ConnectionEvent::Incompatible {
            pid,
            identity,
            reason,
        } => format!(
            "client pid={pid} status=incompatible instance={} reason={reason:?}",
            optional_instance(*identity)
        ),
    }
}

fn optional_instance(identity: Option<ClientIdentity>) -> String {
    identity.map_or_else(
        || "unknown".into(),
        |identity| hex(&identity.dll_instance_id),
    )
}

fn architecture(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X86 => "x86",
        Architecture::X86_64 => "x86_64",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02X}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{ClientIdentity, ConnectionEvent, Registry, TargetStatus};
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};

    fn hello(instance: u8, creation_time: u64) -> Hello {
        Hello {
            protocol_versions: SUPPORTED_VERSIONS,
            dll_instance_id: [instance; 16],
            process_id: 42,
            process_creation_time: creation_time,
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            executable_fingerprint: [0xCD; 32],
            layout_id: 741,
        }
    }

    #[test]
    fn replaces_changed_process_and_dll_identity() {
        let mut registry = Registry::new();
        let first = hello(1, 100);
        let second = hello(2, 200);
        assert!(registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: first,
            selected_version: SUPPORTED_VERSIONS.max,
        }));
        assert!(registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: second,
            selected_version: SUPPORTED_VERSIONS.max,
        }));

        assert_eq!(registry.clients.len(), 1);
        assert!(
            !registry
                .clients
                .contains_key(&ClientIdentity::from_hello(first))
        );
        assert!(
            registry
                .clients
                .contains_key(&ClientIdentity::from_hello(second))
        );
    }

    #[test]
    fn ignores_a_stale_disconnect_after_replacement() {
        let mut registry = Registry::new();
        let first = hello(1, 100);
        let second = hello(2, 200);
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: first,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: second,
            selected_version: SUPPORTED_VERSIONS.max,
        });

        assert!(!registry.apply(&ConnectionEvent::Disconnected {
            pid: 42,
            identity: Some(ClientIdentity::from_hello(first)),
            reason: "stale".into(),
        }));
        assert_eq!(
            registry.targets.get(&42),
            Some(&TargetStatus::Connected(ClientIdentity::from_hello(second)))
        );
    }

    #[test]
    fn keeps_incompatible_identity_visible_but_unregistered() {
        let mut registry = Registry::new();
        let identity = ClientIdentity::from_hello(hello(3, 300));
        assert!(registry.apply(&ConnectionEvent::Incompatible {
            pid: 42,
            identity: Some(identity),
            reason: "unsupported protocol".into(),
        }));
        assert!(registry.clients.is_empty());
        assert!(matches!(
            registry.targets.get(&42),
            Some(TargetStatus::Incompatible {
                identity: Some(actual),
                ..
            }) if *actual == identity
        ));
    }
}
