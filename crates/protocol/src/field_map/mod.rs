use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16, push_u32},
};
use darpc_model::{FieldMapDestination, FieldMapSelection, FieldMapState, FieldMapUpdate};

pub const MAX_FIELD_MAP_DESTINATIONS: usize = u8::MAX as usize;
pub const MAX_FIELD_MAP_TEXT_LEN: usize = u8::MAX as usize;

pub(crate) fn encode_optional_state(
    output: &mut Vec<u8>,
    state: Option<&FieldMapState>,
) -> Result<(), EncodeError> {
    push_bool(output, state.is_some());
    if let Some(state) = state {
        encode_state(output, state)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_state(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<FieldMapState>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_state(reader))
        .transpose()
}

pub(crate) fn encode_update(
    output: &mut Vec<u8>,
    update: &FieldMapUpdate,
) -> Result<(), EncodeError> {
    match update {
        FieldMapUpdate::Opened(state) => {
            output.push(1);
            encode_state(output, state)?;
        }
        FieldMapUpdate::Changed(state) => {
            output.push(2);
            encode_state(output, state)?;
        }
        FieldMapUpdate::SelectionSubmitted(state) => {
            output.push(3);
            encode_state(output, state)?;
        }
        FieldMapUpdate::Closed { previous } => {
            output.push(4);
            encode_state(output, previous)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<FieldMapUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(FieldMapUpdate::Opened(decode_state(reader)?)),
        2 => Ok(FieldMapUpdate::Changed(decode_state(reader)?)),
        3 => Ok(FieldMapUpdate::SelectionSubmitted(decode_state(reader)?)),
        4 => Ok(FieldMapUpdate::Closed {
            previous: decode_state(reader)?,
        }),
        actual => Err(DecodeError::InvalidFieldMapField { actual }),
    }
}

fn encode_state(output: &mut Vec<u8>, state: &FieldMapState) -> Result<(), EncodeError> {
    push_u32(output, state.revision);
    encode_text(output, &state.field_name)?;
    push_bool(output, state.current_node_index.is_some());
    if let Some(index) = state.current_node_index {
        output.push(index);
    }
    let count = u8::try_from(state.destinations.len()).map_err(|_| {
        EncodeError::SnapshotCollectionTooLong {
            length: state.destinations.len(),
            max: MAX_FIELD_MAP_DESTINATIONS,
        }
    })?;
    output.push(count);
    for (expected, destination) in (0..count).zip(&state.destinations) {
        if destination.index != expected {
            return Err(EncodeError::InvalidFieldMapIndex {
                actual: destination.index,
                expected,
            });
        }
        encode_destination(output, destination)?;
    }
    push_bool(output, state.selection.is_some());
    if let Some(selection) = state.selection {
        output.push(selection.destination_index);
    }
    validate_indices(state.current_node_index, state.selection, count).map_err(|actual| {
        EncodeError::InvalidFieldMapIndex {
            actual,
            expected: count,
        }
    })
}

fn decode_state(reader: &mut PayloadReader<'_>) -> Result<FieldMapState, DecodeError> {
    let revision = reader.read_u32()?;
    let field_name = decode_text(reader)?;
    let current_node_index = reader.read_bool()?.then(|| reader.read_u8()).transpose()?;
    let count = reader.read_u8()?;
    let mut destinations = Vec::with_capacity(usize::from(count));
    for index in 0..count {
        destinations.push(decode_destination(reader, index)?);
    }
    let selection = reader
        .read_bool()?
        .then(|| {
            reader
                .read_u8()
                .map(|destination_index| FieldMapSelection { destination_index })
        })
        .transpose()?;
    validate_indices(current_node_index, selection, count)
        .map_err(|actual| DecodeError::InvalidFieldMapIndex { actual, count })?;
    Ok(FieldMapState {
        revision,
        field_name,
        current_node_index,
        destinations,
        selection,
    })
}

fn encode_destination(
    output: &mut Vec<u8>,
    destination: &FieldMapDestination,
) -> Result<(), EncodeError> {
    output.push(destination.index);
    push_u16(output, destination.screen_x);
    push_u16(output, destination.screen_y);
    encode_text(output, &destination.name)?;
    push_u16(output, destination.checksum);
    push_u16(output, destination.map_id);
    push_u16(output, destination.map_x);
    push_u16(output, destination.map_y);
    Ok(())
}

fn decode_destination(
    reader: &mut PayloadReader<'_>,
    expected: u8,
) -> Result<FieldMapDestination, DecodeError> {
    let index = reader.read_u8()?;
    if index != expected {
        return Err(DecodeError::InvalidFieldMapDestinationIndex { index, expected });
    }
    Ok(FieldMapDestination {
        index,
        screen_x: reader.read_u16()?,
        screen_y: reader.read_u16()?,
        name: decode_text(reader)?,
        checksum: reader.read_u16()?,
        map_id: reader.read_u16()?,
        map_x: reader.read_u16()?,
        map_y: reader.read_u16()?,
    })
}

fn encode_text(output: &mut Vec<u8>, value: &str) -> Result<(), EncodeError> {
    let bytes = value.as_bytes();
    let length = u8::try_from(bytes.len()).map_err(|_| EncodeError::SnapshotStringTooLong {
        length: bytes.len(),
        max: MAX_FIELD_MAP_TEXT_LEN,
    })?;
    output.push(length);
    output.extend_from_slice(bytes);
    Ok(())
}

fn decode_text(reader: &mut PayloadReader<'_>) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u8()?);
    std::str::from_utf8(reader.take(length)?)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}

fn validate_indices(
    current_node_index: Option<u8>,
    selection: Option<FieldMapSelection>,
    count: u8,
) -> Result<(), u8> {
    if let Some(index) = current_node_index
        && index >= count
    {
        return Err(index);
    }
    if let Some(selection) = selection
        && selection.destination_index >= count
    {
        return Err(selection.destination_index);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_noncontiguous_and_out_of_range_indices() {
        let destination = FieldMapDestination {
            index: 1,
            screen_x: 0,
            screen_y: 0,
            name: "Mileth".into(),
            checksum: 1,
            map_id: 2,
            map_x: 3,
            map_y: 4,
        };
        let state = FieldMapState {
            revision: 1,
            field_name: "field001".into(),
            current_node_index: Some(1),
            destinations: vec![destination],
            selection: None,
        };
        assert!(matches!(
            encode_optional_state(&mut Vec::new(), Some(&state)),
            Err(EncodeError::InvalidFieldMapIndex { .. })
        ));
    }
}
