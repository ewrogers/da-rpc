use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16, push_u32},
};
use darpc_model::{
    DialogChoice, DialogCloseReason, DialogInput, DialogInteraction, DialogItem, DialogKind,
    DialogNavigation, DialogSlot, DialogSpeaker, DialogSpriteType, DialogState, DialogSubmission,
    DialogTarget, DialogUpdate,
};

const MAX_DIALOG_STRING_BYTES: usize = 4 * 1024;
const MAX_DIALOG_ROWS: usize = 512;

pub(crate) fn encode_update(
    output: &mut Vec<u8>,
    update: &DialogUpdate,
) -> Result<(), EncodeError> {
    match update {
        DialogUpdate::Opened(state) => {
            output.push(1);
            encode_state(output, state)?;
        }
        DialogUpdate::Changed(state) => {
            output.push(2);
            encode_state(output, state)?;
        }
        DialogUpdate::Submitted {
            state,
            previous_revision,
            submission,
        } => {
            output.push(3);
            encode_state(output, state)?;
            push_u32(output, *previous_revision);
            encode_submission(output, submission)?;
        }
        DialogUpdate::Closed { previous, reason } => {
            output.push(4);
            push_bool(output, previous.is_some());
            if let Some(previous) = previous {
                encode_state(output, previous)?;
            }
            output.push(close_reason(*reason));
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<DialogUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(DialogUpdate::Opened(decode_state(reader)?)),
        2 => Ok(DialogUpdate::Changed(decode_state(reader)?)),
        3 => Ok(DialogUpdate::Submitted {
            state: decode_state(reader)?,
            previous_revision: reader.read_u32()?,
            submission: decode_submission(reader)?,
        }),
        4 => Ok(DialogUpdate::Closed {
            previous: reader
                .read_bool()?
                .then(|| decode_state(reader))
                .transpose()?,
            reason: decode_close_reason(reader.read_u8()?)?,
        }),
        actual => Err(DecodeError::InvalidDialogField { actual }),
    }
}

pub(crate) fn encode_optional_state(
    output: &mut Vec<u8>,
    state: Option<&DialogState>,
) -> Result<(), EncodeError> {
    push_bool(output, state.is_some());
    if let Some(state) = state {
        encode_state(output, state)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_state(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<DialogState>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_state(reader))
        .transpose()
}

fn encode_state(output: &mut Vec<u8>, state: &DialogState) -> Result<(), EncodeError> {
    push_u32(output, state.revision);
    output.push(match state.kind {
        DialogKind::Merchant => 1,
        DialogKind::Pursuit => 2,
    });
    push_u32(output, state.target.id);
    encode_speaker(output, &state.speaker)?;
    encode_optional_string(output, state.content.as_deref())?;
    push_bool(output, state.response_pending);
    push_bool(output, state.navigation.previous);
    push_bool(output, state.navigation.next);
    push_bool(output, state.navigation.close);
    encode_interaction(output, &state.interaction)
}

fn decode_state(reader: &mut PayloadReader<'_>) -> Result<DialogState, DecodeError> {
    let revision = reader.read_u32()?;
    let kind = match reader.read_u8()? {
        1 => DialogKind::Merchant,
        2 => DialogKind::Pursuit,
        actual => return Err(DecodeError::InvalidDialogField { actual }),
    };
    Ok(DialogState {
        revision,
        kind,
        target: DialogTarget {
            id: reader.read_u32()?,
        },
        speaker: decode_speaker(reader)?,
        content: decode_optional_string(reader)?,
        response_pending: reader.read_bool()?,
        navigation: DialogNavigation {
            previous: reader.read_bool()?,
            next: reader.read_bool()?,
            close: reader.read_bool()?,
        },
        interaction: decode_interaction(reader)?,
    })
}

fn encode_speaker(output: &mut Vec<u8>, speaker: &DialogSpeaker) -> Result<(), EncodeError> {
    encode_optional_string(output, speaker.name.as_deref())?;
    push_u16(output, speaker.sprite);
    output.push(match speaker.sprite_type {
        DialogSpriteType::Creature => 1,
        DialogSpriteType::Item => 2,
        DialogSpriteType::Unknown => 0,
    });
    output.push(speaker.color);
    push_bool(output, speaker.show_graphic);
    Ok(())
}

fn decode_speaker(reader: &mut PayloadReader<'_>) -> Result<DialogSpeaker, DecodeError> {
    let name = decode_optional_string(reader)?;
    let sprite = reader.read_u16()?;
    let sprite_type = match reader.read_u8()? {
        0 => DialogSpriteType::Unknown,
        1 => DialogSpriteType::Creature,
        2 => DialogSpriteType::Item,
        actual => return Err(DecodeError::InvalidDialogField { actual }),
    };
    Ok(DialogSpeaker {
        name,
        sprite,
        sprite_type,
        color: reader.read_u8()?,
        show_graphic: reader.read_bool()?,
    })
}

fn encode_interaction(output: &mut Vec<u8>, value: &DialogInteraction) -> Result<(), EncodeError> {
    match value {
        DialogInteraction::Message => output.push(0),
        DialogInteraction::Choices(rows) => {
            output.push(1);
            encode_choices(output, rows)?;
        }
        DialogInteraction::Input(input) => {
            output.push(2);
            encode_input(output, input)?;
        }
        DialogInteraction::Items(rows) => {
            output.push(3);
            encode_items(output, rows)?;
        }
        DialogInteraction::Inventory(rows) => {
            output.push(4);
            encode_slots(output, rows)?;
        }
        DialogInteraction::Spells(rows) => {
            output.push(5);
            encode_slots(output, rows)?;
        }
        DialogInteraction::Skills(rows) => {
            output.push(6);
            encode_slots(output, rows)?;
        }
        DialogInteraction::Protected => output.push(7),
        DialogInteraction::Unsupported => output.push(8),
    }
    Ok(())
}

fn decode_interaction(reader: &mut PayloadReader<'_>) -> Result<DialogInteraction, DecodeError> {
    Ok(match reader.read_u8()? {
        0 => DialogInteraction::Message,
        1 => DialogInteraction::Choices(decode_choices(reader)?),
        2 => DialogInteraction::Input(decode_input(reader)?),
        3 => DialogInteraction::Items(decode_items(reader)?),
        4 => DialogInteraction::Inventory(decode_slots(reader)?),
        5 => DialogInteraction::Spells(decode_slots(reader)?),
        6 => DialogInteraction::Skills(decode_slots(reader)?),
        7 => DialogInteraction::Protected,
        8 => DialogInteraction::Unsupported,
        actual => return Err(DecodeError::InvalidDialogField { actual }),
    })
}

fn encode_choices(output: &mut Vec<u8>, rows: &[DialogChoice]) -> Result<(), EncodeError> {
    encode_count(output, rows.len())?;
    for row in rows {
        push_u16(output, row.index);
        encode_string(output, &row.text)?;
    }
    Ok(())
}

fn decode_choices(reader: &mut PayloadReader<'_>) -> Result<Vec<DialogChoice>, DecodeError> {
    let count = decode_count(reader)?;
    (0..count)
        .map(|_| {
            Ok(DialogChoice {
                index: reader.read_u16()?,
                text: decode_string(reader)?,
            })
        })
        .collect()
}

fn encode_input(output: &mut Vec<u8>, input: &DialogInput) -> Result<(), EncodeError> {
    encode_optional_string(output, input.prolog.as_deref())?;
    output.push(input.maximum_bytes);
    encode_optional_string(output, input.epilog.as_deref())
}

fn decode_input(reader: &mut PayloadReader<'_>) -> Result<DialogInput, DecodeError> {
    Ok(DialogInput {
        prolog: decode_optional_string(reader)?,
        maximum_bytes: reader.read_u8()?,
        epilog: decode_optional_string(reader)?,
    })
}

fn encode_items(output: &mut Vec<u8>, rows: &[DialogItem]) -> Result<(), EncodeError> {
    encode_count(output, rows.len())?;
    for row in rows {
        push_u16(output, row.index);
        push_u16(output, row.sprite);
        output.push(row.color);
        encode_optional_string(output, row.name.as_deref())?;
        encode_optional_string(output, row.description.as_deref())?;
        encode_optional_u32(output, row.value);
        push_bool(output, row.available_quantity.is_some());
        if let Some(value) = row.available_quantity {
            output.push(value);
        }
    }
    Ok(())
}

fn decode_items(reader: &mut PayloadReader<'_>) -> Result<Vec<DialogItem>, DecodeError> {
    let count = decode_count(reader)?;
    (0..count)
        .map(|_| {
            Ok(DialogItem {
                index: reader.read_u16()?,
                sprite: reader.read_u16()?,
                color: reader.read_u8()?,
                name: decode_optional_string(reader)?,
                description: decode_optional_string(reader)?,
                value: decode_optional_u32(reader)?,
                available_quantity: reader.read_bool()?.then(|| reader.read_u8()).transpose()?,
            })
        })
        .collect()
}

fn encode_slots(output: &mut Vec<u8>, rows: &[DialogSlot]) -> Result<(), EncodeError> {
    encode_count(output, rows.len())?;
    for row in rows {
        push_u16(output, row.index);
        output.push(row.slot);
        encode_optional_u32(output, row.value);
        encode_optional_string(output, row.name.as_deref())?;
        push_bool(output, row.sprite.is_some());
        if let Some(value) = row.sprite {
            push_u16(output, value);
        }
        push_bool(output, row.color.is_some());
        if let Some(value) = row.color {
            output.push(value);
        }
    }
    Ok(())
}

fn decode_slots(reader: &mut PayloadReader<'_>) -> Result<Vec<DialogSlot>, DecodeError> {
    let count = decode_count(reader)?;
    (0..count)
        .map(|_| {
            Ok(DialogSlot {
                index: reader.read_u16()?,
                slot: reader.read_u8()?,
                value: decode_optional_u32(reader)?,
                name: decode_optional_string(reader)?,
                sprite: reader.read_bool()?.then(|| reader.read_u16()).transpose()?,
                color: reader.read_bool()?.then(|| reader.read_u8()).transpose()?,
            })
        })
        .collect()
}

fn encode_submission(output: &mut Vec<u8>, value: &DialogSubmission) -> Result<(), EncodeError> {
    match value {
        DialogSubmission::Select { index, quantity } => {
            output.push(1);
            push_u16(output, *index);
            output.push(*quantity);
        }
        DialogSubmission::Input { input } => {
            output.push(2);
            encode_string(output, input)?;
        }
        DialogSubmission::Previous => output.push(3),
        DialogSubmission::Next => output.push(4),
        DialogSubmission::Close => output.push(5),
    }
    Ok(())
}

fn decode_submission(reader: &mut PayloadReader<'_>) -> Result<DialogSubmission, DecodeError> {
    Ok(match reader.read_u8()? {
        1 => DialogSubmission::Select {
            index: reader.read_u16()?,
            quantity: reader.read_u8()?,
        },
        2 => DialogSubmission::Input {
            input: decode_string(reader)?,
        },
        3 => DialogSubmission::Previous,
        4 => DialogSubmission::Next,
        5 => DialogSubmission::Close,
        actual => return Err(DecodeError::InvalidDialogField { actual }),
    })
}

fn encode_count(output: &mut Vec<u8>, count: usize) -> Result<(), EncodeError> {
    if count > MAX_DIALOG_ROWS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_DIALOG_ROWS,
        });
    }
    push_u16(output, count as u16);
    Ok(())
}

