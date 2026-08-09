use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GroupSnapshot {
    observation: ObservationMetadata,
    /// Current group state, or null outside the game world.
    group: Option<GroupState>,
}

impl GroupSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            group: snapshot.group.as_ref().map(GroupState::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupState {
    members: Vec<GroupMember>,
    invitations: Vec<GroupInvitation>,
    is_group_open: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_accept: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct GroupMember {
    name: String,
    is_leader: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupInvitation {
    id: u32,
    inviter: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    received_tick_ms: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupInvitationSent {
    pub(crate) observation: EventObservation,
    target: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupInvitationReceived {
    pub(crate) observation: EventObservation,
    invitation: GroupInvitation,
    group: GroupState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupInvitationClosed {
    pub(crate) observation: EventObservation,
    invitation: GroupInvitation,
    reason: GroupInvitationCloseReason,
    group: GroupState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupJoined {
    pub(crate) observation: EventObservation,
    group: GroupState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupSettingsChanged {
    pub(crate) observation: EventObservation,
    group: GroupState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupMemberChanged {
    pub(crate) observation: EventObservation,
    member: GroupMember,
    group: GroupState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GroupDisbanded {
    pub(crate) observation: EventObservation,
    group: GroupState,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GroupInvitationCloseReason {
    AcceptRequested,
    Declined,
    Dismissed,
}

impl From<&model::GroupState> for GroupState {
    fn from(value: &model::GroupState) -> Self {
        Self {
            members: value.members.iter().map(GroupMember::from).collect(),
            invitations: value
                .invitations
                .iter()
                .map(GroupInvitation::from)
                .collect(),
            is_group_open: value.is_group_open,
            auto_accept: value.auto_accept,
        }
    }
}

impl From<&model::GroupMember> for GroupMember {
    fn from(value: &model::GroupMember) -> Self {
        Self {
            name: value.name.clone(),
            is_leader: value.is_leader,
        }
    }
}

impl From<&model::GroupInvitation> for GroupInvitation {
    fn from(value: &model::GroupInvitation) -> Self {
        Self {
            id: value.id,
            inviter: value.inviter.clone(),
            received_tick_ms: value.received_tick_ms,
        }
    }
}

impl GroupInvitationSent {
    pub(crate) fn new(observation: EventObservation, target: String) -> Self {
        Self {
            observation,
            target,
        }
    }
}

impl GroupInvitationReceived {
    pub(crate) fn new(
        observation: EventObservation,
        invitation: model::GroupInvitation,
        state: model::GroupState,
    ) -> Self {
        Self {
            observation,
            invitation: GroupInvitation::from(&invitation),
            group: GroupState::from(&state),
        }
    }
}

impl GroupInvitationClosed {
    pub(crate) fn new(
        observation: EventObservation,
        invitation: model::GroupInvitation,
        reason: model::GroupInvitationCloseReason,
        state: model::GroupState,
    ) -> Self {
        Self {
            observation,
            invitation: GroupInvitation::from(&invitation),
            reason: reason.into(),
            group: GroupState::from(&state),
        }
    }
}

impl GroupJoined {
    pub(crate) fn new(observation: EventObservation, state: model::GroupState) -> Self {
        Self {
            observation,
            group: GroupState::from(&state),
        }
    }
}

impl GroupSettingsChanged {
    pub(crate) fn new(observation: EventObservation, state: model::GroupState) -> Self {
        Self {
            observation,
            group: GroupState::from(&state),
        }
    }
}

impl GroupMemberChanged {
    pub(crate) fn new(
        observation: EventObservation,
        member: model::GroupMember,
        state: model::GroupState,
    ) -> Self {
        Self {
            observation,
            member: GroupMember::from(&member),
            group: GroupState::from(&state),
        }
    }
}

impl GroupDisbanded {
    pub(crate) fn new(observation: EventObservation, state: model::GroupState) -> Self {
        Self {
            observation,
            group: GroupState::from(&state),
        }
    }
}

impl From<model::GroupInvitationCloseReason> for GroupInvitationCloseReason {
    fn from(value: model::GroupInvitationCloseReason) -> Self {
        match value {
            model::GroupInvitationCloseReason::AcceptRequested => Self::AcceptRequested,
            model::GroupInvitationCloseReason::Declined => Self::Declined,
            model::GroupInvitationCloseReason::Dismissed => Self::Dismissed,
        }
    }
}
