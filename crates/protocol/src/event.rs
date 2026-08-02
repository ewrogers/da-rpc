use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
    snapshot::{MAX_MAP_NAME_LEN, decode_optional_string, encode_optional_string},
};
use darpc_model::{
    CharacterModifiers, CharacterStats, CoreStatus, CurrentVitals, Element, LocationUpdate,
    MapChange, ProgressionStatus, StateEvent, StateUpdate, StatusUpdate,
};

pub const MAX_EVENTS_PER_POLL: u16 = 192;
pub const MAX_EVENT_POLL_WAIT_MS: u16 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventPollRequest {
    pub request_id: u32,
    pub after_sequence: u32,
    pub max_events: u16,
    pub wait_ms: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventPollResponse {
    pub request_id: u32,
    pub result: EventPollResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventPollResult {
    Events(Vec<StateEvent>),
    ResyncRequired {
        missing_sequence: u32,
        latest_sequence: u32,
    },
}

pub(crate) fn encode_request(output: &mut Vec<u8>, request: EventPollRequest) {
    push_u32(output, request.request_id);
    push_u32(output, request.after_sequence);
    push_u16(output, request.max_events);
    push_u16(output, request.wait_ms);
}

pub(crate) fn decode_request(
    reader: &mut PayloadReader<'_>,
) -> Result<EventPollRequest, DecodeError> {
    Ok(EventPollRequest {
        request_id: reader.read_u32()?,
        after_sequence: reader.read_u32()?,
        max_events: reader.read_u16()?,
        wait_ms: reader.read_u16()?,
    })
}

pub(crate) fn encode_response(
    output: &mut Vec<u8>,
    response: &EventPollResponse,
) -> Result<(), EncodeError> {
    push_u32(output, response.request_id);
    match &response.result {
        EventPollResult::Events(events) => {
            let length = u16::try_from(events.len()).map_err(|_| EncodeError::LengthOverflow)?;
            if length > MAX_EVENTS_PER_POLL {
                return Err(EncodeError::EventBatchTooLong {
                    length: events.len(),
                    max: usize::from(MAX_EVENTS_PER_POLL),
                });
            }
            output.push(0);
            push_u16(output, length);
            for event in events {
                encode_event(output, event)?;
            }
        }
        EventPollResult::ResyncRequired {
            missing_sequence,
            latest_sequence,
        } => {
            output.push(1);
            push_u32(output, *missing_sequence);
            push_u32(output, *latest_sequence);
        }
    }
    Ok(())
}

pub(crate) fn decode_response(
    reader: &mut PayloadReader<'_>,
) -> Result<EventPollResponse, DecodeError> {
    let request_id = reader.read_u32()?;
    let result = match reader.read_u8()? {
        0 => {
            let length = reader.read_u16()?;
            if length > MAX_EVENTS_PER_POLL {
                return Err(DecodeError::EventBatchTooLong {
                    length: usize::from(length),
                    max: usize::from(MAX_EVENTS_PER_POLL),
                });
            }
            let mut events = Vec::with_capacity(usize::from(length));
            for _ in 0..length {
                events.push(decode_event(reader)?);
            }
            EventPollResult::Events(events)
        }
        1 => EventPollResult::ResyncRequired {
            missing_sequence: reader.read_u32()?,
            latest_sequence: reader.read_u32()?,
        },
        actual => return Err(DecodeError::InvalidEventPollStatus { actual }),
    };
    Ok(EventPollResponse { request_id, result })
}

fn encode_event(output: &mut Vec<u8>, event: &StateEvent) -> Result<(), EncodeError> {
    push_u32(output, event.sequence);
    push_u32(output, event.revision);
    push_u32(output, event.tick_ms);
    match &event.update {
        StateUpdate::Status(update) => {
            output.push(1);
            encode_status(output, *update);
        }
        StateUpdate::Location(update) => {
            output.push(2);
            encode_location(output, update)?;
        }
    }
    Ok(())
}

fn decode_event(reader: &mut PayloadReader<'_>) -> Result<StateEvent, DecodeError> {
    let sequence = reader.read_u32()?;
    let revision = reader.read_u32()?;
    let tick_ms = reader.read_u32()?;
    let update = match reader.read_u8()? {
        1 => StateUpdate::Status(decode_status(reader)?),
        2 => StateUpdate::Location(decode_location(reader)?),
        actual => return Err(DecodeError::InvalidStateUpdateType { actual }),
    };
    Ok(StateEvent {
        sequence,
        revision,
        tick_ms,
        update,
    })
}

fn encode_status(output: &mut Vec<u8>, update: StatusUpdate) {
    let mut fields = 0_u8;
    fields |= u8::from(update.core.is_some());
    fields |= u8::from(update.vitals.is_some()) << 1;
    fields |= u8::from(update.progression.is_some()) << 2;
    fields |= u8::from(update.gold.is_some()) << 3;
    fields |= u8::from(update.modifiers.is_some()) << 4;
    fields |= u8::from(update.is_blinded.is_some()) << 5;
    fields |= u8::from(update.is_action_restricted.is_some()) << 6;
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
}

fn decode_status(reader: &mut PayloadReader<'_>) -> Result<StatusUpdate, DecodeError> {
    let fields = reader.read_u8()?;
    if fields & !0x7F != 0 {
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
    })
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

fn encode_location(output: &mut Vec<u8>, update: &LocationUpdate) -> Result<(), EncodeError> {
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

fn decode_location(reader: &mut PayloadReader<'_>) -> Result<LocationUpdate, DecodeError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_status_update_is_detected() {
        assert!(StatusUpdate::default().is_empty());
    }
}
