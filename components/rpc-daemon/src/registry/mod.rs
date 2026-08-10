use darpc_model::{ClientSnapshot as GameSnapshot, StateEvent};
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
    Initializing {
        pid: u32,
    },
    NotLoaded {
        pid: u32,
    },
    Connected {
        pid: u32,
        hello: Hello,
        selected_version: u16,
    },
    Snapshot {
        pid: u32,
        identity: ClientIdentity,
        snapshot: Box<GameSnapshot>,
    },
    SnapshotUnavailable {
        pid: u32,
        identity: ClientIdentity,
        reason: String,
    },
    #[cfg_attr(not(windows), allow(dead_code))]
    StateEvents {
        pid: u32,
        identity: ClientIdentity,
        events: Vec<StateEvent>,
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
            | Self::Initializing { pid }
            | Self::NotLoaded { pid }
            | Self::Connected { pid, .. }
            | Self::Snapshot { pid, .. }
            | Self::SnapshotUnavailable { pid, .. }
            | Self::StateEvents { pid, .. }
            | Self::Busy { pid }
            | Self::Disconnected { pid, .. }
            | Self::Incompatible { pid, .. } => *pid,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TargetStatus {
    Connecting,
    Initializing,
    NotLoaded,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClientRecord {
    hello: Hello,
    selected_version: u16,
    snapshot: Option<GameSnapshot>,
    snapshot_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClientSnapshotStatus {
    Connecting,
    Initializing,
    NotLoaded,
    Connected,
    Busy,
    Disconnected,
    Incompatible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClientSnapshot {
    pub(crate) pid: u32,
    pub(crate) status: ClientSnapshotStatus,
    pub(crate) identity: Option<ClientIdentity>,
    pub(crate) hello: Option<Hello>,
    pub(crate) selected_version: Option<u16>,
    pub(crate) game_snapshot: Option<GameSnapshot>,
    pub(crate) snapshot_reason: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RegistrySnapshot {
    pub(crate) clients: Vec<ClientSnapshot>,
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
        match event {
            ConnectionEvent::Snapshot {
                identity, snapshot, ..
            } => {
                let Some(record) = self.clients.get_mut(identity) else {
                    return false;
                };
                if record.snapshot.as_ref() == Some(snapshot.as_ref())
                    && record.snapshot_reason.is_none()
                {
                    return false;
                }
                record.snapshot = Some(snapshot.as_ref().clone());
                record.snapshot_reason = None;
                return true;
            }
            ConnectionEvent::SnapshotUnavailable {
                identity, reason, ..
            } => {
                let Some(record) = self.clients.get_mut(identity) else {
                    return false;
                };
                if record.snapshot_reason.as_ref() == Some(reason) {
                    return false;
                }
                record.snapshot_reason = Some(reason.clone());
                return true;
            }
            ConnectionEvent::StateEvents {
                identity, events, ..
            } => {
                if events.is_empty() {
                    return false;
                }
                let Some(record) = self.clients.get_mut(identity) else {
                    return false;
                };
                let Some(current) = record.snapshot.as_ref() else {
                    return false;
                };
                if let Err(error) = validate_collection_batches(events) {
                    let reason = format!("event reduction failed: {error}");
                    if record.snapshot_reason.as_ref() == Some(&reason) {
                        return false;
                    }
                    record.snapshot_reason = Some(reason);
                    return true;
                }
                let mut next = current.clone();
                for state_event in events {
                    if let Err(error) = next.apply_event(state_event.clone()) {
                        let reason = format!("event reduction failed: {error}");
                        if record.snapshot_reason.as_ref() == Some(&reason) {
                            return false;
                        }
                        record.snapshot_reason = Some(reason);
                        return true;
                    }
                }
                record.snapshot = Some(next);
                record.snapshot_reason = None;
                return true;
            }
            _ => {}
        }
        let next = match event {
            ConnectionEvent::Connecting { .. } => TargetStatus::Connecting,
            ConnectionEvent::Initializing { .. } => TargetStatus::Initializing,
            ConnectionEvent::NotLoaded { .. } => TargetStatus::NotLoaded,
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
                        snapshot: None,
                        snapshot_reason: None,
                    },
                );
                TargetStatus::Connected(identity)
            }
            ConnectionEvent::Disconnected {
                identity, reason, ..
            } => {
                if let Some(identity) = identity
                    && matches!(
                        self.targets.get(&pid),
                        Some(TargetStatus::Connected(current)) if current != identity
                    )
                {
                    return false;
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
            ConnectionEvent::Snapshot { .. }
            | ConnectionEvent::SnapshotUnavailable { .. }
            | ConnectionEvent::StateEvents { .. } => {
                unreachable!("snapshot events return before target status reconciliation")
            }
        };

        if self.targets.get(&pid) == Some(&next) {
            return false;
        }
        self.targets.insert(pid, next);
        true
    }

    pub(crate) fn remove(&mut self, pid: u32) -> bool {
        let removed_target = self.targets.remove(&pid).is_some();
        let original_clients = self.clients.len();
        self.clients.retain(|identity, _| identity.pid != pid);
        removed_target || self.clients.len() != original_clients
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> RegistrySnapshot {
        let clients = self
            .targets
            .iter()
            .map(|(&pid, target)| {
                let (status, identity, reason) = match target {
                    TargetStatus::Connecting => (ClientSnapshotStatus::Connecting, None, None),
                    TargetStatus::Initializing => (ClientSnapshotStatus::Initializing, None, None),
                    TargetStatus::NotLoaded => (ClientSnapshotStatus::NotLoaded, None, None),
                    TargetStatus::Connected(identity) => {
                        (ClientSnapshotStatus::Connected, Some(*identity), None)
                    }
                    TargetStatus::Busy => (ClientSnapshotStatus::Busy, None, None),
                    TargetStatus::Disconnected { identity, reason } => (
                        ClientSnapshotStatus::Disconnected,
                        *identity,
                        Some(reason.clone()),
                    ),
                    TargetStatus::Incompatible { identity, reason } => (
                        ClientSnapshotStatus::Incompatible,
                        *identity,
                        Some(reason.clone()),
                    ),
                };
                let record = identity.and_then(|identity| self.clients.get(&identity));
                ClientSnapshot {
                    pid,
                    status,
                    identity,
                    hello: record.map(|record| record.hello),
                    selected_version: record.map(|record| record.selected_version),
                    game_snapshot: record.and_then(|record| record.snapshot.clone()),
                    snapshot_reason: record.and_then(|record| record.snapshot_reason.clone()),
                    reason,
                }
            })
            .collect();
        RegistrySnapshot { clients }
    }
}

fn validate_collection_batches(events: &[StateEvent]) -> Result<(), String> {
    let mut position = 0;
    while position < events.len() {
        let Some((kind, batch)) = events[position].update.collection_batch() else {
            position += 1;
            continue;
        };
        if batch.index != 0 || batch.count == 0 {
            return Err(format!(
                "{} batch begins at {} of {}",
                kind.as_str(),
                batch.index,
                batch.count
            ));
        }
        let count = usize::from(batch.count);
        let end = position.saturating_add(count);
        if end > events.len() {
            return Err(format!(
                "{} batch contains {} of {} events",
                kind.as_str(),
                events.len() - position,
                batch.count
            ));
        }
        for (index, event) in events[position..end].iter().enumerate() {
            let expected = Some((
                kind,
                darpc_model::CollectionBatch {
                    index: u8::try_from(index).expect("collection batch index fits u8"),
                    count: batch.count,
                },
            ));
            if event.update.collection_batch() != expected {
                return Err(format!(
                    "{} batch is interrupted at event {index}",
                    kind.as_str()
                ));
            }
        }
        position = end;
    }
    Ok(())
}

pub(crate) fn render_event(event: &ConnectionEvent) -> String {
    match event {
        ConnectionEvent::Connecting { pid } => format!("client pid={pid} status=connecting"),
        ConnectionEvent::Initializing { pid } => {
            format!("client pid={pid} status=initializing")
        }
        ConnectionEvent::NotLoaded { pid } => format!("client pid={pid} status=not_loaded"),
        ConnectionEvent::Busy { pid } => format!("client pid={pid} status=busy"),
        ConnectionEvent::Connected {
            pid,
            hello,
            selected_version,
        } => format!(
            concat!(
                "client pid={} status=connected creation_time={} instance={} protocol={}.{} ",
                "architecture={} dll_version={}.{}.{} fingerprint={} client_version={}"
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
            hello.client_version,
        ),
        ConnectionEvent::Snapshot { pid, snapshot, .. } => format!(
            "client pid={pid} snapshot=ready revision={} lifecycle={:?} duration_us={}",
            snapshot.revision, snapshot.lifecycle, snapshot.capture_duration_us
        ),
        ConnectionEvent::SnapshotUnavailable { pid, reason, .. } => {
            format!("client pid={pid} snapshot=unavailable reason={reason:?}")
        }
        ConnectionEvent::StateEvents { pid, events, .. } => {
            let first = events.first().map_or(0, |event| event.sequence);
            let last = events.last().map_or(0, |event| event.sequence);
            format!(
                "client pid={pid} events={} sequence={first}..={last}",
                events.len()
            )
        }
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

pub(crate) fn architecture(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X86 => "x86",
        Architecture::X86_64 => "x86_64",
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        ClientIdentity, ClientSnapshotStatus, ConnectionEvent, Registry, TargetStatus, hex,
        render_event, validate_collection_batches,
    };
    use darpc_model::{
        CharacterClass, CharacterProgression, CharacterSnapshot, CharacterStats, CharacterVitals,
        ClientLifecycle, ClientSnapshot as GameSnapshot, CollectionChange, CurrentVitals, Effect,
        EffectDuration, EffectUpdate, InventoryItem, LocationUpdate, MapChange, MovementUpdate,
        SlotUpdate, StateEvent, StateUpdate, StatusUpdate, TilePosition,
    };
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};

    #[test]
    fn hexadecimal_identifiers_are_lowercase() {
        assert_eq!(hex(&[0x01, 0xAB, 0xCD, 0xEF]), "01abcdef");
    }

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
            client_version: 741,
        }
    }

    fn game_snapshot() -> GameSnapshot {
        GameSnapshot {
            revision: 1,
            event_sequence: 0,
            captured_tick_ms: 10,
            updated_tick_ms: 10,
            capture_duration_us: 50,
            world_generation: 1,
            lifecycle: ClientLifecycle::InGame,
            character: Some(CharacterSnapshot {
                id: Some(7),
                name: Some("Silo".into()),
                appearance: None,
                class: CharacterClass::Warrior,
                is_action_restricted: false,
                is_blinded: false,
                is_walking: false,
                is_casting: false,
                gold: 100,
                weight: 25,
                max_weight: 60,
                progression: CharacterProgression {
                    level: 10,
                    ability_level: 1,
                    experience: 1000,
                    ability_points: Some(2),
                    experience_to_next_level: Some(500),
                    ability_to_next_level: Some(800),
                },
                stats: CharacterStats {
                    strength: 10,
                    intelligence: 3,
                    wisdom: 3,
                    constitution: 8,
                    dexterity: 5,
                },
                vitals: CharacterVitals {
                    health: 100,
                    max_health: 120,
                    mana: 50,
                    max_mana: 60,
                },
                modifiers: None,
                location: None,
                inventory: None,
                equipment: None,
                spellbook: None,
                skillbook: None,
                effects: Some(Vec::new()),
            }),
            objects: Some(Vec::new()),
            dialog: None,
            group: None,
            exchange: None,
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

    #[test]
    fn snapshots_and_renders_unavailable_targets() {
        let mut registry = Registry::new();
        for event in [
            ConnectionEvent::Connecting { pid: 1 },
            ConnectionEvent::Initializing { pid: 2 },
            ConnectionEvent::NotLoaded { pid: 3 },
            ConnectionEvent::Busy { pid: 4 },
        ] {
            assert!(registry.apply(&event));
            assert!(render_event(&event).contains("status="));
        }
        let disconnected = ConnectionEvent::Disconnected {
            pid: 5,
            identity: None,
            reason: "closed".into(),
        };
        assert!(registry.apply(&disconnected));
        assert!(render_event(&disconnected).contains("instance=unknown"));

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.clients.len(), 5);
        assert_eq!(snapshot.clients[2].status, ClientSnapshotStatus::NotLoaded);
        assert_eq!(snapshot.clients[4].reason.as_deref(), Some("closed"));
    }

    #[test]
    fn removes_a_disappeared_target_and_identity() {
        let mut registry = Registry::new();
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: hello(1, 100),
            selected_version: SUPPORTED_VERSIONS.max,
        });

        assert!(registry.remove(42));
        assert!(registry.snapshot().clients.is_empty());
        assert!(registry.clients.is_empty());
        assert!(!registry.remove(42));
    }

    #[test]
    fn retains_snapshot_unavailability_for_the_current_identity() {
        let mut registry = Registry::new();
        let hello = hello(1, 100);
        let identity = ClientIdentity::from_hello(hello);
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });

        assert!(registry.apply(&ConnectionEvent::SnapshotUnavailable {
            pid: 42,
            identity,
            reason: "capture timed out".into(),
        }));
        assert_eq!(
            registry.snapshot().clients[0].snapshot_reason.as_deref(),
            Some("capture timed out")
        );
    }

    #[test]
    fn reduces_ordered_state_events_into_the_retained_snapshot() {
        let mut registry = Registry::new();
        let hello = hello(1, 100);
        let identity = ClientIdentity::from_hello(hello);
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Snapshot {
            pid: 42,
            identity,
            snapshot: Box::new(game_snapshot()),
        });

        assert!(registry.apply(&ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![
                StateEvent {
                    sequence: 1,
                    revision: 2,
                    tick_ms: 20,
                    update: StateUpdate::Status(StatusUpdate {
                        vitals: Some(CurrentVitals {
                            health: 80,
                            mana: 40,
                        }),
                        gold: Some(125),
                        is_action_restricted: Some(true),
                        ..StatusUpdate::default()
                    }),
                },
                StateEvent {
                    sequence: 2,
                    revision: 3,
                    tick_ms: 21,
                    update: StateUpdate::Location(LocationUpdate {
                        x: 43,
                        y: 40,
                        map: Some(MapChange {
                            id: 3001,
                            name: Some("Mileth".into()),
                            width: 100,
                            height: 80,
                        }),
                    }),
                },
                StateEvent {
                    sequence: 3,
                    revision: 4,
                    tick_ms: 22,
                    update: StateUpdate::Effect(EffectUpdate::Added(Effect {
                        icon: 300,
                        duration: EffectDuration::White,
                    })),
                },
                StateEvent {
                    sequence: 4,
                    revision: 5,
                    tick_ms: 23,
                    update: StateUpdate::Movement(MovementUpdate::Started {
                        current: TilePosition { x: 43, y: 40 },
                        destination: Some(TilePosition { x: 50, y: 45 }),
                    }),
                },
            ],
        }));

        let snapshot = registry.snapshot().clients[0]
            .game_snapshot
            .clone()
            .unwrap();
        assert_eq!(snapshot.revision, 5);
        assert_eq!(snapshot.event_sequence, 4);
        assert_eq!(snapshot.updated_tick_ms, 23);
        let character = snapshot.character.unwrap();
        assert_eq!(character.vitals.health, 80);
        assert_eq!(character.gold, 125);
        assert!(character.is_action_restricted);
        assert!(character.is_walking);
        assert_eq!(
            character.location,
            Some(darpc_model::MapLocation {
                id: 3001,
                name: Some("Mileth".into()),
                x: Some(43),
                y: Some(40),
                width: 100,
                height: 80,
            })
        );
        assert_eq!(
            character.effects,
            Some(vec![Effect {
                icon: 300,
                duration: EffectDuration::White,
            }])
        );
    }

    #[test]
    fn collection_batches_must_arrive_complete_and_contiguous() {
        let event = |sequence, batch_index, batch_count| StateEvent {
            sequence,
            revision: sequence,
            tick_ms: 20,
            update: StateUpdate::Inventory(SlotUpdate {
                batch_index,
                batch_count,
                change: CollectionChange::Changed,
                slot: batch_index + 1,
                before: None,
                after: Some(InventoryItem {
                    slot: batch_index + 1,
                    sprite: 21,
                    dye_color: 2,
                    name: Some("Hy-Brasyl Gauntlet".into()),
                    quantity: 1,
                    can_stack: false,
                    durability: 900,
                    max_durability: 1_000,
                }),
            }),
        };

        assert!(validate_collection_batches(&[event(1, 0, 2), event(2, 1, 2)]).is_ok());
        assert!(validate_collection_batches(&[event(1, 0, 2)]).is_err());
        assert!(validate_collection_batches(&[event(1, 1, 2)]).is_err());
    }
}
