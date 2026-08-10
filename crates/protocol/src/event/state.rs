use super::*;

pub(super) fn encode_effect(output: &mut Vec<u8>, update: EffectUpdate) {
    match update {
        EffectUpdate::Added(effect) => {
            output.push(1);
            push_u16(output, effect.icon);
            output.push(effect.duration.raw());
        }
        EffectUpdate::Removed { icon } => {
            output.push(2);
            push_u16(output, icon);
        }
        EffectUpdate::Changed(effect) => {
            output.push(3);
            push_u16(output, effect.icon);
            output.push(effect.duration.raw());
        }
    }
}

pub(super) fn decode_effect(reader: &mut PayloadReader<'_>) -> Result<EffectUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    match kind {
        1 | 3 => {
            let icon = reader.read_u16()?;
            let duration = reader.read_u8()?;
            let effect = Effect {
                icon,
                duration: EffectDuration::from_raw(duration)
                    .ok_or(DecodeError::InvalidEffectDuration { actual: duration })?,
            };
            Ok(if kind == 1 {
                EffectUpdate::Added(effect)
            } else {
                EffectUpdate::Changed(effect)
            })
        }
        2 => Ok(EffectUpdate::Removed {
            icon: reader.read_u16()?,
        }),
        actual => Err(DecodeError::InvalidEffectUpdateType { actual }),
    }
}

pub(super) fn encode_status(output: &mut Vec<u8>, update: StatusUpdate) {
    let mut fields = 0_u8;
    fields |= u8::from(update.core.is_some());
    fields |= u8::from(update.vitals.is_some()) << 1;
    fields |= u8::from(update.progression.is_some()) << 2;
    fields |= u8::from(update.gold.is_some()) << 3;
    fields |= u8::from(update.modifiers.is_some()) << 4;
    fields |= u8::from(update.is_blinded.is_some()) << 5;
    fields |= u8::from(update.is_action_restricted.is_some()) << 6;
    fields |= u8::from(update.is_casting.is_some()) << 7;
    output.push(fields);
    if let Some(core) = update.core {
        output.push(core.level);
        output.push(core.ability_level);
        push_u32(output, core.max_health);
        push_u32(output, core.max_mana);
        push_u32(output, core.weight);
        push_u32(output, core.max_weight);
        encode_stats(output, core.stats);
    }
    if let Some(vitals) = update.vitals {
        push_u32(output, vitals.health);
        push_u32(output, vitals.mana);
    }
    if let Some(progression) = update.progression {
        push_u32(output, progression.experience);
        push_u32(output, progression.ability_points);
        push_u32(output, progression.experience_to_next_level);
        push_u32(output, progression.ability_to_next_level);
    }
    if let Some(gold) = update.gold {
        push_u32(output, gold);
    }
    if let Some(modifiers) = update.modifiers {
        output.push(modifiers.armor_class as u8);
        output.push(modifiers.damage);
        output.push(modifiers.hit);
        push_u16(output, modifiers.magic_resistance);
        push_u16(output, modifiers.attack_element.raw());
        push_u16(output, modifiers.defense_element.raw());
    }
    if let Some(is_blinded) = update.is_blinded {
        push_bool(output, is_blinded);
    }
    if let Some(is_action_restricted) = update.is_action_restricted {
        push_bool(output, is_action_restricted);
    }
    if let Some(is_casting) = update.is_casting {
        push_bool(output, is_casting);
    }
}

pub(super) fn decode_status(reader: &mut PayloadReader<'_>) -> Result<StatusUpdate, DecodeError> {
    let fields = reader.read_u8()?;
    if fields == 0 {
        return Err(DecodeError::InvalidStatusFields { actual: fields });
    }
    Ok(StatusUpdate {
        core: (fields & 0x01 != 0)
            .then(|| decode_core(reader))
            .transpose()?,
        vitals: (fields & 0x02 != 0)
            .then(|| decode_vitals(reader))
            .transpose()?,
        progression: (fields & 0x04 != 0)
            .then(|| decode_progression(reader))
            .transpose()?,
        gold: (fields & 0x08 != 0)
            .then(|| reader.read_u32())
            .transpose()?,
        modifiers: (fields & 0x10 != 0)
            .then(|| decode_modifiers(reader))
            .transpose()?,
        is_blinded: (fields & 0x20 != 0)
            .then(|| reader.read_bool())
            .transpose()?,
        is_action_restricted: (fields & 0x40 != 0)
            .then(|| reader.read_bool())
            .transpose()?,
        is_casting: (fields & 0x80 != 0)
            .then(|| reader.read_bool())
            .transpose()?,
    })
}

