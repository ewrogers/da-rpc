use super::*;

pub(super) fn encode_object_update(
    output: &mut Vec<u8>,
    update: &ObjectUpdate,
) -> Result<(), EncodeError> {
    let object = match update {
        ObjectUpdate::Appeared(object) => {
            output.push(1);
            object
        }
        ObjectUpdate::Disappeared(object) => {
            output.push(2);
            object
        }
        ObjectUpdate::Moved(object) => {
            output.push(3);
            object
        }
        ObjectUpdate::DirectionChanged(object) => {
            output.push(4);
            object
        }
    };
    crate::snapshot::objects::encode_object(output, object)
}

pub(super) fn decode_object_update(
    reader: &mut PayloadReader<'_>,
) -> Result<ObjectUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    if !(1..=4).contains(&kind) {
        return Err(DecodeError::InvalidObjectUpdateType { actual: kind });
    }
    let object = crate::snapshot::objects::decode_object(reader)?;
    match kind {
        1 => Ok(ObjectUpdate::Appeared(object)),
        2 => Ok(ObjectUpdate::Disappeared(object)),
        3 => Ok(ObjectUpdate::Moved(object)),
        4 => Ok(ObjectUpdate::DirectionChanged(object)),
        _ => unreachable!("validated object update type is matched"),
    }
}

pub(super) fn encode_entity(
    output: &mut Vec<u8>,
    update: &EntityUpdate,
) -> Result<(), EncodeError> {
    match update {
        EntityUpdate::Animated {
            entity,
            animation,
            duration_10ms,
        } => {
            output.push(1);
            crate::snapshot::objects::encode_object(output, entity)?;
            output.push(*animation);
            push_u16(output, *duration_10ms);
        }
        EntityUpdate::Effect {
            entity,
            effect,
            source,
            frame_interval_ms,
        } => {
            output.push(2);
            crate::snapshot::objects::encode_object(output, entity)?;
            push_u16(output, *effect);
            push_bool(output, source.is_some());
            if let Some(source) = source {
                crate::snapshot::objects::encode_object(output, source)?;
            }
            push_bool(output, frame_interval_ms.is_some());
            if let Some(frame_interval_ms) = frame_interval_ms {
                push_u16(output, *frame_interval_ms as u16);
            }
        }
        EntityUpdate::Damaged {
            entity,
            health_percent,
        } => {
            if *health_percent > 100 {
                return Err(EncodeError::InvalidHealthPercent {
                    actual: *health_percent,
                });
            }
            output.push(3);
            crate::snapshot::objects::encode_object(output, entity)?;
            output.push(*health_percent);
        }
    }
    Ok(())
}

pub(super) fn decode_entity(reader: &mut PayloadReader<'_>) -> Result<EntityUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    let entity = crate::snapshot::objects::decode_object(reader)?;
    match kind {
        1 => Ok(EntityUpdate::Animated {
            entity,
            animation: reader.read_u8()?,
            duration_10ms: reader.read_u16()?,
        }),
        2 => Ok(EntityUpdate::Effect {
            entity,
            effect: reader.read_u16()?,
            source: reader
                .read_bool()?
                .then(|| crate::snapshot::objects::decode_object(reader))
                .transpose()?,
            frame_interval_ms: reader
                .read_bool()?
                .then(|| reader.read_u16().map(|value| value as i16))
                .transpose()?,
        }),
        3 => {
            let health_percent = reader.read_u8()?;
            if health_percent > 100 {
                return Err(DecodeError::InvalidHealthPercent {
                    actual: health_percent,
                });
            }
            Ok(EntityUpdate::Damaged {
                entity,
                health_percent,
            })
        }
        actual => Err(DecodeError::InvalidEntityUpdateType { actual }),
    }
}
