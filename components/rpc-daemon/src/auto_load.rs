use crate::registry::ConnectionEvent;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Publish,
    Suppress,
    Start(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Loading(u64),
    Handled,
}

pub(crate) struct Policy {
    enabled: bool,
    processes: BTreeMap<u32, State>,
    next_attempt: u64,
}

impl Policy {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            processes: BTreeMap::new(),
            next_attempt: 1,
        }
    }

    pub(crate) fn observe(&mut self, event: &ConnectionEvent) -> Action {
        if !self.enabled {
            return Action::Publish;
        }

        let pid = event.pid();
        match (event, self.processes.get(&pid)) {
            (ConnectionEvent::NotLoaded { .. }, Some(State::Loading(_))) => Action::Suppress,
            (ConnectionEvent::NotLoaded { .. }, Some(State::Handled)) => Action::Publish,
            (ConnectionEvent::NotLoaded { .. }, None) => {
                let attempt = self.next_attempt;
                self.next_attempt = self.next_attempt.wrapping_add(1);
                self.processes.insert(pid, State::Loading(attempt));
                Action::Start(attempt)
            }
            (
                ConnectionEvent::Initializing { .. }
                | ConnectionEvent::Connected { .. }
                | ConnectionEvent::Busy { .. }
                | ConnectionEvent::Incompatible { .. },
                _,
            ) => {
                self.processes.insert(pid, State::Handled);
                Action::Publish
            }
            _ => Action::Publish,
        }
    }

    pub(crate) fn suppress(&mut self, pid: u32) {
        if self.enabled {
            self.processes.insert(pid, State::Handled);
        }
    }

    pub(crate) fn finish(&mut self, pid: u32, attempt: u64) -> bool {
        if self.processes.get(&pid) != Some(&State::Loading(attempt)) {
            return false;
        }
        self.processes.insert(pid, State::Handled);
        true
    }

    pub(crate) fn forget(&mut self, pid: u32) {
        self.processes.remove(&pid);
    }
}

#[cfg(windows)]
pub(crate) fn spawn(
    pid: u32,
    attempt: u64,
    lifecycle: std::sync::Arc<dyn crate::lifecycle::LifecycleControl>,
    events: std::sync::mpsc::Sender<crate::event::DaemonEvent>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(format!("darpcd-auto-load-{pid}"))
        .spawn(move || {
            let result = lifecycle.load(pid);
            let _ = events.send(crate::event::DaemonEvent::AutoLoadFinished {
                pid,
                attempt,
                result,
            });
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Action, Policy};
    use crate::registry::ConnectionEvent;

    #[test]
    fn loads_each_tracked_process_once() {
        let mut policy = Policy::new(true);
        let not_loaded = ConnectionEvent::NotLoaded { pid: 42 };

        let Action::Start(attempt) = policy.observe(&not_loaded) else {
            panic!("first not-loaded event did not start auto-load");
        };
        assert_eq!(policy.observe(&not_loaded), Action::Suppress);
        assert!(policy.finish(42, attempt));
        assert_eq!(policy.observe(&not_loaded), Action::Publish);

        policy.forget(42);
        assert!(matches!(policy.observe(&not_loaded), Action::Start(_)));
    }

    #[test]
    fn respects_existing_and_explicit_lifecycle_state() {
        for event in [
            ConnectionEvent::Initializing { pid: 1 },
            ConnectionEvent::Busy { pid: 2 },
            ConnectionEvent::Incompatible {
                pid: 3,
                identity: None,
                reason: "unsupported".into(),
            },
        ] {
            let mut policy = Policy::new(true);
            let pid = event.pid();
            assert_eq!(policy.observe(&event), Action::Publish);
            assert_eq!(
                policy.observe(&ConnectionEvent::NotLoaded { pid }),
                Action::Publish
            );
        }

        let mut disabled = Policy::new(false);
        assert_eq!(
            disabled.observe(&ConnectionEvent::NotLoaded { pid: 4 }),
            Action::Publish
        );

        let mut completed_elsewhere = Policy::new(true);
        let Action::Start(attempt) =
            completed_elsewhere.observe(&ConnectionEvent::NotLoaded { pid: 5 })
        else {
            panic!("first not-loaded event did not start auto-load");
        };
        assert_eq!(
            completed_elsewhere.observe(&ConnectionEvent::Busy { pid: 5 }),
            Action::Publish
        );
        assert!(!completed_elsewhere.finish(5, attempt));

        let mut explicit_operation = Policy::new(true);
        explicit_operation.suppress(6);
        assert_eq!(
            explicit_operation.observe(&ConnectionEvent::NotLoaded { pid: 6 }),
            Action::Publish
        );
    }
}
