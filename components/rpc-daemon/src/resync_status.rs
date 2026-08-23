use crate::registry::ClientIdentity;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use utoipa::ToSchema;

const MAX_PENDING_RESYNCS: usize = 64;
const COMPLETED_RESYNC_RETENTION: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResyncPhase {
    /// No resync request is currently visible to the daemon.
    Idle,
    /// The active request is waiting for its outgoing packet to be observed.
    WaitingToSend,
    /// The outgoing packet was observed and the server response is pending.
    AwaitingResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ResyncSchedulerStatus {
    /// Current daemon-observed phase of the active resync request.
    pub(crate) phase: ResyncPhase,
    /// Correlation ID of the active request, or null when idle.
    pub(crate) active_resync_id: Option<u32>,
    /// Accepted resync requests waiting behind the active request.
    pub(crate) pending_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveResync {
    id: u32,
    phase: ResyncPhase,
}

#[derive(Debug, Default)]
struct ResyncTracker {
    active: Option<ActiveResync>,
    pending: VecDeque<u32>,
    completed: VecDeque<u32>,
}

impl ResyncTracker {
    fn accepted(&mut self, resync_id: u32) {
        if self.completed.contains(&resync_id)
            || self.active.is_some_and(|active| active.id == resync_id)
            || self.pending.contains(&resync_id)
        {
            return;
        }
        if self.active.is_none() {
            self.active = Some(ActiveResync {
                id: resync_id,
                phase: ResyncPhase::WaitingToSend,
            });
        } else if self.pending.len() < MAX_PENDING_RESYNCS {
            self.pending.push_back(resync_id);
        }
    }

    fn outgoing(&mut self, resync_id: u32) {
        if self.active.is_some_and(|active| active.id == resync_id) {
            self.active = Some(ActiveResync {
                id: resync_id,
                phase: ResyncPhase::AwaitingResponse,
            });
            return;
        }

        if let Some(index) = self.pending.iter().position(|id| *id == resync_id) {
            self.pending.remove(index);
        }
        if let Some(previous) = self.active.take() {
            if self.pending.len() == MAX_PENDING_RESYNCS {
                self.pending.pop_back();
            }
            self.pending.push_front(previous.id);
        }
        self.active = Some(ActiveResync {
            id: resync_id,
            phase: ResyncPhase::AwaitingResponse,
        });
    }

    fn completed(&mut self, resync_id: u32) {
        if !self.completed.contains(&resync_id) {
            if self.completed.len() == COMPLETED_RESYNC_RETENTION {
                self.completed.pop_front();
            }
            self.completed.push_back(resync_id);
        }

        if self.active.is_some_and(|active| active.id == resync_id) {
            self.active = self.pending.pop_front().map(|id| ActiveResync {
                id,
                phase: ResyncPhase::WaitingToSend,
            });
        } else if let Some(index) = self.pending.iter().position(|id| *id == resync_id) {
            self.pending.remove(index);
        }
    }

    fn status(&self) -> ResyncSchedulerStatus {
        ResyncSchedulerStatus {
            phase: self.active.map_or(ResyncPhase::Idle, |active| active.phase),
            active_resync_id: self.active.map(|active| active.id),
            pending_count: u32::try_from(self.pending.len())
                .expect("bounded resync pending count fits u32"),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ResyncTrackers {
    clients: BTreeMap<ClientIdentity, ResyncTracker>,
}

impl ResyncTrackers {
    pub(crate) fn accepted(
        &mut self,
        identity: ClientIdentity,
        resync_id: u32,
    ) -> ResyncSchedulerStatus {
        let tracker = self.clients.entry(identity).or_default();
        tracker.accepted(resync_id);
        tracker.status()
    }

    pub(crate) fn outgoing(&mut self, identity: ClientIdentity, resync_id: u32) {
        self.clients
            .entry(identity)
            .or_default()
            .outgoing(resync_id);
    }

    pub(crate) fn completed(&mut self, identity: ClientIdentity, resync_id: u32) {
        self.clients
            .entry(identity)
            .or_default()
            .completed(resync_id);
    }

    pub(crate) fn status(&self, identity: ClientIdentity) -> ResyncSchedulerStatus {
        self.clients
            .get(&identity)
            .map_or_else(|| ResyncTracker::default().status(), ResyncTracker::status)
    }

    pub(crate) fn remove(&mut self, identity: ClientIdentity) {
        self.clients.remove(&identity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ClientIdentity {
        ClientIdentity {
            pid: 42,
            process_creation_time: 7,
            dll_instance_id: [0xab; 16],
        }
    }

    #[test]
    fn follows_accepted_outgoing_and_completed_phases() {
        let mut trackers = ResyncTrackers::default();
        let identity = identity();

        assert_eq!(
            trackers.accepted(identity, 7),
            ResyncSchedulerStatus {
                phase: ResyncPhase::WaitingToSend,
                active_resync_id: Some(7),
                pending_count: 0,
            }
        );
        assert_eq!(trackers.accepted(identity, 9).pending_count, 1);

        trackers.outgoing(identity, 7);
        assert_eq!(
            trackers.status(identity),
            ResyncSchedulerStatus {
                phase: ResyncPhase::AwaitingResponse,
                active_resync_id: Some(7),
                pending_count: 1,
            }
        );

        trackers.completed(identity, 7);
        assert_eq!(
            trackers.status(identity),
            ResyncSchedulerStatus {
                phase: ResyncPhase::WaitingToSend,
                active_resync_id: Some(9),
                pending_count: 0,
            }
        );
    }

    #[test]
    fn handles_events_that_arrive_before_the_http_response() {
        let mut trackers = ResyncTrackers::default();
        let identity = identity();

        trackers.outgoing(identity, 7);
        trackers.completed(identity, 7);
        trackers.accepted(identity, 7);

        assert_eq!(trackers.status(identity).phase, ResyncPhase::Idle);
    }

    #[test]
    fn inserts_an_unobserved_physical_refresh_before_http_work() {
        let mut trackers = ResyncTrackers::default();
        let identity = identity();

        trackers.accepted(identity, 7);
        trackers.accepted(identity, 9);
        trackers.outgoing(identity, 11);

        assert_eq!(
            trackers.status(identity),
            ResyncSchedulerStatus {
                phase: ResyncPhase::AwaitingResponse,
                active_resync_id: Some(11),
                pending_count: 2,
            }
        );
        trackers.completed(identity, 11);
        assert_eq!(trackers.status(identity).active_resync_id, Some(7));
    }
}
