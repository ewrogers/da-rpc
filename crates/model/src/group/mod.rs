#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GroupState {
    pub members: Vec<GroupMember>,
    pub invitations: Vec<GroupInvitation>,
    pub is_group_open: Option<bool>,
    pub auto_accept: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupMember {
    pub name: String,
    pub is_leader: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupInvitation {
    pub id: u32,
    pub inviter: String,
    pub received_tick_ms: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroupUpdate {
    SettingsChanged {
        state: GroupState,
    },
    InvitationSent {
        target: String,
    },
    InvitationReceived {
        invitation: GroupInvitation,
        state: GroupState,
    },
    InvitationClosed {
        invitation: GroupInvitation,
        reason: GroupInvitationCloseReason,
        state: GroupState,
    },
    Joined {
        state: GroupState,
    },
    MemberJoined {
        member: GroupMember,
        state: GroupState,
    },
    MemberLeft {
        member: GroupMember,
        state: GroupState,
    },
    Disbanded {
        state: GroupState,
    },
}

impl GroupUpdate {
    #[must_use]
    pub const fn state(&self) -> Option<&GroupState> {
        match self {
            Self::InvitationSent { .. } => None,
            Self::SettingsChanged { state }
            | Self::InvitationReceived { state, .. }
            | Self::InvitationClosed { state, .. }
            | Self::Joined { state }
            | Self::MemberJoined { state, .. }
            | Self::MemberLeft { state, .. }
            | Self::Disbanded { state } => Some(state),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupInvitationCloseReason {
    AcceptRequested,
    Declined,
    Dismissed,
}
