use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
    snapshot::{MAX_MAP_NAME_LEN, decode_optional_string, encode_optional_string},
};
use darpc_model::{
    AbilityUpdate, ActionUpdate, CharacterModifiers, CharacterStats, ClientMessage,
    CollectionChange, CoreStatus, CurrentVitals, Direction, Effect, EffectDuration, EffectUpdate,
    Element, EntityUpdate, EquipmentSlot, InventoryItem, LocationUpdate, MapChange, MessageKind,
    MovementUpdate, ObjectUpdate, ProgressionStatus, Skill, SlotUpdate, Spell,
    SpellCancellationSource, SpellCastArguments, StateEvent, StateUpdate, StatusUpdate,
    TilePosition,
};

mod action;
mod collection;
mod message;
mod object;
mod state;

use action::{decode_ability, decode_action, encode_ability, encode_action};
use collection::{decode_slot_update, encode_slot_update};
use message::{decode_event_string, decode_message, encode_event_string, encode_message};
use object::{decode_entity, decode_object_update, encode_entity, encode_object_update};
use state::{
    decode_effect, decode_location, decode_movement, decode_status, encode_effect, encode_location,
    encode_movement, encode_status,
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
        StateUpdate::Movement(update) => {
            output.push(9);
            encode_movement(output, *update)?;
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
        StateUpdate::Ability(update) => {
            output.push(10);
            encode_ability(output, update)?;
        }
        StateUpdate::Action(update) => {
            output.push(11);
            encode_action(output, *update);
        }
        StateUpdate::Entity(update) => {
            output.push(12);
            encode_entity(output, update)?;
        }
        StateUpdate::Dialog(update) => {
            output.push(13);
            crate::dialog::encode_update(output, update)?;
        }
        StateUpdate::Group(update) => {
            output.push(14);
            crate::group::encode_update(output, update)?;
        }
        StateUpdate::Exchange(update) => {
            output.push(15);
            crate::exchange::encode_update(output, update)?;
        }
        StateUpdate::Legend(update) => {
            output.push(16);
            crate::legend::encode_update(output, update)?;
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
        9 => StateUpdate::Movement(decode_movement(reader)?),
        10 => StateUpdate::Ability(decode_ability(reader)?),
        11 => StateUpdate::Action(decode_action(reader)?),
        12 => StateUpdate::Entity(decode_entity(reader)?),
        13 => StateUpdate::Dialog(crate::dialog::decode_update(reader)?),
        14 => StateUpdate::Group(crate::group::decode_update(reader)?),
        15 => StateUpdate::Exchange(crate::exchange::decode_update(reader)?),
        16 => StateUpdate::Legend(crate::legend::decode_update(reader)?),
        actual => return Err(DecodeError::InvalidStateUpdateType { actual }),
    };
    Ok(StateEvent {
        sequence,
        revision,
        tick_ms,
        update,
    })
}

#[cfg(test)]
mod tests;
