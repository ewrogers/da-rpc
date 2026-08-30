use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
};
use darpc_model::{
    BulletinCompose, BulletinEntry, BulletinEntrySummary, BulletinOperation,
    BulletinOperationResult, BulletinPagination, BulletinSection, BulletinSectionKind,
    BulletinSource, BulletinState, BulletinUpdate, BulletinView, BulletinViewport,
};

pub const MAX_BULLETIN_SECTIONS: usize = 64;
pub const MAX_BULLETIN_ENTRIES: usize = 128;
pub const MAX_BULLETIN_TEXT_LEN: usize = u8::MAX as usize;
pub const MAX_BULLETIN_BODY_LEN: usize = 32 * 1024 - 1;

pub(crate) fn encode_optional_state(
    output: &mut Vec<u8>,
    state: Option<&BulletinState>,
) -> Result<(), EncodeError> {
    push_bool(output, state.is_some());
    if let Some(state) = state {
        encode_state(output, state)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_state(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<BulletinState>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_state(reader))
        .transpose()
}

pub(crate) fn encode_update(
    output: &mut Vec<u8>,
    update: &BulletinUpdate,
) -> Result<(), EncodeError> {
    match update {
        BulletinUpdate::Opened(state) => {
            output.push(1);
            encode_state(output, state)?;
        }
        BulletinUpdate::Changed(state) => {
            output.push(2);
            encode_state(output, state)?;
        }
        BulletinUpdate::ActionSubmitted { state, operation } => {
            output.push(3);
            encode_operation(output, *operation);
            encode_optional_state(output, state.as_ref())?;
        }
        BulletinUpdate::OperationResult { state, result } => {
            output.push(4);
            encode_state(output, state)?;
            encode_result(output, result)?;
        }
        BulletinUpdate::Closed { previous } => {
            output.push(5);
            encode_state(output, previous)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<BulletinUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(BulletinUpdate::Opened(decode_state(reader)?)),
        2 => Ok(BulletinUpdate::Changed(decode_state(reader)?)),
        3 => Ok(BulletinUpdate::ActionSubmitted {
            operation: decode_operation(reader)?,
            state: decode_optional_state(reader)?,
        }),
        4 => Ok(BulletinUpdate::OperationResult {
            state: decode_state(reader)?,
            result: decode_result(reader)?,
        }),
        5 => Ok(BulletinUpdate::Closed {
            previous: decode_state(reader)?,
        }),
        actual => Err(DecodeError::InvalidBulletinField { actual }),
    }
}

fn encode_state(output: &mut Vec<u8>, state: &BulletinState) -> Result<(), EncodeError> {
    push_u32(output, state.revision);
    push_bool(output, state.pending.is_some());
    if let Some(operation) = state.pending {
        encode_operation(output, operation);
    }
    push_bool(output, state.last_operation_result.is_some());
    if let Some(result) = &state.last_operation_result {
        encode_result(output, result)?;
    }
    push_bool(output, state.can_go_back);
    push_bool(output, state.can_go_forward);
    encode_view(output, &state.view)
}

fn decode_state(reader: &mut PayloadReader<'_>) -> Result<BulletinState, DecodeError> {
    let revision = reader.read_u32()?;
    let pending = reader
        .read_bool()?
        .then(|| decode_operation(reader))
        .transpose()?;
    let last_operation_result = reader
        .read_bool()?
        .then(|| decode_result(reader))
        .transpose()?;
    let can_go_back = reader.read_bool()?;
    let can_go_forward = reader.read_bool()?;
    let view = decode_view(reader)?;
    Ok(BulletinState {
        revision,
        pending,
        last_operation_result,
        can_go_back,
        can_go_forward,
        view,
    })
}

fn encode_view(output: &mut Vec<u8>, view: &BulletinView) -> Result<(), EncodeError> {
    match view {
        BulletinView::Sections {
            heading,
            sections,
            selected_section_id,
            viewport,
            truncated,
        } => {
            output.push(1);
            encode_text(output, heading, MAX_BULLETIN_TEXT_LEN)?;
            let count = checked_count(sections.len(), MAX_BULLETIN_SECTIONS)?;
            output.push(u8::try_from(count).map_err(|_| EncodeError::LengthOverflow)?);
            for section in sections {
                encode_section(output, section)?;
            }
            encode_optional_u16(output, *selected_section_id);
            encode_viewport(output, *viewport);
            push_bool(output, *truncated);
        }
        BulletinView::Entries {
            section,
            entries,
            selected_entry_id,
            viewport,
            pagination,
            truncated,
        } => {
            output.push(2);
            encode_section(output, section)?;
            let count = checked_count(entries.len(), MAX_BULLETIN_ENTRIES)?;
            push_u16(
                output,
                u16::try_from(count).map_err(|_| EncodeError::LengthOverflow)?,
            );
            for entry in entries {
                encode_summary(output, entry)?;
            }
            encode_optional_i16(output, *selected_entry_id);
            encode_viewport(output, *viewport);
            output.push(pagination_wire(*pagination));
            push_bool(output, *truncated);
        }
        BulletinView::Entry {
            section,
            entry,
            viewport,
        } => {
            output.push(3);
            encode_section(output, section)?;
            encode_entry(output, entry)?;
            encode_viewport(output, *viewport);
        }
        BulletinView::Compose(BulletinCompose::BoardPost {
            section,
            author,
            subject,
            body,
        }) => {
            output.push(4);
            encode_section(output, section)?;
            encode_text(output, author, MAX_BULLETIN_TEXT_LEN)?;
            encode_text(output, subject, MAX_BULLETIN_TEXT_LEN)?;
            encode_text(output, body, MAX_BULLETIN_BODY_LEN)?;
        }
        BulletinView::Compose(BulletinCompose::PlayerMail {
            mailbox,
            recipient,
            recipient_editable,
            subject,
            body,
        }) => {
            output.push(5);
            encode_section(output, mailbox)?;
            encode_text(output, recipient, MAX_BULLETIN_TEXT_LEN)?;
            push_bool(output, *recipient_editable);
            encode_text(output, subject, MAX_BULLETIN_TEXT_LEN)?;
            encode_text(output, body, MAX_BULLETIN_BODY_LEN)?;
        }
    }
    Ok(())
}

fn decode_view(reader: &mut PayloadReader<'_>) -> Result<BulletinView, DecodeError> {
    match reader.read_u8()? {
        1 => {
            let heading = decode_text(reader, MAX_BULLETIN_TEXT_LEN)?;
            let count = usize::from(reader.read_u8()?);
            if count > MAX_BULLETIN_SECTIONS {
                return Err(DecodeError::SnapshotCollectionTooLong {
                    length: count,
                    max: MAX_BULLETIN_SECTIONS,
                });
            }
            let mut sections = Vec::with_capacity(count);
            for _ in 0..count {
                sections.push(decode_section(reader)?);
            }
            Ok(BulletinView::Sections {
                heading,
                sections,
                selected_section_id: decode_optional_u16(reader)?,
                viewport: decode_viewport(reader)?,
                truncated: reader.read_bool()?,
            })
        }
        2 => {
            let section = decode_section(reader)?;
            let count = usize::from(reader.read_u16()?);
            if count > MAX_BULLETIN_ENTRIES {
                return Err(DecodeError::SnapshotCollectionTooLong {
                    length: count,
                    max: MAX_BULLETIN_ENTRIES,
                });
            }
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                entries.push(decode_summary(reader)?);
            }
            Ok(BulletinView::Entries {
                section,
                entries,
                selected_entry_id: decode_optional_i16(reader)?,
                viewport: decode_viewport(reader)?,
                pagination: pagination_from_wire(reader.read_u8()?)?,
                truncated: reader.read_bool()?,
            })
        }
        3 => Ok(BulletinView::Entry {
            section: decode_section(reader)?,
            entry: decode_entry(reader)?,
            viewport: decode_viewport(reader)?,
        }),
        4 => Ok(BulletinView::Compose(BulletinCompose::BoardPost {
            section: decode_section(reader)?,
            author: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
            subject: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
            body: decode_text(reader, MAX_BULLETIN_BODY_LEN)?,
        })),
        5 => Ok(BulletinView::Compose(BulletinCompose::PlayerMail {
            mailbox: decode_section(reader)?,
            recipient: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
            recipient_editable: reader.read_bool()?,
            subject: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
            body: decode_text(reader, MAX_BULLETIN_BODY_LEN)?,
        })),
        actual => Err(DecodeError::InvalidBulletinField { actual }),
    }
}

fn encode_section(output: &mut Vec<u8>, section: &BulletinSection) -> Result<(), EncodeError> {
    push_u16(output, section.id);
    output.push(match section.kind {
        BulletinSectionKind::Unknown => 0,
        BulletinSectionKind::Board => 1,
        BulletinSectionKind::Mailbox => 2,
    });
    output.push(section.source.raw());
    encode_text(output, &section.name, MAX_BULLETIN_TEXT_LEN)
}

fn decode_section(reader: &mut PayloadReader<'_>) -> Result<BulletinSection, DecodeError> {
    let id = reader.read_u16()?;
    let kind = match reader.read_u8()? {
        0 => BulletinSectionKind::Unknown,
        1 => BulletinSectionKind::Board,
        2 => BulletinSectionKind::Mailbox,
        actual => return Err(DecodeError::InvalidBulletinField { actual }),
    };
    let source = BulletinSource::from_raw(reader.read_u8()?);
    let name = decode_text(reader, MAX_BULLETIN_TEXT_LEN)?;
    Ok(BulletinSection {
        id,
        name,
        kind,
        source,
    })
}

fn encode_summary(output: &mut Vec<u8>, entry: &BulletinEntrySummary) -> Result<(), EncodeError> {
    push_u16(output, entry.id as u16);
    output.push(entry.flags);
    output.push(entry.month);
    output.push(entry.day);
    encode_text(output, &entry.author, MAX_BULLETIN_TEXT_LEN)?;
    encode_text(output, &entry.subject, MAX_BULLETIN_TEXT_LEN)
}

fn decode_summary(reader: &mut PayloadReader<'_>) -> Result<BulletinEntrySummary, DecodeError> {
    Ok(BulletinEntrySummary {
        id: reader.read_u16()? as i16,
        flags: reader.read_u8()?,
        month: reader.read_u8()?,
        day: reader.read_u8()?,
        author: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
        subject: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
    })
}

fn encode_entry(output: &mut Vec<u8>, entry: &BulletinEntry) -> Result<(), EncodeError> {
    push_u16(output, entry.id as u16);
    push_bool(output, entry.flags.is_some());
    if let Some(flags) = entry.flags {
        output.push(flags);
    }
    output.push(entry.month);
    output.push(entry.day);
    output.push(entry.navigation_flags);
    output.push(entry.unknown_before_id);
    encode_text(output, &entry.author, MAX_BULLETIN_TEXT_LEN)?;
    encode_text(output, &entry.subject, MAX_BULLETIN_TEXT_LEN)?;
    encode_text(output, &entry.body, MAX_BULLETIN_BODY_LEN)
}

fn decode_entry(reader: &mut PayloadReader<'_>) -> Result<BulletinEntry, DecodeError> {
    let id = reader.read_u16()? as i16;
    let flags = reader.read_bool()?.then(|| reader.read_u8()).transpose()?;
    Ok(BulletinEntry {
        id,
        flags,
        month: reader.read_u8()?,
        day: reader.read_u8()?,
        navigation_flags: reader.read_u8()?,
        unknown_before_id: reader.read_u8()?,
        author: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
        subject: decode_text(reader, MAX_BULLETIN_TEXT_LEN)?,
        body: decode_text(reader, MAX_BULLETIN_BODY_LEN)?,
    })
}

fn encode_viewport(output: &mut Vec<u8>, viewport: BulletinViewport) {
    push_i32(output, viewport.position);
    push_i32(output, viewport.maximum);
}

fn decode_viewport(reader: &mut PayloadReader<'_>) -> Result<BulletinViewport, DecodeError> {
    Ok(BulletinViewport {
        position: reader.read_i32()?,
        maximum: reader.read_i32()?,
    })
}

fn encode_result(
    output: &mut Vec<u8>,
    result: &BulletinOperationResult,
) -> Result<(), EncodeError> {
    encode_operation(output, result.operation);
    output.push(result.raw_status);
    push_bool(output, result.message.is_some());
    if let Some(message) = &result.message {
        encode_text(output, message, MAX_BULLETIN_TEXT_LEN)?;
    }
    Ok(())
}

fn decode_result(reader: &mut PayloadReader<'_>) -> Result<BulletinOperationResult, DecodeError> {
    Ok(BulletinOperationResult {
        operation: decode_operation(reader)?,
        raw_status: reader.read_u8()?,
        message: reader
            .read_bool()?
            .then(|| decode_text(reader, MAX_BULLETIN_TEXT_LEN))
            .transpose()?,
    })
}

fn encode_operation(output: &mut Vec<u8>, operation: BulletinOperation) {
    output.push(match operation {
        BulletinOperation::Unknown => 0,
        BulletinOperation::OpenSections => 1,
        BulletinOperation::OpenWorldBoard => 2,
        BulletinOperation::OpenSection => 3,
        BulletinOperation::LoadOlder => 4,
        BulletinOperation::OpenEntry => 5,
        BulletinOperation::PreviousEntry => 6,
        BulletinOperation::NextEntry => 7,
        BulletinOperation::PostArticle => 8,
        BulletinOperation::DeleteEntry => 9,
        BulletinOperation::SendMail => 10,
        BulletinOperation::HighlightArticle => 11,
        BulletinOperation::SelectSection => 12,
        BulletinOperation::SelectEntry => 13,
        BulletinOperation::Scroll => 14,
        BulletinOperation::Back => 15,
        BulletinOperation::Forward => 16,
        BulletinOperation::BeginBoardPost => 17,
        BulletinOperation::BeginPlayerMail => 18,
        BulletinOperation::BeginReply => 19,
        BulletinOperation::UpdateCompose => 20,
        BulletinOperation::Close => 21,
    });
}

fn decode_operation(reader: &mut PayloadReader<'_>) -> Result<BulletinOperation, DecodeError> {
    Ok(match reader.read_u8()? {
        0 => BulletinOperation::Unknown,
        1 => BulletinOperation::OpenSections,
        2 => BulletinOperation::OpenWorldBoard,
        3 => BulletinOperation::OpenSection,
        4 => BulletinOperation::LoadOlder,
        5 => BulletinOperation::OpenEntry,
        6 => BulletinOperation::PreviousEntry,
        7 => BulletinOperation::NextEntry,
        8 => BulletinOperation::PostArticle,
        9 => BulletinOperation::DeleteEntry,
        10 => BulletinOperation::SendMail,
        11 => BulletinOperation::HighlightArticle,
        12 => BulletinOperation::SelectSection,
        13 => BulletinOperation::SelectEntry,
        14 => BulletinOperation::Scroll,
        15 => BulletinOperation::Back,
        16 => BulletinOperation::Forward,
        17 => BulletinOperation::BeginBoardPost,
        18 => BulletinOperation::BeginPlayerMail,
        19 => BulletinOperation::BeginReply,
        20 => BulletinOperation::UpdateCompose,
        21 => BulletinOperation::Close,
        actual => return Err(DecodeError::InvalidBulletinField { actual }),
    })
}

fn pagination_wire(value: BulletinPagination) -> u8 {
    match value {
        BulletinPagination::Unknown => 0,
        BulletinPagination::Ready => 1,
        BulletinPagination::Loading => 2,
        BulletinPagination::Exhausted => 3,
    }
}

fn pagination_from_wire(value: u8) -> Result<BulletinPagination, DecodeError> {
    match value {
        0 => Ok(BulletinPagination::Unknown),
        1 => Ok(BulletinPagination::Ready),
        2 => Ok(BulletinPagination::Loading),
        3 => Ok(BulletinPagination::Exhausted),
        actual => Err(DecodeError::InvalidBulletinField { actual }),
    }
}

fn encode_optional_u16(output: &mut Vec<u8>, value: Option<u16>) {
    push_bool(output, value.is_some());
    if let Some(value) = value {
        push_u16(output, value);
    }
}

fn decode_optional_u16(reader: &mut PayloadReader<'_>) -> Result<Option<u16>, DecodeError> {
    reader.read_bool()?.then(|| reader.read_u16()).transpose()
}

fn encode_optional_i16(output: &mut Vec<u8>, value: Option<i16>) {
    encode_optional_u16(output, value.map(|value| value as u16));
}

fn decode_optional_i16(reader: &mut PayloadReader<'_>) -> Result<Option<i16>, DecodeError> {
    decode_optional_u16(reader).map(|value| value.map(|value| value as i16))
}

fn encode_text(output: &mut Vec<u8>, value: &str, max: usize) -> Result<(), EncodeError> {
    let bytes = value.as_bytes();
    if bytes.len() > max {
        return Err(EncodeError::SnapshotStringTooLong {
            length: bytes.len(),
            max,
        });
    }
    push_u16(
        output,
        u16::try_from(bytes.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    output.extend_from_slice(bytes);
    Ok(())
}

fn decode_text(reader: &mut PayloadReader<'_>, max: usize) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::SnapshotStringTooLong { length, max });
    }
    std::str::from_utf8(reader.take(length)?)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}

fn checked_count(length: usize, max: usize) -> Result<usize, EncodeError> {
    if length > max {
        Err(EncodeError::SnapshotCollectionTooLong { length, max })
    } else {
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> BulletinState {
        BulletinState {
            revision: 7,
            pending: Some(BulletinOperation::LoadOlder),
            last_operation_result: None,
            can_go_back: true,
            can_go_forward: false,
            view: BulletinView::Entries {
                section: BulletinSection {
                    id: 184,
                    name: "Mileth Adventure".into(),
                    kind: BulletinSectionKind::Board,
                    source: BulletinSource::Clicked,
                },
                entries: vec![BulletinEntrySummary {
                    id: 62,
                    flags: 1,
                    author: "Test".into(),
                    month: 8,
                    day: 29,
                    subject: "Hello".into(),
                }],
                selected_entry_id: Some(62),
                viewport: BulletinViewport {
                    position: 3,
                    maximum: 12,
                },
                pagination: BulletinPagination::Loading,
                truncated: false,
            },
        }
    }

    #[test]
    fn state_round_trips() {
        let expected = state();
        let mut encoded = Vec::new();
        encode_optional_state(&mut encoded, Some(&expected)).unwrap();
        let mut reader = PayloadReader::new(crate::MessageType::SnapshotResponse, &encoded);
        assert_eq!(decode_optional_state(&mut reader).unwrap(), Some(expected));
        assert!(reader.is_empty());
    }

    #[test]
    fn operation_result_update_round_trips() {
        let expected = BulletinUpdate::OperationResult {
            state: state(),
            result: BulletinOperationResult {
                operation: BulletinOperation::DeleteEntry,
                raw_status: 0,
                message: Some("You cannot destroy this message.".into()),
            },
        };
        let mut encoded = Vec::new();
        encode_update(&mut encoded, &expected).unwrap();
        let mut reader = PayloadReader::new(crate::MessageType::EventPollResponse, &encoded);
        assert_eq!(decode_update(&mut reader).unwrap(), expected);
        assert!(reader.is_empty());
    }
}
