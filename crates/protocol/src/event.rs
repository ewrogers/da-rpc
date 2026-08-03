use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
    snapshot::{MAX_MAP_NAME_LEN, decode_optional_string, encode_optional_string},
};
use darpc_model::{
    CharacterModifiers, CharacterStats, ClientMessage, CollectionChange, CoreStatus, CurrentVitals,
    Effect, EffectDuration, EffectUpdate, Element, InventoryItem, LocationUpdate, MapChange,
    MessageKind, ObjectUpdate, ProgressionStatus, Skill, SlotUpdate, Spell, StateEvent,
    StateUpdate, StatusUpdate,
};

pub const MAX_EVENTS_PER_POLL: u16 = 192;
pub const MAX_EVENT_POLL_WAIT_MS: u16 = 1_000;
pub const MAX_MESSAGE_NAME_LEN: usize = 15;
pub const MAX_MESSAGE_TEXT_LEN: usize = 4 * 1024;

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
        StateUpdate::Effect(update) => {
            output.push(3);
            encode_effect(output, *update);
        }
        StateUpdate::Object(update) => {
            output.push(4);
            encode_object_update(output, update)?;
        }
        StateUpdate::Message(message) => {
            output.push(5);
            encode_message(output, message)?;
        }
        StateUpdate::Inventory(update) => {
            output.push(6);
            encode_slot_update(
                output,
                update,
                crate::snapshot::collections::encode_inventory_item,
            )?;
        }
        StateUpdate::Spellbook(update) => {
            output.push(7);
            encode_slot_update(output, update, crate::snapshot::collections::encode_spell)?;
        }
        StateUpdate::Skillbook(update) => {
            output.push(8);
            encode_slot_update(output, update, crate::snapshot::collections::encode_skill)?;
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
        3 => StateUpdate::Effect(decode_effect(reader)?),
        4 => StateUpdate::Object(decode_object_update(reader)?),
        5 => StateUpdate::Message(decode_message(reader)?),
        6 => StateUpdate::Inventory(decode_slot_update(
            reader,
            crate::snapshot::collections::decode_inventory_item,
        )?),
        7 => StateUpdate::Spellbook(decode_slot_update(
            reader,
            crate::snapshot::collections::decode_spell,
        )?),
        8 => StateUpdate::Skillbook(decode_slot_update(
            reader,
            crate::snapshot::collections::decode_skill,
        )?),
        actual => return Err(DecodeError::InvalidStateUpdateType { actual }),
    };
    Ok(StateEvent {
        sequence,
        revision,
        tick_ms,
        update,
    })
}

fn encode_slot_update<T>(
    output: &mut Vec<u8>,
    update: &SlotUpdate<T>,
    encode_item: fn(&mut Vec<u8>, &T) -> Result<(), EncodeError>,
) -> Result<(), EncodeError> {
    if update.batch_count == 0 || update.batch_index >= update.batch_count {
        return Err(EncodeError::InvalidCollectionBatch {
            index: update.batch_index,
            count: update.batch_count,
        });
    }
    output.push(update.batch_index);
    output.push(update.batch_count);
    output.push(match update.change {
        CollectionChange::Added => 1,
        CollectionChange::Removed => 2,
        CollectionChange::Changed => 3,
    });
    output.push(update.slot);
    let fields = u8::from(update.before.is_some()) | (u8::from(update.after.is_some()) << 1);
    if fields == 0 {
        return Err(EncodeError::EmptyCollectionUpdate);
    }
    output.push(fields);
    if let Some(before) = &update.before {
        encode_item(output, before)?;
    }
    if let Some(after) = &update.after {
        encode_item(output, after)?;
    }
    Ok(())
}

