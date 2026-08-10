#![cfg_attr(not(windows), allow(dead_code))]

use crate::packet::ParseError;
use darpc_model::MessageKind;

const MESSAGE_OPCODE: u8 = 0x0A;
const SAY_OPCODE: u8 = 0x0D;
const SAY_MODE: u8 = 0;
const SHOUT_MODE: u8 = 1;
const CHANT_MODE: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Participant<'a> {
    None,
    SelfPlayer,
    Named(&'a [u8]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParsedMessage<'a> {
    pub(crate) kind: MessageKind,
    pub(crate) sender: Participant<'a>,
    pub(crate) recipient: Participant<'a>,
    pub(crate) sender_id: Option<u32>,
    pub(crate) text: &'a [u8],
}

pub(crate) fn update(body: &[u8]) -> Result<Option<ParsedMessage<'_>>, ParseError> {
    match body.first().copied() {
        Some(MESSAGE_OPCODE) => parse_message(body),
        Some(SAY_OPCODE) => parse_say(body),
        _ => Ok(None),
    }
}

fn parse_message(body: &[u8]) -> Result<Option<ParsedMessage<'_>>, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(MESSAGE_OPCODE)?;
    let message_type = reader.u8()?;
    if !is_history_message(message_type) {
        return Ok(None);
    }
    let message = reader.string16()?;
    Ok(classify_message(trim_ascii(message)))
}

fn parse_say(body: &[u8]) -> Result<Option<ParsedMessage<'_>>, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(SAY_OPCODE)?;
    let mode = reader.u8()?;
    if !matches!(mode, SAY_MODE | SHOUT_MODE | CHANT_MODE) {
        return Ok(None);
    }
    let sender_id = reader.u32_be()?;
    let displayed = trim_ascii(reader.string8()?);
    if mode == CHANT_MODE {
        return Ok((!displayed.is_empty()).then_some(ParsedMessage {
            kind: MessageKind::Chant,
            sender: Participant::None,
            recipient: Participant::None,
            sender_id: Some(sender_id),
            text: displayed,
        }));
    }
    if mode == SHOUT_MODE && world_prefix(displayed).is_some() {
        return Ok(None);
    }
    let delimiter = if mode == SAY_MODE { b':' } else { b'!' };
    let (sender, text) = split_named(displayed, delimiter)
        .map_or((Participant::None, displayed), |(name, text)| {
            (Participant::Named(name), text)
        });
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(ParsedMessage {
        kind: if mode == SAY_MODE {
            MessageKind::Say
        } else {
            MessageKind::Shout
        },
        sender,
        recipient: Participant::None,
        sender_id: Some(sender_id),
        text,
    }))
}

fn classify_message(message: &[u8]) -> Option<ParsedMessage<'_>> {
    if message.is_empty() {
        return None;
    }
    if let Some((sender, text)) = enclosed_prefix(message, b"[!", b']') {
        return parsed(MessageKind::Group, Participant::Named(sender), text);
    }
    if let Some((sender, text)) = enclosed_prefix(message, b"<!", b'>') {
        return parsed(MessageKind::Guild, Participant::Named(sender), text);
    }
    if let Some((sender, text)) = world_prefix(message) {
        return parsed(MessageKind::World, Participant::Named(sender), text);
    }
    if let Some((recipient, text)) = split_named(message, b'>') {
        return (!text.is_empty()).then_some(ParsedMessage {
            kind: MessageKind::Whisper,
            sender: Participant::SelfPlayer,
            recipient: Participant::Named(recipient),
            sender_id: None,
            text,
        });
    }
    if let Some((sender, text)) = split_named(message, b'"') {
        return (!text.is_empty()).then_some(ParsedMessage {
            kind: MessageKind::Whisper,
            sender: Participant::Named(sender),
            recipient: Participant::SelfPlayer,
            sender_id: None,
            text,
        });
    }
    parsed(MessageKind::System, Participant::None, message)
}

fn parsed<'a>(
    kind: MessageKind,
    sender: Participant<'a>,
    text: &'a [u8],
) -> Option<ParsedMessage<'a>> {
    (!text.is_empty()).then_some(ParsedMessage {
        kind,
        sender,
        recipient: Participant::None,
        sender_id: None,
        text,
    })
}

fn enclosed_prefix<'a>(message: &'a [u8], prefix: &[u8], end: u8) -> Option<(&'a [u8], &'a [u8])> {
    let remainder = message.strip_prefix(prefix)?;
    let end_index = remainder.iter().position(|byte| *byte == end)?;
    let name = trim_ascii(&remainder[..end_index]);
    let text = trim_ascii(&remainder[end_index + 1..]);
    valid_name(name).then_some((name, text))
}

fn world_prefix(message: &[u8]) -> Option<(&[u8], &[u8])> {
    let remainder = message.strip_prefix(b"[")?;
    let end_index = remainder.windows(2).position(|bytes| bytes == b"]:")?;
    let name = trim_ascii(&remainder[..end_index]);
    let text = trim_ascii(&remainder[end_index + 2..]);
    valid_name(name).then_some((name, text))
}

fn split_named(message: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let index = message.iter().position(|byte| *byte == delimiter)?;
    let name = trim_ascii(&message[..index]);
    let text = trim_ascii(&message[index + 1..]);
    valid_name(name).then_some((name, text))
}

