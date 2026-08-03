pub(crate) mod message;
pub(crate) mod object;
mod state;

use self::{
    message::ParsedMessage,
    object::WorldUpdate,
    state::{CollectionDirty, Position, SpelledUpdate, StatePacketUpdate, UserAppearance},
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerUpdate<'a> {
    Status(StatusUpdate),
    UserAppearance(UserAppearance),
    UserPosition(Position),
    Move(Position),
    Effect(SpelledUpdate),
    World(WorldUpdate),
    Message(ParsedMessage<'a>),
    Collection(CollectionDirty),
    SpellCancelled,
}

pub(crate) fn update<'a>(
    body: &'a [u8],
    objects: &mut RawObjects,
) -> Result<Option<ServerUpdate<'a>>, ParseError> {
    if let Some(update) = message::update(body)? {
        return Ok(Some(ServerUpdate::Message(update)));
    }
    if let Some(update) = object::update(body, objects)? {
        return Ok(Some(ServerUpdate::World(update)));
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
}
