use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16},
};
use darpc_model::{LegendIcon, LegendMark, LegendUpdate};

pub const MAX_LEGEND_MARKS: usize = u8::MAX as usize;
pub const MAX_LEGEND_TEXT_LEN: usize = u8::MAX as usize;
pub const MAX_LEGEND_TAG_LEN: usize = u8::MAX as usize;

pub(crate) fn encode_optional(
    output: &mut Vec<u8>,
    legend: Option<&[LegendMark]>,
) -> Result<(), EncodeError> {
    push_bool(output, legend.is_some());
    if let Some(legend) = legend {
        encode(output, legend)?;
    }
    Ok(())
}

pub(crate) fn decode_optional(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<Vec<LegendMark>>, DecodeError> {
    if reader.read_bool()? {
        decode(reader).map(Some)
    } else {
        Ok(None)
    }
}

pub(crate) fn encode(output: &mut Vec<u8>, legend: &[LegendMark]) -> Result<(), EncodeError> {
    if legend.len() > MAX_LEGEND_MARKS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: legend.len(),
            max: MAX_LEGEND_MARKS,
        });
    }
    output.push(u8::try_from(legend.len()).map_err(|_| EncodeError::LengthOverflow)?);
    for mark in legend {
        encode_mark(output, mark)?;
    }
    Ok(())
}

pub(crate) fn decode(reader: &mut PayloadReader<'_>) -> Result<Vec<LegendMark>, DecodeError> {
    let count = usize::from(reader.read_u8()?);
    let mut legend = Vec::with_capacity(count);
    for _ in 0..count {
        legend.push(decode_mark(reader)?);
    }
    Ok(legend)
}

pub(crate) fn encode_update(
    output: &mut Vec<u8>,
    update: &LegendUpdate,
) -> Result<(), EncodeError> {
    match update {
        LegendUpdate::MarkAdded { mark } => {
            output.push(1);
            encode_mark(output, mark)?;
        }
        LegendUpdate::MarkChanged { previous, current } => {
            output.push(2);
            encode_mark(output, previous)?;
            encode_mark(output, current)?;
        }
        LegendUpdate::MarkRemoved { mark } => {
            output.push(3);
            encode_mark(output, mark)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<LegendUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(LegendUpdate::MarkAdded {
            mark: decode_mark(reader)?,
        }),
        2 => Ok(LegendUpdate::MarkChanged {
            previous: decode_mark(reader)?,
            current: decode_mark(reader)?,
        }),
        3 => Ok(LegendUpdate::MarkRemoved {
            mark: decode_mark(reader)?,
        }),
        actual => Err(DecodeError::InvalidLegendUpdateType { actual }),
    }
}

fn encode_mark(output: &mut Vec<u8>, mark: &LegendMark) -> Result<(), EncodeError> {
    output.push(mark.icon.raw());
    output.push(mark.color);
    encode_string(output, &mark.tag, MAX_LEGEND_TAG_LEN)?;
    encode_string(output, &mark.text, MAX_LEGEND_TEXT_LEN)
}

fn decode_mark(reader: &mut PayloadReader<'_>) -> Result<LegendMark, DecodeError> {
    Ok(LegendMark {
        icon: LegendIcon::from_raw(reader.read_u8()?),
        color: reader.read_u8()?,
        tag: decode_string(reader, MAX_LEGEND_TAG_LEN)?,
        text: decode_string(reader, MAX_LEGEND_TEXT_LEN)?,
    })
}

fn encode_string(output: &mut Vec<u8>, value: &str, max: usize) -> Result<(), EncodeError> {
    if value.len() > max {
        return Err(EncodeError::SnapshotStringTooLong {
            length: value.len(),
            max,
        });
    }
    push_u16(
        output,
        u16::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_string(reader: &mut PayloadReader<'_>, max: usize) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::SnapshotStringTooLong { length, max });
    }
    String::from_utf8(reader.take(length)?.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
}
