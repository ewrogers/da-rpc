use super::{decode_optional_string, encode_optional_string};
use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
};
use darpc_model::{CreatureKind, Direction, WorldObject};

const MAX_WORLD_OBJECTS: usize = 512;
const MAX_OBJECT_NAME_LEN: usize = 63;

pub(super) fn encode(
    output: &mut Vec<u8>,
    objects: Option<&[WorldObject]>,
) -> Result<(), EncodeError> {
    push_bool(output, objects.is_some());
    let Some(objects) = objects else {
        return Ok(());
    };
    if objects.len() > MAX_WORLD_OBJECTS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: objects.len(),
            max: MAX_WORLD_OBJECTS,
        });
    }
    push_u16(
        output,
        u16::try_from(objects.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    for (index, object) in objects.iter().enumerate() {
        let id = object.id();
        if objects[..index].iter().any(|current| current.id() == id) {
            return Err(EncodeError::DuplicateWorldObjectId { id });
        }
        encode_object(output, object)?;
    }
    Ok(())
}

pub(crate) fn encode_object(output: &mut Vec<u8>, object: &WorldObject) -> Result<(), EncodeError> {
    match object {
        WorldObject::Player {
            id,
            name,
            x,
            y,
            direction,
            is_hidden,
            ..
        } => {
            output.push(1);
            push_u32(output, *id);
            push_i32(output, *x);
            push_i32(output, *y);
            output.push(direction.raw());
            push_bool(output, *is_hidden);
            encode_optional_string(output, name.as_deref(), MAX_OBJECT_NAME_LEN)?;
        }
        WorldObject::Creature {
            id,
            kind,
            sprite,
            name,
            x,
            y,
            direction,
        } => {
            output.push(2);
            push_u32(output, *id);
            push_i32(output, *x);
            push_i32(output, *y);
            output.push(direction.raw());
            output.push(match kind {
                CreatureKind::Monster => 1,
                CreatureKind::Npc => 2,
            });
            push_bool(output, sprite.is_some());
            if let Some(sprite) = sprite {
                push_u16(output, *sprite);
            }
            encode_optional_string(output, name.as_deref(), MAX_OBJECT_NAME_LEN)?;
        }
        WorldObject::Item {
            id,
            sprite,
            x,
            y,
            z_index,
        } => {
            output.push(3);
            push_u32(output, *id);
            push_i32(output, *x);
            push_i32(output, *y);
            push_u16(output, *sprite);
            push_u16(output, *z_index);
        }
    }
    Ok(())
}

pub(super) fn decode(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<Vec<WorldObject>>, DecodeError> {
    if !reader.read_bool()? {
        return Ok(None);
    }
    let count = usize::from(reader.read_u16()?);
    if count > MAX_WORLD_OBJECTS {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_WORLD_OBJECTS,
        });
    }
    let mut objects = Vec::with_capacity(count);
    for _ in 0..count {
        let object = decode_object(reader)?;
        let id = object.id();
        if objects
            .iter()
            .any(|current: &WorldObject| current.id() == id)
        {
            return Err(DecodeError::DuplicateWorldObjectId { id });
        }
        objects.push(object);
    }
    Ok(Some(objects))
}

pub(crate) fn decode_object(reader: &mut PayloadReader<'_>) -> Result<WorldObject, DecodeError> {
    let object_type = reader.read_u8()?;
    let id = reader.read_u32()?;
    let x = reader.read_i32()?;
    let y = reader.read_i32()?;
    let object = match object_type {
        1 => {
            let raw_direction = reader.read_u8()?;
            let direction =
                Direction::from_raw(raw_direction).ok_or(DecodeError::InvalidDirection {
                    actual: raw_direction,
                })?;
            let is_hidden = reader.read_bool()?;
            let name = decode_optional_string(reader, MAX_OBJECT_NAME_LEN)?;
            WorldObject::Player {
                id,
                name,
                x,
                y,
                direction,
                is_hidden,
                profile: None,
            }
        }
        2 => {
            let raw_direction = reader.read_u8()?;
            let direction =
                Direction::from_raw(raw_direction).ok_or(DecodeError::InvalidDirection {
                    actual: raw_direction,
                })?;
            let kind = match reader.read_u8()? {
                1 => CreatureKind::Monster,
                2 => CreatureKind::Npc,
                actual => return Err(DecodeError::InvalidCreatureKind { actual }),
            };
            let sprite = if reader.read_bool()? {
                Some(reader.read_u16()?)
            } else {
                None
            };
            WorldObject::Creature {
                id,
                kind,
                sprite,
                name: decode_optional_string(reader, MAX_OBJECT_NAME_LEN)?,
                x,
                y,
                direction,
            }
        }
        3 => WorldObject::Item {
            id,
            sprite: reader.read_u16()?,
            x,
            y,
            z_index: reader.read_u16()?,
        },
        actual => return Err(DecodeError::InvalidWorldObjectType { actual }),
    };
    Ok(object)
}