fn decode_count(reader: &mut PayloadReader<'_>) -> Result<usize, DecodeError> {
    let count = usize::from(reader.read_u16()?);
    if count > MAX_DIALOG_ROWS {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_DIALOG_ROWS,
        });
    }
    Ok(count)
}

fn encode_string(output: &mut Vec<u8>, value: &str) -> Result<(), EncodeError> {
    if value.len() > MAX_DIALOG_STRING_BYTES {
        return Err(EncodeError::SnapshotStringTooLong {
            length: value.len(),
            max: MAX_DIALOG_STRING_BYTES,
        });
    }
    push_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_string(reader: &mut PayloadReader<'_>) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > MAX_DIALOG_STRING_BYTES {
        return Err(DecodeError::SnapshotStringTooLong {
            length,
            max: MAX_DIALOG_STRING_BYTES,
        });
    }
    String::from_utf8(reader.take(length)?.to_vec()).map_err(|_| DecodeError::InvalidDialogText)
}

fn encode_optional_string(output: &mut Vec<u8>, value: Option<&str>) -> Result<(), EncodeError> {
    push_bool(output, value.is_some());
    if let Some(value) = value {
        encode_string(output, value)?;
    }
    Ok(())
}

fn decode_optional_string(reader: &mut PayloadReader<'_>) -> Result<Option<String>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_string(reader))
        .transpose()
}

fn encode_optional_u32(output: &mut Vec<u8>, value: Option<u32>) {
    push_bool(output, value.is_some());
    if let Some(value) = value {
        push_u32(output, value);
    }
}
fn decode_optional_u32(reader: &mut PayloadReader<'_>) -> Result<Option<u32>, DecodeError> {
    reader.read_bool()?.then(|| reader.read_u32()).transpose()
}

fn close_reason(value: DialogCloseReason) -> u8 {
    match value {
        DialogCloseReason::Client => 1,
        DialogCloseReason::Server => 2,
        DialogCloseReason::WorldChanged => 3,
        DialogCloseReason::Disconnected => 4,
        DialogCloseReason::Replaced => 5,
    }
}
fn decode_close_reason(actual: u8) -> Result<DialogCloseReason, DecodeError> {
    match actual {
        1 => Ok(DialogCloseReason::Client),
        2 => Ok(DialogCloseReason::Server),
        3 => Ok(DialogCloseReason::WorldChanged),
        4 => Ok(DialogCloseReason::Disconnected),
        5 => Ok(DialogCloseReason::Replaced),
        actual => Err(DecodeError::InvalidDialogField { actual }),
    }
}
