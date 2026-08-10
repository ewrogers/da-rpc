use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16, push_u32},
};
use darpc_model::{
    EquipmentSlot, Nation, PlayerEquipmentItem, PlayerIdentity, PlayerProfile, UserState,
};

pub const MAX_PLAYER_IDENTITY_TEXT_LEN: usize = u8::MAX as usize;
pub const MAX_PLAYER_EQUIPMENT_ITEMS: usize = 18;

pub(crate) fn encode_identity(
    output: &mut Vec<u8>,
    identity: &PlayerIdentity,
) -> Result<(), EncodeError> {
    output.push(identity.nation.raw());
    encode_string(output, &identity.title)?;
    encode_string(output, &identity.guild_rank)?;
    encode_string(output, &identity.display_class)?;
    encode_string(output, &identity.guild)
}

pub(crate) fn decode_identity(
    reader: &mut PayloadReader<'_>,
) -> Result<PlayerIdentity, DecodeError> {
    let actual = reader.read_u8()?;
    let nation = Nation::from_raw(actual).ok_or(DecodeError::InvalidNation { actual })?;
    Ok(PlayerIdentity {
        nation,
        title: decode_string(reader)?,
        guild_rank: decode_string(reader)?,
        display_class: decode_string(reader)?,
        guild: decode_string(reader)?,
    })
}

pub(crate) fn encode_optional_identity(
    output: &mut Vec<u8>,
    identity: Option<&PlayerIdentity>,
) -> Result<(), EncodeError> {
    push_bool(output, identity.is_some());
    if let Some(identity) = identity {
        encode_identity(output, identity)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_identity(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<PlayerIdentity>, DecodeError> {
    if reader.read_bool()? {
        decode_identity(reader).map(Some)
    } else {
        Ok(None)
    }
}

pub(crate) fn encode_profile(
    output: &mut Vec<u8>,
    profile: &PlayerProfile,
) -> Result<(), EncodeError> {
    encode_identity(output, &profile.identity)?;
    output.push(profile.user_state.raw());
    push_bool(output, profile.is_group_open);
    if profile.equipment.len() > MAX_PLAYER_EQUIPMENT_ITEMS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: profile.equipment.len(),
            max: MAX_PLAYER_EQUIPMENT_ITEMS,
        });
    }
    output.push(u8::try_from(profile.equipment.len()).map_err(|_| EncodeError::LengthOverflow)?);
    for (index, item) in profile.equipment.iter().enumerate() {
        let slot = item.slot.raw();
        if profile.equipment[..index]
            .iter()
            .any(|current| current.slot.raw() == slot)
        {
            return Err(EncodeError::DuplicateSnapshotSlot { slot });
        }
        output.push(slot);
        push_u16(output, item.sprite);
        output.push(item.dye_color);
    }
    crate::legend::encode(output, &profile.legend)?;
    push_u32(output, profile.inspected_tick_ms);
    Ok(())
}

pub(crate) fn decode_profile(reader: &mut PayloadReader<'_>) -> Result<PlayerProfile, DecodeError> {
    let identity = decode_identity(reader)?;
    let user_state = UserState::from_raw(reader.read_u8()?);
    let is_group_open = reader.read_bool()?;
    let count = usize::from(reader.read_u8()?);
    if count > MAX_PLAYER_EQUIPMENT_ITEMS {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_PLAYER_EQUIPMENT_ITEMS,
        });
    }
    let mut equipment: Vec<PlayerEquipmentItem> = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = reader.read_u8()?;
        if equipment.iter().any(|current| current.slot.raw() == slot) {
            return Err(DecodeError::DuplicateSnapshotSlot { slot });
        }
        equipment.push(PlayerEquipmentItem {
            slot: EquipmentSlot::from_raw(slot).ok_or(DecodeError::InvalidSnapshotSlot {
                slot,
                max: MAX_PLAYER_EQUIPMENT_ITEMS as u8,
            })?,
            sprite: reader.read_u16()?,
            dye_color: reader.read_u8()?,
        });
    }
    Ok(PlayerProfile {
        identity,
        user_state,
        is_group_open,
        equipment,
        legend: crate::legend::decode(reader)?,
        inspected_tick_ms: reader.read_u32()?,
    })
}

fn encode_string(output: &mut Vec<u8>, value: &str) -> Result<(), EncodeError> {
    if value.len() > MAX_PLAYER_IDENTITY_TEXT_LEN {
        return Err(EncodeError::SnapshotStringTooLong {
            length: value.len(),
            max: MAX_PLAYER_IDENTITY_TEXT_LEN,
        });
    }
    push_u16(
        output,
        u16::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_string(reader: &mut PayloadReader<'_>) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > MAX_PLAYER_IDENTITY_TEXT_LEN {
        return Err(DecodeError::SnapshotStringTooLong {
            length,
            max: MAX_PLAYER_IDENTITY_TEXT_LEN,
        });
    }
    String::from_utf8(reader.take(length)?.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
}
