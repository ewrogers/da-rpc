use super::*;

pub(super) fn encode_action(output: &mut Vec<u8>, update: ActionUpdate) {
    match update {
        ActionUpdate::ItemUsed { slot } => output.extend_from_slice(&[1, slot]),
        ActionUpdate::ItemDropped {
            slot,
            quantity,
            position,
        } => {
            output.extend_from_slice(&[2, slot]);
            push_u32(output, quantity);
            push_i32(output, position.x);
            push_i32(output, position.y);
        }
        ActionUpdate::ItemGiven {
            slot,
            quantity,
            object_id,
        } => {
            output.extend_from_slice(&[3, slot]);
            push_u32(output, quantity);
            push_u32(output, object_id);
        }
        ActionUpdate::GoldDropped { amount, position } => {
            output.push(4);
            push_u32(output, amount);
            push_i32(output, position.x);
            push_i32(output, position.y);
        }
        ActionUpdate::GoldGiven { amount, object_id } => {
            output.push(5);
            push_u32(output, amount);
            push_u32(output, object_id);
        }
        ActionUpdate::ItemPickedUp {
            destination_slot,
            position,
        } => {
            output.extend_from_slice(&[6, destination_slot]);
            push_i32(output, position.x);
            push_i32(output, position.y);
        }
        ActionUpdate::EquipmentUnequipped { slot } => {
            output.extend_from_slice(&[7, slot.raw()]);
        }
        ActionUpdate::Emoted { code } => output.extend_from_slice(&[8, code]),
        ActionUpdate::Turned { direction } => output.extend_from_slice(&[9, direction.raw()]),
        ActionUpdate::Resync { resync_id } => {
            output.push(10);
            push_u32(output, resync_id);
        }
        ActionUpdate::ResyncCompleted { resync_id } => {
            output.push(11);
            push_u32(output, resync_id);
        }
        ActionUpdate::ResyncTimedOut { resync_id } => {
            output.push(12);
            push_u32(output, resync_id);
        }
    }
}

pub(super) fn decode_action(reader: &mut PayloadReader<'_>) -> Result<ActionUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(ActionUpdate::ItemUsed {
            slot: reader.read_u8()?,
        }),
        2 => Ok(ActionUpdate::ItemDropped {
            slot: reader.read_u8()?,
            quantity: reader.read_u32()?,
            position: TilePosition {
                x: reader.read_i32()?,
                y: reader.read_i32()?,
            },
        }),
        3 => Ok(ActionUpdate::ItemGiven {
            slot: reader.read_u8()?,
            quantity: reader.read_u32()?,
            object_id: reader.read_u32()?,
        }),
        4 => Ok(ActionUpdate::GoldDropped {
            amount: reader.read_u32()?,
            position: TilePosition {
                x: reader.read_i32()?,
                y: reader.read_i32()?,
            },
        }),
        5 => Ok(ActionUpdate::GoldGiven {
            amount: reader.read_u32()?,
            object_id: reader.read_u32()?,
        }),
        6 => Ok(ActionUpdate::ItemPickedUp {
            destination_slot: reader.read_u8()?,
            position: TilePosition {
                x: reader.read_i32()?,
                y: reader.read_i32()?,
            },
        }),
        7 => {
            let actual = reader.read_u8()?;
            EquipmentSlot::from_raw(actual)
                .map(|slot| ActionUpdate::EquipmentUnequipped { slot })
                .ok_or(DecodeError::InvalidEquipmentSlot { actual })
        }
        8 => Ok(ActionUpdate::Emoted {
            code: reader.read_u8()?,
        }),
        9 => {
            let actual = reader.read_u8()?;
            Direction::from_raw(actual)
                .map(|direction| ActionUpdate::Turned { direction })
                .ok_or(DecodeError::InvalidDirection { actual })
        }
        10 => Ok(ActionUpdate::Resync {
            resync_id: decode_resync_id(reader)?,
        }),
        11 => Ok(ActionUpdate::ResyncCompleted {
            resync_id: decode_resync_id(reader)?,
        }),
        12 => Ok(ActionUpdate::ResyncTimedOut {
            resync_id: decode_resync_id(reader)?,
        }),
        actual => Err(DecodeError::InvalidStateUpdateType { actual }),
    }
}

fn decode_resync_id(reader: &mut PayloadReader<'_>) -> Result<u32, DecodeError> {
    let resync_id = reader.read_u32()?;
    if resync_id == 0 {
        return Err(DecodeError::InvalidCommandId);
    }
    Ok(resync_id)
}

