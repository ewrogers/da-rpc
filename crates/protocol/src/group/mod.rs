use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u32},
};
use darpc_model::{
    GroupInvitation, GroupInvitationCloseReason, GroupMember, GroupState, GroupUpdate,
};

const MAX_GROUP_MEMBERS: usize = 64;
const MAX_GROUP_INVITATIONS: usize = 8;
const MAX_GROUP_NAME_BYTES: usize = 64;

pub(crate) fn encode_optional_state(
    output: &mut Vec<u8>,
    state: Option<&GroupState>,
) -> Result<(), EncodeError> {
    push_bool(output, state.is_some());
    if let Some(state) = state {
        encode_state(output, state)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_state(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<GroupState>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_state(reader))
        .transpose()
}

pub(crate) fn encode_update(output: &mut Vec<u8>, update: &GroupUpdate) -> Result<(), EncodeError> {
    match update {
        GroupUpdate::SettingsChanged { state } => {
            output.push(8);
            encode_state(output, state)?;
        }
        GroupUpdate::InvitationSent { target } => {
            output.push(1);
            encode_name(output, target)?;
        }
        GroupUpdate::InvitationReceived { invitation, state } => {
            output.push(2);
            encode_invitation(output, invitation)?;
            encode_state(output, state)?;
        }
        GroupUpdate::InvitationClosed {
            invitation,
            reason,
            state,
        } => {
            output.push(3);
            encode_invitation(output, invitation)?;
            output.push(match reason {
                GroupInvitationCloseReason::AcceptRequested => 1,
                GroupInvitationCloseReason::Declined => 2,
                GroupInvitationCloseReason::Dismissed => 3,
            });
            encode_state(output, state)?;
        }
        GroupUpdate::Joined { state } => {
            output.push(4);
            encode_state(output, state)?;
        }
        GroupUpdate::MemberJoined { member, state } => {
            output.push(5);
            encode_member(output, member)?;
            encode_state(output, state)?;
        }
        GroupUpdate::MemberLeft { member, state } => {
            output.push(6);
            encode_member(output, member)?;
            encode_state(output, state)?;
        }
        GroupUpdate::Disbanded { state } => {
            output.push(7);
            encode_state(output, state)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<GroupUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(GroupUpdate::InvitationSent {
            target: decode_name(reader)?,
        }),
        2 => Ok(GroupUpdate::InvitationReceived {
            invitation: decode_invitation(reader)?,
            state: decode_state(reader)?,
        }),
        3 => Ok(GroupUpdate::InvitationClosed {
            invitation: decode_invitation(reader)?,
            reason: match reader.read_u8()? {
                1 => GroupInvitationCloseReason::AcceptRequested,
                2 => GroupInvitationCloseReason::Declined,
                3 => GroupInvitationCloseReason::Dismissed,
                actual => return Err(DecodeError::InvalidGroupField { actual }),
            },
            state: decode_state(reader)?,
        }),
        4 => Ok(GroupUpdate::Joined {
            state: decode_state(reader)?,
        }),
        5 => Ok(GroupUpdate::MemberJoined {
            member: decode_member(reader)?,
            state: decode_state(reader)?,
        }),
        6 => Ok(GroupUpdate::MemberLeft {
            member: decode_member(reader)?,
            state: decode_state(reader)?,
        }),
        7 => Ok(GroupUpdate::Disbanded {
            state: decode_state(reader)?,
        }),
        8 => Ok(GroupUpdate::SettingsChanged {
            state: decode_state(reader)?,
        }),
        actual => Err(DecodeError::InvalidGroupField { actual }),
    }
}

fn encode_state(output: &mut Vec<u8>, state: &GroupState) -> Result<(), EncodeError> {
    encode_members(output, &state.members)?;
    encode_count(output, state.invitations.len(), MAX_GROUP_INVITATIONS)?;
    for invitation in &state.invitations {
        encode_invitation(output, invitation)?;
    }
    encode_optional_bool(output, state.is_group_open);
    encode_optional_bool(output, state.auto_accept);
    Ok(())
}

fn decode_state(reader: &mut PayloadReader<'_>) -> Result<GroupState, DecodeError> {
    let members = decode_members(reader)?;
    let invitation_count = decode_count(reader, MAX_GROUP_INVITATIONS)?;
    let invitations = (0..invitation_count)
        .map(|_| decode_invitation(reader))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GroupState {
        members,
        invitations,
        is_group_open: decode_optional_bool(reader)?,
        auto_accept: decode_optional_bool(reader)?,
    })
}

fn encode_members(output: &mut Vec<u8>, members: &[GroupMember]) -> Result<(), EncodeError> {
    encode_count(output, members.len(), MAX_GROUP_MEMBERS)?;
    for member in members {
        encode_member(output, member)?;
    }
    Ok(())
}

fn decode_members(reader: &mut PayloadReader<'_>) -> Result<Vec<GroupMember>, DecodeError> {
    let count = decode_count(reader, MAX_GROUP_MEMBERS)?;
    (0..count).map(|_| decode_member(reader)).collect()
}

fn encode_member(output: &mut Vec<u8>, member: &GroupMember) -> Result<(), EncodeError> {
    encode_name(output, &member.name)?;
    push_bool(output, member.is_leader);
    Ok(())
}

fn decode_member(reader: &mut PayloadReader<'_>) -> Result<GroupMember, DecodeError> {
    Ok(GroupMember {
        name: decode_name(reader)?,
        is_leader: reader.read_bool()?,
    })
}

fn encode_invitation(
    output: &mut Vec<u8>,
    invitation: &GroupInvitation,
) -> Result<(), EncodeError> {
    push_u32(output, invitation.id);
    encode_name(output, &invitation.inviter)?;
    push_bool(output, invitation.received_tick_ms.is_some());
    if let Some(tick_ms) = invitation.received_tick_ms {
        push_u32(output, tick_ms);
    }
    Ok(())
}

fn decode_invitation(reader: &mut PayloadReader<'_>) -> Result<GroupInvitation, DecodeError> {
    Ok(GroupInvitation {
        id: reader.read_u32()?,
        inviter: decode_name(reader)?,
        received_tick_ms: reader.read_bool()?.then(|| reader.read_u32()).transpose()?,
    })
}

fn encode_name(output: &mut Vec<u8>, value: &str) -> Result<(), EncodeError> {
    if value.is_empty() || value.len() > MAX_GROUP_NAME_BYTES {
        return Err(EncodeError::EventStringTooLong {
            length: value.len(),
            max: MAX_GROUP_NAME_BYTES,
        });
    }
    output.push(u8::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow)?);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_name(reader: &mut PayloadReader<'_>) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u8()?);
    if length == 0 || length > MAX_GROUP_NAME_BYTES {
        return Err(DecodeError::EventStringTooLong {
            length,
            max: MAX_GROUP_NAME_BYTES,
        });
    }
    String::from_utf8(reader.take(length)?.to_vec())
        .map_err(|_| DecodeError::InvalidGroupField { actual: 0 })
}

fn encode_count(output: &mut Vec<u8>, count: usize, max: usize) -> Result<(), EncodeError> {
    if count > max {
        return Err(EncodeError::SnapshotCollectionTooLong { length: count, max });
    }
    output.push(u8::try_from(count).map_err(|_| EncodeError::LengthOverflow)?);
    Ok(())
}

fn decode_count(reader: &mut PayloadReader<'_>, max: usize) -> Result<usize, DecodeError> {
    let count = usize::from(reader.read_u8()?);
    if count > max {
        return Err(DecodeError::SnapshotCollectionTooLong { length: count, max });
    }
    Ok(count)
}

fn encode_optional_bool(output: &mut Vec<u8>, value: Option<bool>) {
    output.push(match value {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    });
}

fn decode_optional_bool(reader: &mut PayloadReader<'_>) -> Result<Option<bool>, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(false)),
        2 => Ok(Some(true)),
        actual => Err(DecodeError::InvalidGroupField { actual }),
    }
}