fn decode_slot_update<T>(
    reader: &mut PayloadReader<'_>,
    decode_item: fn(&mut PayloadReader<'_>) -> Result<T, DecodeError>,
) -> Result<SlotUpdate<T>, DecodeError>
where
    T: CollectionSlot,
{
    let batch_index = reader.read_u8()?;
    let batch_count = reader.read_u8()?;
    if batch_count == 0 || batch_index >= batch_count {
        return Err(DecodeError::InvalidCollectionBatch {
            index: batch_index,
            count: batch_count,
        });
    }
    let change = match reader.read_u8()? {
        1 => CollectionChange::Added,
        2 => CollectionChange::Removed,
        3 => CollectionChange::Changed,
        actual => return Err(DecodeError::InvalidCollectionChange { actual }),
    };
    let slot = reader.read_u8()?;
    let fields = reader.read_u8()?;
    if fields == 0 || fields & !0x03 != 0 {
        return Err(DecodeError::InvalidCollectionFields { actual: fields });
    }
    let before = (fields & 0x01 != 0)
        .then(|| decode_item(reader))
        .transpose()?;
    let after = (fields & 0x02 != 0)
        .then(|| decode_item(reader))
        .transpose()?;
    if before.as_ref().is_some_and(|item| item.slot() != slot)
        || after.as_ref().is_some_and(|item| item.slot() != slot)
    {
        return Err(DecodeError::CollectionSlotMismatch { slot });
    }
    Ok(SlotUpdate {
        batch_index,
        batch_count,
        change,
        slot,
        before,
        after,
    })
}

trait CollectionSlot {
    fn slot(&self) -> u8;
}

impl CollectionSlot for InventoryItem {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl CollectionSlot for Spell {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl CollectionSlot for Skill {
    fn slot(&self) -> u8 {
        self.slot
    }
}

fn encode_message(output: &mut Vec<u8>, message: &ClientMessage) -> Result<(), EncodeError> {
    output.push(match message.kind {
        MessageKind::Say => 1,
        MessageKind::Shout => 2,
        MessageKind::Whisper => 3,
        MessageKind::Guild => 4,
        MessageKind::Group => 5,
        MessageKind::System => 6,
        MessageKind::World => 7,
    });
    encode_optional_message_name(output, message.sender.as_deref())?;
    encode_optional_message_name(output, message.recipient.as_deref())?;
    encode_event_string(output, &message.text, MAX_MESSAGE_TEXT_LEN)
}

fn decode_message(reader: &mut PayloadReader<'_>) -> Result<ClientMessage, DecodeError> {
    let kind = match reader.read_u8()? {
        1 => MessageKind::Say,
        2 => MessageKind::Shout,
        3 => MessageKind::Whisper,
        4 => MessageKind::Guild,
        5 => MessageKind::Group,
        6 => MessageKind::System,
        7 => MessageKind::World,
        actual => return Err(DecodeError::InvalidMessageKind { actual }),
    };
    Ok(ClientMessage {
        kind,
        sender: decode_optional_message_name(reader)?,
        recipient: decode_optional_message_name(reader)?,
        text: decode_event_string(reader, MAX_MESSAGE_TEXT_LEN)?,
    })
}

fn encode_optional_message_name(
    output: &mut Vec<u8>,
    value: Option<&str>,
) -> Result<(), EncodeError> {
    push_bool(output, value.is_some());
    if let Some(value) = value {
        encode_event_string(output, value, MAX_MESSAGE_NAME_LEN)?;
    }
    Ok(())
}

fn decode_optional_message_name(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<String>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_event_string(reader, MAX_MESSAGE_NAME_LEN))
        .transpose()
}

fn encode_event_string(output: &mut Vec<u8>, value: &str, max: usize) -> Result<(), EncodeError> {
    let bytes = value.as_bytes();
    if bytes.len() > max {
        return Err(EncodeError::EventStringTooLong {
            length: bytes.len(),
            max,
        });
    }
    let length = u16::try_from(bytes.len()).map_err(|_| EncodeError::LengthOverflow)?;
    push_u16(output, length);
    output.extend_from_slice(bytes);
    Ok(())
}

fn decode_event_string(reader: &mut PayloadReader<'_>, max: usize) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::EventStringTooLong { length, max });
    }
    let bytes = reader.take(length)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}

fn encode_object_update(output: &mut Vec<u8>, update: &ObjectUpdate) -> Result<(), EncodeError> {
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
        ObjectUpdate::Cleared => {
            output.push(5);
            return Ok(());
        }
    };
    crate::snapshot::objects::encode_object(output, object)
}

fn decode_object_update(reader: &mut PayloadReader<'_>) -> Result<ObjectUpdate, DecodeError> {
    let kind = reader.read_u8()?;
    if kind == 5 {
        return Ok(ObjectUpdate::Cleared);
    }
    let object = crate::snapshot::objects::decode_object(reader)?;
    match kind {
        1 => Ok(ObjectUpdate::Appeared(object)),
        2 => Ok(ObjectUpdate::Disappeared(object)),
        3 => Ok(ObjectUpdate::Moved(object)),
        4 => Ok(ObjectUpdate::DirectionChanged(object)),
        actual => Err(DecodeError::InvalidObjectUpdateType { actual }),
    }
}

fn encode_effect(output: &mut Vec<u8>, update: EffectUpdate) {
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

fn decode_effect(reader: &mut PayloadReader<'_>) -> Result<EffectUpdate, DecodeError> {
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
