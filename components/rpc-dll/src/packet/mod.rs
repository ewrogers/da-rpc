mod action_delay;
pub(crate) mod audio;
pub(crate) mod message;
pub(crate) mod object;
mod state;
pub(crate) mod visual;

use self::{
    action_delay::ActionDelay,
    audio::AudioUpdate,
    message::ParsedMessage,
    object::WorldUpdate,
    state::{CollectionDirty, Position, SpelledUpdate, StatePacketUpdate, UserAppearance},
    visual::VisualUpdate,
};
use darpc_game_client::RawObjects;
use darpc_model::StatusUpdate;
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseError {
    offset: usize,
    needed: usize,
    remaining: usize,
    invalid_value: Option<u32>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(actual) = self.invalid_value {
            return write!(
                formatter,
                "server packet has invalid byte {actual} at offset {}",
                self.offset
            );
        }
        write!(
            formatter,
            "server packet truncated at offset {}: need {} bytes, {} remain",
            self.offset, self.needed, self.remaining
        )
    }
}

impl Error for ParseError {}

impl ParseError {
    pub(crate) const fn truncated(offset: usize, needed: usize, remaining: usize) -> Self {
        Self {
            offset,
            needed,
            remaining,
            invalid_value: None,
        }
    }

    pub(crate) const fn invalid(offset: usize, value: u32) -> Self {
        Self {
            offset,
            needed: 0,
            remaining: 0,
            invalid_value: Some(value),
        }
    }

    pub(crate) const fn offset(self) -> usize {
        self.offset
    }

    pub(crate) const fn needed(self) -> usize {
        self.needed
    }

    pub(crate) const fn remaining(self) -> usize {
        self.remaining
    }
}

pub(crate) struct PacketReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PacketReader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) const fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        let offset = self.offset;
        let actual = self.u8()?;
        if actual != expected {
            return Err(ParseError::invalid(offset, u32::from(actual)));
        }
        Ok(())
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16_be(&mut self) -> Result<u16, ParseError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two-byte slice");
        Ok(u16::from_be_bytes(bytes))
    }

    pub(crate) fn i16_be(&mut self) -> Result<i16, ParseError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two-byte slice");
        Ok(i16::from_be_bytes(bytes))
    }

    pub(crate) fn u32_be(&mut self) -> Result<u32, ParseError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("four-byte slice");
        Ok(u32::from_be_bytes(bytes))
    }

    pub(crate) fn string8(&mut self) -> Result<&'a [u8], ParseError> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    pub(crate) fn string16(&mut self) -> Result<&'a [u8], ParseError> {
        let length = usize::from(self.u16_be()?);
        self.take(length)
    }

    pub(crate) fn skip(&mut self, length: usize) -> Result<(), ParseError> {
        self.take(length).map(|_| ())
    }

    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], ParseError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if length > remaining {
            return Err(ParseError::truncated(self.offset, length, remaining));
        }
        let end = self.offset + length;
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    pub(crate) fn invalid_u8(&self, value: u8) -> ParseError {
        ParseError::invalid(self.offset.saturating_sub(1), u32::from(value))
    }

    pub(crate) fn invalid_usize(&self, value: usize) -> ParseError {
        ParseError::invalid(self.offset, u32::try_from(value).unwrap_or(u32::MAX))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerUpdate<'a> {
    ActionDelay(ActionDelay),
    Audio(AudioUpdate),
    Status(StatusUpdate),
    UserAppearance(UserAppearance),
    UserPosition(Position),
    Move(Position),
    Effect(SpelledUpdate),
    World(WorldUpdate),
    Message(ParsedMessage<'a>),
    Collection(CollectionDirty),
    SpellCancelled,
    Visual(VisualUpdate),
    Dialog(&'a [u8]),
    Group(&'a [u8]),
    Exchange(&'a [u8]),
}

pub(crate) fn update<'a>(
    body: &'a [u8],
    objects: &mut RawObjects,
) -> Result<Option<ServerUpdate<'a>>, ParseError> {
    if let Some(update) = action_delay::update(body)? {
        return Ok(Some(ServerUpdate::ActionDelay(update)));
    }
    if matches!(body.first(), Some(0x2F | 0x30)) {
        if body.len() < 2 {
            return Err(ParseError::truncated(1, 1, body.len().saturating_sub(1)));
        }
        return Ok(Some(ServerUpdate::Dialog(body)));
    }
    if matches!(body.first(), Some(0x39 | 0x63)) {
        return Ok(Some(ServerUpdate::Group(body)));
    }
    if body.first() == Some(&0x42) {
        if body.len() < 2 {
            return Err(ParseError::truncated(1, 1, 0));
        }
        return Ok(Some(ServerUpdate::Exchange(body)));
    }
    if let Some(update) = message::update(body)? {
        return Ok(Some(ServerUpdate::Message(update)));
    }
    if let Some(update) = audio::update(body)? {
        return Ok(Some(ServerUpdate::Audio(update)));
    }
    if let Some(update) = object::update(body, objects)? {
        return Ok(Some(ServerUpdate::World(update)));
    }
    if let Some(update) = visual::update(body)? {
        return Ok(Some(ServerUpdate::Visual(update)));
    }
    Ok(state::update(body)?.map(ServerUpdate::from))
}

impl From<StatePacketUpdate> for ServerUpdate<'_> {
    fn from(update: StatePacketUpdate) -> Self {
        match update {
            StatePacketUpdate::Status(value) => Self::Status(value),
            StatePacketUpdate::UserAppearance(value) => Self::UserAppearance(value),
            StatePacketUpdate::UserPosition(value) => Self::UserPosition(value),
            StatePacketUpdate::Move(value) => Self::Move(value),
            StatePacketUpdate::Effect(value) => Self::Effect(value),
            StatePacketUpdate::Collection(value) => Self::Collection(value),
            StatePacketUpdate::SpellCancelled => Self::SpellCancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_unknown_packets() {
        let mut objects = RawObjects::empty();
        assert!(update(&[0xFF], &mut objects).unwrap().is_none());
    }

    #[test]
    fn packet_reader_reports_truncation_at_the_cursor() {
        let mut reader = PacketReader::new(&[1, 2, 3]);
        assert_eq!(reader.u8(), Ok(1));
        assert_eq!(reader.u32_be(), Err(ParseError::truncated(1, 4, 2)));
    }

    #[test]
    fn packet_reader_validates_expected_bytes() {
        let mut reader = PacketReader::new(&[4]);
        assert_eq!(reader.expect(5), Err(ParseError::invalid(0, 4)));
    }
}
