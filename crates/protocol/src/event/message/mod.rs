use super::*;

pub(super) fn encode_message(
    output: &mut Vec<u8>,
    message: &ClientMessage,
) -> Result<(), EncodeError> {
    output.push(match message.kind {
        MessageKind::Say => 1,
        MessageKind::Shout => 2,
        MessageKind::Chant => 8,
        MessageKind::Whisper => 3,
        MessageKind::Guild => 4,
        MessageKind::Group => 5,
        MessageKind::System => 6,
        MessageKind::World => 7,
    });
    push_bool(output, message.sender_id.is_some());
    if let Some(id) = message.sender_id {
        push_u32(output, id);
    }
    output.push(match message.sender_type {
        None => 0,
        Some(MessageSenderType::Player) => 1,
        Some(MessageSenderType::Monster) => 2,
        Some(MessageSenderType::Mundane) => 3,
    });
    encode_optional_message_name(output, message.sender.as_deref())?;
    encode_optional_message_name(output, message.recipient.as_deref())?;
    encode_event_string(output, &message.text, MAX_MESSAGE_TEXT_LEN)
}

pub(super) fn decode_message(reader: &mut PayloadReader<'_>) -> Result<ClientMessage, DecodeError> {
    let kind = match reader.read_u8()? {
        1 => MessageKind::Say,
        2 => MessageKind::Shout,
        8 => MessageKind::Chant,
        3 => MessageKind::Whisper,
        4 => MessageKind::Guild,
        5 => MessageKind::Group,
        6 => MessageKind::System,
        7 => MessageKind::World,
        actual => return Err(DecodeError::InvalidMessageKind { actual }),
    };
    let sender_id = reader.read_bool()?.then(|| reader.read_u32()).transpose()?;
    let sender_type = match reader.read_u8()? {
        0 => None,
        1 => Some(MessageSenderType::Player),
        2 => Some(MessageSenderType::Monster),
        3 => Some(MessageSenderType::Mundane),
        actual => return Err(DecodeError::InvalidMessageSenderType { actual }),
    };
    Ok(ClientMessage {
        kind,
        sender_id,
        sender_type,
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

pub(super) fn encode_event_string(
    output: &mut Vec<u8>,
    value: &str,
    max: usize,
) -> Result<(), EncodeError> {
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

pub(super) fn decode_event_string(
    reader: &mut PayloadReader<'_>,
    max: usize,
) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::EventStringTooLong { length, max });
    }
    let bytes = reader.take(length)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_metadata_round_trips_for_local_speech() {
        for kind in [MessageKind::Say, MessageKind::Shout] {
            for sender_type in [
                None,
                Some(MessageSenderType::Player),
                Some(MessageSenderType::Monster),
                Some(MessageSenderType::Mundane),
            ] {
                for sender_id in [None, Some(0), Some(u32::MAX)] {
                    let message = ClientMessage {
                        kind,
                        sender_id,
                        sender_type,
                        sender: Some("Speaker".into()),
                        recipient: None,
                        text: "Hello".into(),
                    };
                    let mut bytes = Vec::new();
                    encode_message(&mut bytes, &message).unwrap();
                    assert_eq!(
                        decode_message(&mut PayloadReader::new(
                            crate::MessageType::EventPollResponse,
                            &bytes
                        ))
                        .unwrap(),
                        message
                    );
                }
            }
        }
    }

    #[test]
    fn rejects_unknown_sender_types_and_truncated_ids() {
        assert_eq!(
            decode_message(&mut PayloadReader::new(
                crate::MessageType::EventPollResponse,
                &[1, 0, 4]
            )),
            Err(DecodeError::InvalidMessageSenderType { actual: 4 })
        );
        for bytes in [&[1, 1][..], &[1, 1, 42], &[1, 1, 42, 0], &[1, 1, 42, 0, 0]] {
            assert!(
                decode_message(&mut PayloadReader::new(
                    crate::MessageType::EventPollResponse,
                    bytes
                ))
                .is_err()
            );
        }
    }
}