pub(super) fn encode_ability(
    output: &mut Vec<u8>,
    update: &AbilityUpdate,
) -> Result<(), EncodeError> {
    let slot = match update {
        AbilityUpdate::SkillUsed { slot }
        | AbilityUpdate::SpellBegin { slot, .. }
        | AbilityUpdate::SpellChant { slot, .. }
        | AbilityUpdate::SpellCast { slot, .. }
        | AbilityUpdate::SpellCancelled { slot, .. } => *slot,
    };
    validate_ability_slot_encode(slot)?;
    match update {
        AbilityUpdate::SkillUsed { .. } => output.push(1),
        AbilityUpdate::SpellBegin { total_lines, .. } => {
            if *total_lines == 0 {
                return Err(EncodeError::InvalidSpellProgress { line: 0, total: 0 });
            }
            output.push(2);
            output.push(*total_lines);
        }
        AbilityUpdate::SpellChant {
            line, total_lines, ..
        } => {
            validate_spell_progress_encode(*line, *total_lines)?;
            output.push(3);
            output.push(*line);
            output.push(*total_lines);
        }
        AbilityUpdate::SpellCast { arguments, .. } => {
            output.push(4);
            encode_spell_arguments(output, arguments)?;
        }
        AbilityUpdate::SpellCancelled { source, .. } => {
            output.push(5);
            output.push(match source {
                SpellCancellationSource::Client => 1,
                SpellCancellationSource::Server => 2,
                SpellCancellationSource::Replaced => 3,
            });
        }
    }
    output.push(slot);
    Ok(())
}

pub(super) fn decode_ability(reader: &mut PayloadReader<'_>) -> Result<AbilityUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    let update = match kind {
        1 => AbilityUpdate::SkillUsed {
            slot: decode_ability_slot(reader)?,
        },
        2 => {
            let total_lines = reader.read_u8()?;
            if total_lines == 0 {
                return Err(DecodeError::InvalidSpellProgress { line: 0, total: 0 });
            }
            AbilityUpdate::SpellBegin {
                slot: decode_ability_slot(reader)?,
                total_lines,
            }
        }
        3 => {
            let line = reader.read_u8()?;
            let total_lines = reader.read_u8()?;
            validate_spell_progress_decode(line, total_lines)?;
            AbilityUpdate::SpellChant {
                slot: decode_ability_slot(reader)?,
                line,
                total_lines,
            }
        }
        4 => {
            let arguments = decode_spell_arguments(reader)?;
            AbilityUpdate::SpellCast {
                slot: decode_ability_slot(reader)?,
                arguments,
            }
        }
        5 => {
            let source = match reader.read_u8()? {
                1 => SpellCancellationSource::Client,
                2 => SpellCancellationSource::Server,
                3 => SpellCancellationSource::Replaced,
                actual => return Err(DecodeError::InvalidSpellCancellationSource { actual }),
            };
            AbilityUpdate::SpellCancelled {
                slot: decode_ability_slot(reader)?,
                source,
            }
        }
        actual => return Err(DecodeError::InvalidAbilityUpdateType { actual }),
    };
    Ok(update)
}

fn encode_spell_arguments(
    output: &mut Vec<u8>,
    arguments: &SpellCastArguments,
) -> Result<(), EncodeError> {
    match arguments {
        SpellCastArguments::Unknown => output.push(4),
        SpellCastArguments::None => output.push(0),
        SpellCastArguments::Target { id, x, y } => {
            output.push(1);
            push_bool(output, id.is_some());
            if let Some(id) = id {
                push_u32(output, *id);
            }
            push_i32(output, *x);
            push_i32(output, *y);
        }
        SpellCastArguments::Input(input) => {
            output.push(2);
            encode_event_string(output, input, crate::command::MAX_SPELL_INPUT_LEN)?;
        }
        SpellCastArguments::Values(values) => {
            if values.is_empty() || values.len() > 4 {
                return Err(EncodeError::InvalidSpellValues {
                    count: values.len(),
                });
            }
            output.push(3);
            output.push(values.len() as u8);
            for value in values {
                push_u16(output, *value);
            }
        }
    }
    Ok(())
}

fn decode_spell_arguments(
    reader: &mut PayloadReader<'_>,
) -> Result<SpellCastArguments, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(SpellCastArguments::None),
        1 => Ok(SpellCastArguments::Target {
            id: reader.read_bool()?.then(|| reader.read_u32()).transpose()?,
            x: reader.read_i32()?,
            y: reader.read_i32()?,
        }),
        2 => decode_event_string(reader, crate::command::MAX_SPELL_INPUT_LEN)
            .map(SpellCastArguments::Input),
        3 => {
            let count = usize::from(reader.read_u8()?);
            if !(1..=4).contains(&count) {
                return Err(DecodeError::InvalidSpellValues { count });
            }
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(reader.read_u16()?);
            }
            Ok(SpellCastArguments::Values(values))
        }
        4 => Ok(SpellCastArguments::Unknown),
        actual => Err(DecodeError::InvalidSpellCastArguments { actual }),
    }
}

fn validate_ability_slot_encode(slot: u8) -> Result<(), EncodeError> {
    if slot == 0 || slot > 90 {
        return Err(EncodeError::InvalidAbilitySlot { slot });
    }
    Ok(())
}

fn decode_ability_slot(reader: &mut PayloadReader<'_>) -> Result<u8, DecodeError> {
    let actual = reader.read_u8()?;
    if actual == 0 || actual > 90 {
        return Err(DecodeError::InvalidAbilitySlot { actual });
    }
    Ok(actual)
}

fn validate_spell_progress_encode(line: u8, total: u8) -> Result<(), EncodeError> {
    if total == 0 || line == 0 || line > total {
        return Err(EncodeError::InvalidSpellProgress { line, total });
    }
    Ok(())
}

fn validate_spell_progress_decode(line: u8, total: u8) -> Result<(), DecodeError> {
    if total == 0 || line == 0 || line > total {
        return Err(DecodeError::InvalidSpellProgress { line, total });
    }
    Ok(())
}
