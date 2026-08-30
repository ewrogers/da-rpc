use crate::{
    commands::{CommandCall, CommandReply},
    event::DaemonEvent,
    registry::{CommitOutcome, ConnectionEvent, Registry, RegistrySnapshot},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

const LAUNCH_DISCOVERY_GRACE: Duration = Duration::from_secs(5);

trait WorkerHandle {
    fn stop(&self);
    fn route_command(&self, call: CommandCall);
    fn request_fresh_snapshot(&self);
}

#[cfg(windows)]
impl WorkerHandle for crate::connection::Worker {
    fn stop(&self) {
        crate::connection::Worker::stop(self);
    }

    fn route_command(&self, call: CommandCall) {
        crate::connection::Worker::route_command(self, call);
    }

    fn request_fresh_snapshot(&self) {
        crate::connection::Worker::request_fresh_snapshot(self);
    }
}

type WorkerSpawner = dyn FnMut(u32, Sender<DaemonEvent>) -> io::Result<Box<dyn WorkerHandle>>;

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ReconcileOutcome {
    pub(crate) changed: bool,
    pub(crate) removed: Vec<u32>,
}

/// Owns the complete lifecycle of the daemon's current client roster.
///
/// A tracked target can temporarily lack a worker when thread creation fails.
/// Keeping target membership separate from worker availability ensures that
/// discovery can retry it and can still remove its registry record later.
pub(crate) struct ClientRoster {
    events: Sender<DaemonEvent>,
    spawn_worker: Box<WorkerSpawner>,
    explicit_pids: BTreeSet<u32>,
    launch_grace: BTreeMap<u32, Instant>,
    tracked_pids: BTreeSet<u32>,
    workers: BTreeMap<u32, Box<dyn WorkerHandle>>,
    registry: Registry,
}

impl ClientRoster {
    #[cfg(windows)]
    pub(crate) fn new(explicit_pids: BTreeSet<u32>, events: Sender<DaemonEvent>) -> Self {
        Self::with_spawner(explicit_pids, events, |pid, events| {
            crate::connection::spawn(pid, events)
                .map(|worker| Box::new(worker) as Box<dyn WorkerHandle>)
        })
    }

    fn with_spawner(
        explicit_pids: BTreeSet<u32>,
        events: Sender<DaemonEvent>,
        spawn_worker: impl FnMut(u32, Sender<DaemonEvent>) -> io::Result<Box<dyn WorkerHandle>>
        + 'static,
    ) -> Self {
        Self {
            events,
            spawn_worker: Box::new(spawn_worker),
            explicit_pids,
            launch_grace: BTreeMap::new(),
            tracked_pids: BTreeSet::new(),
            workers: BTreeMap::new(),
            registry: Registry::new(),
        }
    }

    #[must_use]
    pub(crate) fn contains(&self, pid: u32) -> bool {
        self.tracked_pids.contains(&pid)
    }

    pub(crate) fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.tracked_pids.iter().copied()
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> RegistrySnapshot {
        self.registry.snapshot()
    }

    /// Immediately tracks a client launched through the HTTP lifecycle route.
    /// Discovery may omit its window briefly, so the target remains desired for
    /// a bounded grace period.
    pub(crate) fn track_launched(&mut self, pid: u32, now: Instant) -> bool {
        self.launch_grace.insert(pid, now + LAUNCH_DISCOVERY_GRACE);
        self.ensure_worker(pid)
    }

    /// Reconciles explicit, discovered, and recently launched targets.
    pub(crate) fn reconcile(
        &mut self,
        discovered: &BTreeSet<u32>,
        now: Instant,
    ) -> ReconcileOutcome {
        self.launch_grace
            .retain(|pid, deadline| !discovered.contains(pid) && *deadline > now);

        let mut desired = self.explicit_pids.clone();
        desired.extend(discovered);
        desired.extend(self.launch_grace.keys().copied());

        let mut changed = false;
        for &pid in &desired {
            changed |= self.ensure_worker(pid);
        }

        let removed = self
            .tracked_pids
            .difference(&desired)
            .copied()
            .collect::<Vec<_>>();
        for &pid in &removed {
            self.tracked_pids.remove(&pid);
            if let Some(worker) = self.workers.remove(&pid) {
                worker.stop();
            }
            changed |= self.registry.remove(pid);
            println!("client pid={pid} status=removed");
        }

        ReconcileOutcome { changed, removed }
    }

    /// Applies an event only while its target belongs to this roster.
    /// Rejected observations also request the fresh snapshot required to
    /// re-establish the worker's event boundary.
    #[must_use = "the caller must publish or recover from the commit outcome"]
    pub(crate) fn commit(&mut self, event: ConnectionEvent) -> CommitOutcome {
        if !self.contains(event.pid()) {
            return CommitOutcome::Ignored;
        }
        let outcome = self.registry.commit(event);
        if let CommitOutcome::ObservationRejected { pid, .. } = &outcome
            && let Some(worker) = self.workers.get(pid)
        {
            worker.request_fresh_snapshot();
        }
        outcome
    }

    pub(crate) fn route_command(&self, call: CommandCall) {
        if let Some(worker) = self.workers.get(&call.pid) {
            worker.route_command(call);
        } else {
            let _ = call.reply.send(CommandReply::Unavailable);
        }
    }

    fn ensure_worker(&mut self, pid: u32) -> bool {
        self.tracked_pids.insert(pid);
        if self.workers.contains_key(&pid) {
            return false;
        }

        let mut changed = !matches!(
            self.registry.commit(ConnectionEvent::Connecting { pid }),
            CommitOutcome::Ignored
        );
        match (self.spawn_worker)(pid, self.events.clone()) {
            Ok(worker) => {
                self.workers.insert(pid, worker);
            }
            Err(error) => {
                let event = ConnectionEvent::Disconnected {
                    pid,
                    identity: None,
                    reason: format!("failed to start connection worker: {error}"),
                };
                changed |= !matches!(self.registry.commit(event.clone()), CommitOutcome::Ignored);
                eprintln!("darpcd: {}", crate::registry::render_event(&event));
            }
        }
        changed
    }
}

impl Drop for ClientRoster {
    fn drop(&mut self) {
        for worker in self.workers.values() {
            worker.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::ClientOperation,
        registry::{ClientIdentity, ClientSnapshotStatus},
    };
    use darpc_model::{
        ClientLifecycle, ClientSnapshot as GameSnapshot, LifecycleUpdate, MessageDialogsState,
        StateEvent, StateUpdate,
    };
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    #[derive(Default)]
    struct FakeWorkerState {
        stops: AtomicUsize,
        refreshes: AtomicUsize,
        routed_commands: AtomicUsize,
    }

    struct FakeWorker {
        state: Arc<FakeWorkerState>,
    }

    impl WorkerHandle for FakeWorker {
        fn stop(&self) {
            self.state.stops.fetch_add(1, Ordering::Relaxed);
        }

        fn route_command(&self, _call: CommandCall) {
            self.state.routed_commands.fetch_add(1, Ordering::Relaxed);
        }

        fn request_fresh_snapshot(&self) {
            self.state.refreshes.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct FactoryState {
        failures: BTreeSet<u32>,
        attempts: Mutex<Vec<u32>>,
        workers: Mutex<BTreeMap<u32, Arc<FakeWorkerState>>>,
    }

    fn roster(
        explicit_pids: BTreeSet<u32>,
        failures: BTreeSet<u32>,
    ) -> (ClientRoster, Arc<FactoryState>) {
        let state = Arc::new(FactoryState {
            failures,
            attempts: Mutex::new(Vec::new()),
            workers: Mutex::new(BTreeMap::new()),
        });
        let factory_state = Arc::clone(&state);
        let (events, _receiver) = mpsc::channel();
        let roster = ClientRoster::with_spawner(explicit_pids, events, move |pid, _events| {
            factory_state.attempts.lock().unwrap().push(pid);
            if factory_state.failures.contains(&pid) {
                return Err(io::Error::other("worker unavailable"));
            }
            let worker_state = Arc::new(FakeWorkerState::default());
            factory_state
                .workers
                .lock()
                .unwrap()
                .insert(pid, Arc::clone(&worker_state));
            Ok(Box::new(FakeWorker {
                state: worker_state,
            }))
        });
        (roster, state)
    }

    fn pids(roster: &ClientRoster) -> Vec<u32> {
        roster
            .snapshot()
            .clients
            .into_iter()
            .map(|client| client.pid)
            .collect()
    }

    fn hello() -> Hello {
        Hello {
            protocol_versions: SUPPORTED_VERSIONS,
            dll_instance_id: [1; 16],
            process_id: 42,
            process_creation_time: 1,
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 1,
                minor: 7,
                patch: 0,
            },
            executable_fingerprint: [2; 32],
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
            character: None,
            objects: None,
            dialog: None,
            message_dialogs: MessageDialogsState::default(),
            active_bulletin: None,
            active_field_map: None,
            group: None,
            exchange: None,
            legend: None,
            planned_route: None,
        }
    }

    #[test]
    fn reconciles_worker_and_registry_lifetimes() {
        let now = Instant::now();
        let (mut roster, state) = roster(BTreeSet::from([1]), BTreeSet::new());

        let outcome = roster.reconcile(&BTreeSet::from([2]), now);
        assert!(outcome.changed);
        assert!(outcome.removed.is_empty());
        assert_eq!(roster.pids().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(pids(&roster), vec![1, 2]);

        let worker_1 = Arc::clone(&state.workers.lock().unwrap()[&1]);
        let worker_2 = Arc::clone(&state.workers.lock().unwrap()[&2]);
        let outcome = roster.reconcile(&BTreeSet::new(), now);
        assert!(outcome.changed);
        assert_eq!(outcome.removed, vec![2]);
        assert_eq!(pids(&roster), vec![1]);
        assert_eq!(worker_2.stops.load(Ordering::Relaxed), 1);

        drop(roster);
        assert_eq!(worker_1.stops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn removes_target_after_worker_start_failure() {
        let now = Instant::now();
        let (mut roster, state) = roster(BTreeSet::new(), BTreeSet::from([42]));

        let outcome = roster.reconcile(&BTreeSet::from([42]), now);
        assert!(outcome.changed);
        assert_eq!(state.attempts.lock().unwrap().as_slice(), &[42]);
        assert_eq!(
            roster.snapshot().clients[0].status,
            ClientSnapshotStatus::Disconnected
        );

        let outcome = roster.reconcile(&BTreeSet::new(), now);
        assert!(outcome.changed);
        assert_eq!(outcome.removed, vec![42]);
        assert!(roster.snapshot().clients.is_empty());
    }

    #[test]
    fn retains_launched_target_during_discovery_grace() {
        let now = Instant::now();
        let (mut roster, _state) = roster(BTreeSet::new(), BTreeSet::new());

        assert!(roster.track_launched(42, now));
        let outcome = roster.reconcile(&BTreeSet::new(), now + Duration::from_secs(4));
        assert!(outcome.removed.is_empty());
        assert!(roster.contains(42));

        let outcome = roster.reconcile(&BTreeSet::new(), now + LAUNCH_DISCOVERY_GRACE);
        assert_eq!(outcome.removed, vec![42]);
        assert!(!roster.contains(42));
    }

    #[test]
    fn filters_untracked_events_and_routes_commands_by_worker_availability() {
        let now = Instant::now();
        let (mut roster, state) = roster(BTreeSet::new(), BTreeSet::new());
        assert_eq!(
            roster.commit(ConnectionEvent::Connecting { pid: 42 }),
            CommitOutcome::Ignored
        );

        let (reply, mut response) = tokio::sync::oneshot::channel();
        roster.route_command(CommandCall {
            pid: 42,
            identity: ClientIdentity {
                pid: 42,
                process_creation_time: 1,
                dll_instance_id: [1; 16],
            },
            operation: ClientOperation::Snapshot(crate::commands::SnapshotFreshness::Recent),
            reply,
        });
        assert_eq!(response.try_recv().unwrap(), CommandReply::Unavailable);

        assert!(roster.track_launched(42, now));
        let (reply, _response) = tokio::sync::oneshot::channel();
        roster.route_command(CommandCall {
            pid: 42,
            identity: ClientIdentity {
                pid: 42,
                process_creation_time: 1,
                dll_instance_id: [1; 16],
            },
            operation: ClientOperation::Snapshot(crate::commands::SnapshotFreshness::Recent),
            reply,
        });
        let worker = Arc::clone(&state.workers.lock().unwrap()[&42]);
        assert_eq!(worker.routed_commands.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rejected_observation_requests_a_fresh_worker_snapshot() {
        let (mut roster, state) = roster(BTreeSet::new(), BTreeSet::new());
        assert!(roster.track_launched(42, Instant::now()));
        let hello = hello();
        let identity = ClientIdentity::from_hello(hello);
        assert!(matches!(
            roster.commit(ConnectionEvent::Connected {
                pid: 42,
                hello,
                selected_version: SUPPORTED_VERSIONS.max,
            }),
            CommitOutcome::Applied(_)
        ));
        assert!(matches!(
            roster.commit(ConnectionEvent::Snapshot {
                pid: 42,
                identity,
                snapshot: Box::new(game_snapshot()),
            }),
            CommitOutcome::Applied(_)
        ));

        let outcome = roster.commit(ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![StateEvent {
                sequence: 2,
                revision: 2,
                tick_ms: 20,
                update: StateUpdate::Lifecycle(LifecycleUpdate {
                    previous: ClientLifecycle::InGame,
                    current: ClientLifecycle::Disconnected,
                }),
            }],
        });
        assert!(matches!(outcome, CommitOutcome::ObservationRejected { .. }));
        let worker = Arc::clone(&state.workers.lock().unwrap()[&42]);
        assert_eq!(worker.refreshes.load(Ordering::Relaxed), 1);
    }
}