fn valid_name(name: &[u8]) -> bool {
    !name.is_empty() && name.len() <= 15 && !name.iter().any(u8::is_ascii_whitespace)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn is_history_message(message_type: u8) -> bool {
    matches!(message_type, 0x00..=0x06 | 0x0B | 0x0C)
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        let offset = self.offset;
        let actual = self.u8()?;
        if actual != expected {
            return Err(ParseError::invalid(offset, u32::from(actual)));
        }
        Ok(())
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    fn u16_be(&mut self) -> Result<u16, ParseError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32_be(&mut self) -> Result<u32, ParseError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn string8(&mut self) -> Result<&'a [u8], ParseError> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn string16(&mut self) -> Result<&'a [u8], ParseError> {
        let length = usize::from(self.u16_be()?);
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ParseError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining < length {
            return Err(ParseError::truncated(self.offset, length, remaining));
        }
        let start = self.offset;
        self.offset += length;
        Ok(&self.bytes[start..self.offset])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(message_type: u8, text: &[u8]) -> Vec<u8> {
        let mut body = vec![MESSAGE_OPCODE, message_type];
        body.extend_from_slice(&u16::try_from(text.len()).unwrap().to_be_bytes());
        body.extend_from_slice(text);
        body
    }

    fn say(mode: u8, sender_id: u32, text: &[u8]) -> Vec<u8> {
        let mut body = vec![SAY_OPCODE, mode];
        body.extend_from_slice(&sender_id.to_be_bytes());
        body.push(u8::try_from(text.len()).unwrap());
        body.extend_from_slice(text);
        body
    }

    #[test]
    fn classifies_history_message_prefixes() {
        let cases = [
            (
                b"[!Aisling] hello".as_slice(),
                MessageKind::Group,
                Participant::Named(b"Aisling"),
                Participant::None,
            ),
            (
                b"<!Aisling> hello".as_slice(),
                MessageKind::Guild,
                Participant::Named(b"Aisling"),
                Participant::None,
            ),
            (
                b"[Aisling]: hello".as_slice(),
                MessageKind::World,
                Participant::Named(b"Aisling"),
                Participant::None,
            ),
            (
                b"Aisling> hello".as_slice(),
                MessageKind::Whisper,
                Participant::SelfPlayer,
                Participant::Named(b"Aisling"),
            ),
            (
                b"Aisling\" hello".as_slice(),
                MessageKind::Whisper,
                Participant::Named(b"Aisling"),
                Participant::SelfPlayer,
            ),
        ];
        for (raw, kind, sender, recipient) in cases {
            let body = message(0, raw);
            let parsed = update(&body).unwrap().unwrap();
            assert_eq!(parsed.kind, kind);
            assert_eq!(parsed.text, b"hello");
            assert_eq!(parsed.sender, sender);
            assert_eq!(parsed.recipient, recipient);
        }
    }

    #[test]
    fn parses_say_and_shout_without_formatting() {
        let body = say(SAY_MODE, 42, b"Aisling: hello");
        let parsed = update(&body).unwrap().unwrap();
        assert_eq!(parsed.kind, MessageKind::Say);
        assert_eq!(parsed.sender, Participant::Named(b"Aisling"));
        assert_eq!(parsed.text, b"hello");
        assert_eq!(parsed.sender_id, Some(42));

        let body = say(SHOUT_MODE, 43, b"Aisling! hello");
        let parsed = update(&body).unwrap().unwrap();
        assert_eq!(parsed.kind, MessageKind::Shout);
        assert_eq!(parsed.text, b"hello");
    }

    #[test]
    fn ignores_popups_and_parses_spell_chants() {
        assert_eq!(update(&message(0x08, b"popup")).unwrap(), None);
        let body = say(CHANT_MODE, 42, b"ard cradh");
        let parsed = update(&body).unwrap().unwrap();
        assert_eq!(parsed.kind, MessageKind::Chant);
        assert_eq!(parsed.sender, Participant::None);
        assert_eq!(parsed.sender_id, Some(42));
        assert_eq!(parsed.text, b"ard cradh");
    }

    #[test]
    fn ignores_empty_messages() {
        for body in [
            message(0, b""),
            message(0, b"   "),
            message(0, b"[Aisling]:   "),
            message(0, b"Aisling>   "),
            say(SAY_MODE, 42, b""),
            say(SAY_MODE, 42, b"Aisling:   "),
            say(SHOUT_MODE, 42, b"Aisling!   "),
            say(CHANT_MODE, 42, b"   "),
        ] {
            assert_eq!(update(&body).unwrap(), None);
        }
    }

    #[test]
    fn ignores_the_rendered_shout_companion_for_a_world_message() {
        assert_eq!(
            update(&say(SHOUT_MODE, 42, b"[Aisling]: hello")).unwrap(),
            None
        );
        assert_eq!(
            update(&say(SHOUT_MODE, 42, b"Aisling! hello"))
                .unwrap()
                .unwrap()
                .kind,
            MessageKind::Shout
        );
    }

    #[test]
    fn rejects_truncated_messages() {
        let error = update(&[MESSAGE_OPCODE, 0, 0, 4, b'o']).unwrap_err();
        assert_eq!(error.offset(), 4);
        assert_eq!(error.needed(), 4);
        assert_eq!(error.remaining(), 1);
    }
}
