pub const GROUP_MEMBER_CAPACITY: usize = 64;
pub const GROUP_INVITATION_CAPACITY: usize = 8;
pub const GROUP_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawGroupState {
    pub member_count: u8,
    pub members: [RawGroupMember; GROUP_MEMBER_CAPACITY],
    pub invitation_count: u8,
    pub invitations: [RawGroupInvitation; GROUP_INVITATION_CAPACITY],
    pub is_group_open: Option<bool>,
    pub auto_accept: Option<bool>,
}

impl RawGroupState {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            member_count: 0,
            members: [RawGroupMember::empty(); GROUP_MEMBER_CAPACITY],
            invitation_count: 0,
            invitations: [RawGroupInvitation::empty(); GROUP_INVITATION_CAPACITY],
            is_group_open: None,
            auto_accept: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawGroupMember {
    pub name: [u8; GROUP_NAME_BYTES],
    pub name_len: u8,
    pub is_leader: bool,
}

impl RawGroupMember {
    const fn empty() -> Self {
        Self {
            name: [0; GROUP_NAME_BYTES],
            name_len: 0,
            is_leader: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawGroupInvitation {
    pub id: u32,
    pub inviter: [u8; GROUP_NAME_BYTES],
    pub inviter_len: u8,
    pub received_tick_ms: Option<u32>,
}

impl RawGroupInvitation {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            inviter: [0; GROUP_NAME_BYTES],
            inviter_len: 0,
            received_tick_ms: None,
        }
    }
}
