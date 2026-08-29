use super::{decode_optional_string, encode_optional_string};
use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
};
use darpc_model::{
    CreatureKind, Direction, HumanVisual, MAX_WORLD_OBJECTS, PlayerVisual, WorldObject,
};

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
            visual,
            ..
        } => {
            output.push(1);
            push_u32(output, *id);
            push_i32(output, *x);
            push_i32(output, *y);
            output.push(direction.raw());
            push_bool(output, *is_hidden);
            push_bool(output, visual.is_some());
            if let Some(visual) = visual {
                encode_player_visual(output, visual);
            }
            encode_optional_string(output, name.as_deref(), MAX_OBJECT_NAME_LEN)?;
        }
        WorldObject::Creature {
            id,
            kind,
            is_solid,
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
            output.push(match (kind, is_solid) {
                (CreatureKind::Monster, true) => 1,
                (CreatureKind::Npc, _) => 2,
                (CreatureKind::Monster, false) => 3,
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
            dye_color,
            x,
            y,
            z_index,
        } => {
            output.push(3);
            push_u32(output, *id);
            push_i32(output, *x);
            push_i32(output, *y);
            push_u16(output, *sprite);
            output.push(*dye_color);
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
            let visual = reader
                .read_bool()?
                .then(|| decode_player_visual(reader))
                .transpose()?;
            let name = decode_optional_string(reader, MAX_OBJECT_NAME_LEN)?;
            WorldObject::Player {
                id,
                name,
                x,
                y,
                direction,
                is_hidden,
                visual,
                profile: None,
            }
        }
        2 => {
            let raw_direction = reader.read_u8()?;
            let direction =
                Direction::from_raw(raw_direction).ok_or(DecodeError::InvalidDirection {
                    actual: raw_direction,
                })?;
            let (kind, is_solid) = match reader.read_u8()? {
                1 => (CreatureKind::Monster, true),
                2 => (CreatureKind::Npc, true),
                3 => (CreatureKind::Monster, false),
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
                is_solid,
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
            dye_color: reader.read_u8()?,
            x,
            y,
            z_index: reader.read_u16()?,
        },
        actual => return Err(DecodeError::InvalidWorldObjectType { actual }),
    };
    Ok(object)
}

fn encode_player_visual(output: &mut Vec<u8>, visual: &PlayerVisual) {
    match visual {
        PlayerVisual::Human(visual) => {
            output.push(1);
            output.push(visual.gender.raw());
            push_u16(output, visual.head_sprite);
            push_u16(output, visual.body_sprite);
            push_u16(output, visual.arms_sprite);
            push_u16(output, visual.boots_sprite);
            push_u16(output, visual.pants_sprite);
            push_u16(output, visual.armor_sprite);
            push_u16(output, visual.weapon_sprite);
            push_u16(output, visual.shield_sprite);
            push_u16(output, visual.overcoat_sprite);
            push_u16(output, visual.accessory1_sprite);
            push_u16(output, visual.accessory2_sprite);
            push_u16(output, visual.accessory3_sprite);
            output.push(visual.hair_color);
            output.push(visual.skin_color);
            output.push(visual.boots_color);
            output.push(visual.pants_color);
            output.push(visual.overcoat_color);
            output.push(visual.accessory1_color);
            output.push(visual.accessory2_color);
            output.push(visual.accessory3_color);
            output.push(visual.rest_position);
            output.push(visual.face_shape);
            push_bool(output, visual.is_translucent);
        }
        PlayerVisual::Creature {
            sprite,
            color,
            boots_color,
            pants_color,
        } => {
            output.push(2);
            push_u16(output, *sprite);
            output.push(*color);
            output.push(*boots_color);
            output.push(*pants_color);
        }
    }
}

fn decode_player_visual(reader: &mut PayloadReader<'_>) -> Result<PlayerVisual, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(PlayerVisual::Human(HumanVisual {
            gender: darpc_model::Gender::from_raw(reader.read_u8()?),
            head_sprite: reader.read_u16()?,
            body_sprite: reader.read_u16()?,
            arms_sprite: reader.read_u16()?,
            boots_sprite: reader.read_u16()?,
            pants_sprite: reader.read_u16()?,
            armor_sprite: reader.read_u16()?,
            weapon_sprite: reader.read_u16()?,
            shield_sprite: reader.read_u16()?,
            overcoat_sprite: reader.read_u16()?,
            accessory1_sprite: reader.read_u16()?,
            accessory2_sprite: reader.read_u16()?,
            accessory3_sprite: reader.read_u16()?,
            hair_color: reader.read_u8()?,
            skin_color: reader.read_u8()?,
            boots_color: reader.read_u8()?,
            pants_color: reader.read_u8()?,
            overcoat_color: reader.read_u8()?,
            accessory1_color: reader.read_u8()?,
            accessory2_color: reader.read_u8()?,
            accessory3_color: reader.read_u8()?,
            rest_position: reader.read_u8()?,
            face_shape: reader.read_u8()?,
            is_translucent: reader.read_bool()?,
        })),
        2 => Ok(PlayerVisual::Creature {
            sprite: reader.read_u16()?,
            color: reader.read_u8()?,
            boots_color: reader.read_u8()?,
            pants_color: reader.read_u8()?,
        }),
        actual => Err(DecodeError::InvalidPlayerVisualType { actual }),
    }
}