pub(super) fn encode_movement(
    output: &mut Vec<u8>,
    update: MovementUpdate,
) -> Result<(), EncodeError> {
    match update {
        MovementUpdate::Started {
            current,
            destination,
        } => {
            output.push(1);
            encode_tile_position(output, current);
            encode_destination(output, destination);
        }
        MovementUpdate::Stopped {
            current,
            destination,
            reached_destination,
        } => {
            if destination.is_some() != reached_destination.is_some() {
                return Err(EncodeError::InvalidMovementOutcome);
            }
            output.push(2);
            encode_tile_position(output, current);
            encode_destination(output, destination);
            output.push(match reached_destination {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
        }
    }
    Ok(())
}

pub(super) fn decode_movement(
    reader: &mut PayloadReader<'_>,
) -> Result<MovementUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    let current = decode_tile_position(reader)?;
    let destination = decode_destination(reader)?;
    match kind {
        1 => Ok(MovementUpdate::Started {
            current,
            destination,
        }),
        2 => {
            let actual = reader.read_u8()?;
            let reached_destination = match (destination.is_some(), actual) {
                (false, 0) => None,
                (true, 1) => Some(false),
                (true, 2) => Some(true),
                (has_destination, actual) => {
                    return Err(DecodeError::InvalidMovementOutcome {
                        actual,
                        has_destination,
                    });
                }
            };
            Ok(MovementUpdate::Stopped {
                current,
                destination,
                reached_destination,
            })
        }
        actual => Err(DecodeError::InvalidMovementUpdateType { actual }),
    }
}

fn encode_tile_position(output: &mut Vec<u8>, position: TilePosition) {
    push_i32(output, position.x);
    push_i32(output, position.y);
}

fn decode_tile_position(reader: &mut PayloadReader<'_>) -> Result<TilePosition, DecodeError> {
    Ok(TilePosition {
        x: reader.read_i32()?,
        y: reader.read_i32()?,
    })
}

fn encode_destination(output: &mut Vec<u8>, destination: Option<TilePosition>) {
    push_bool(output, destination.is_some());
    if let Some(destination) = destination {
        encode_tile_position(output, destination);
    }
}

fn decode_destination(reader: &mut PayloadReader<'_>) -> Result<Option<TilePosition>, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => decode_tile_position(reader).map(Some),
        actual => Err(DecodeError::InvalidMovementDestination { actual }),
    }
}

fn encode_stats(output: &mut Vec<u8>, stats: CharacterStats) {
    push_u16(output, stats.strength);
    push_u16(output, stats.intelligence);
    push_u16(output, stats.wisdom);
    push_u16(output, stats.constitution);
    push_u16(output, stats.dexterity);
}

fn decode_core(reader: &mut PayloadReader<'_>) -> Result<CoreStatus, DecodeError> {
    Ok(CoreStatus {
        level: reader.read_u8()?,
        ability_level: reader.read_u8()?,
        max_health: reader.read_u32()?,
        max_mana: reader.read_u32()?,
        weight: reader.read_u32()?,
        max_weight: reader.read_u32()?,
        stats: CharacterStats {
            strength: reader.read_u16()?,
            intelligence: reader.read_u16()?,
            wisdom: reader.read_u16()?,
            constitution: reader.read_u16()?,
            dexterity: reader.read_u16()?,
        },
    })
}

pub(super) fn encode_location(
    output: &mut Vec<u8>,
    update: &LocationUpdate,
) -> Result<(), EncodeError> {
    push_i32(output, update.x);
    push_i32(output, update.y);
    push_bool(output, update.map.is_some());
    if let Some(map) = &update.map {
        push_u32(output, map.id);
        encode_optional_string(output, map.name.as_deref(), MAX_MAP_NAME_LEN)?;
        push_i32(output, map.width);
        push_i32(output, map.height);
    }
    Ok(())
}

pub(super) fn decode_location(
    reader: &mut PayloadReader<'_>,
) -> Result<LocationUpdate, DecodeError> {
    let x = reader.read_i32()?;
    let y = reader.read_i32()?;
    let map = if reader.read_bool()? {
        Some(MapChange {
            id: reader.read_u32()?,
            name: decode_optional_string(reader, MAX_MAP_NAME_LEN)?,
            width: reader.read_i32()?,
            height: reader.read_i32()?,
        })
    } else {
        None
    };
    Ok(LocationUpdate { x, y, map })
}

fn decode_vitals(reader: &mut PayloadReader<'_>) -> Result<CurrentVitals, DecodeError> {
    Ok(CurrentVitals {
        health: reader.read_u32()?,
        mana: reader.read_u32()?,
    })
}

fn decode_progression(reader: &mut PayloadReader<'_>) -> Result<ProgressionStatus, DecodeError> {
    Ok(ProgressionStatus {
        experience: reader.read_u32()?,
        ability_points: reader.read_u32()?,
        experience_to_next_level: reader.read_u32()?,
        ability_to_next_level: reader.read_u32()?,
    })
}

fn decode_modifiers(reader: &mut PayloadReader<'_>) -> Result<CharacterModifiers, DecodeError> {
    Ok(CharacterModifiers {
        armor_class: reader.read_i8()?,
        damage: reader.read_u8()?,
        hit: reader.read_u8()?,
        magic_resistance: reader.read_u16()?,
        attack_element: Element::from_raw(reader.read_u16()?),
        defense_element: Element::from_raw(reader.read_u16()?),
    })
}
