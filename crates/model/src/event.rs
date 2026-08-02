use crate::{CharacterModifiers, CharacterStats, ClientSnapshot, MapLocation};
use std::{error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateEvent {
    pub sequence: u32,
    pub revision: u32,
    pub tick_ms: u32,
    pub update: StateUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateUpdate {
    Status(StatusUpdate),
    Location(LocationUpdate),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocationUpdate {
    pub x: i32,
    pub y: i32,
    /// Present only when this position completes a staged map transition.
    pub map: Option<MapChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapChange {
    pub id: u32,
    pub name: Option<String>,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusUpdate {
    pub core: Option<CoreStatus>,
    pub vitals: Option<CurrentVitals>,
    pub progression: Option<ProgressionStatus>,
    pub gold: Option<u32>,
    pub modifiers: Option<CharacterModifiers>,
    pub is_blinded: Option<bool>,
    pub is_action_restricted: Option<bool>,
}

impl StatusUpdate {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.core.is_none()
            && self.vitals.is_none()
            && self.progression.is_none()
            && self.gold.is_none()
            && self.modifiers.is_none()
            && self.is_blinded.is_none()
            && self.is_action_restricted.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreStatus {
    pub level: u8,
    pub ability_level: u8,
    pub max_health: u32,
    pub max_mana: u32,
    pub weight: u32,
    pub max_weight: u32,
    pub stats: CharacterStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVitals {
    pub health: u32,
    pub mana: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgressionStatus {
    pub experience: u32,
    pub ability_points: u32,
    pub experience_to_next_level: u32,
    pub ability_to_next_level: u32,
}

impl ClientSnapshot {
    pub fn apply_event(&mut self, event: StateEvent) -> Result<(), ApplyEventError> {
        let expected_sequence = next_nonzero(self.event_sequence);
        if event.sequence != expected_sequence {
            return Err(ApplyEventError::UnexpectedSequence {
                expected: expected_sequence,
                actual: event.sequence,
            });
        }
        let expected_revision = next_nonzero(self.revision);
        if event.revision != expected_revision {
            return Err(ApplyEventError::UnexpectedRevision {
                expected: expected_revision,
                actual: event.revision,
            });
        }
        let character = self
            .character
            .as_mut()
            .ok_or(ApplyEventError::CharacterUnavailable)?;
        match event.update {
            StateUpdate::Status(update) => {
                if let Some(core) = update.core {
                    character.progression.level = core.level;
                    character.progression.ability_level = core.ability_level;
                    character.vitals.max_health = core.max_health;
                    character.vitals.max_mana = core.max_mana;
                    character.weight = core.weight;
                    character.max_weight = core.max_weight;
                    character.stats = core.stats;
                }
                if let Some(vitals) = update.vitals {
                    character.vitals.health = vitals.health;
                    character.vitals.mana = vitals.mana;
                }
                if let Some(progression) = update.progression {
                    character.progression.experience = progression.experience;
                    character.progression.ability_points = Some(progression.ability_points);
                    character.progression.experience_to_next_level =
                        Some(progression.experience_to_next_level);
                    character.progression.ability_to_next_level =
                        Some(progression.ability_to_next_level);
                }
                if let Some(gold) = update.gold {
                    character.gold = gold;
                }
                if let Some(modifiers) = update.modifiers {
                    character.modifiers = Some(modifiers);
                }
                if let Some(is_blinded) = update.is_blinded {
                    character.is_blinded = is_blinded;
                }
                if let Some(is_action_restricted) = update.is_action_restricted {
                    character.is_action_restricted = is_action_restricted;
                }
            }
            StateUpdate::Location(update) => {
                if let Some(map) = update.map {
                    character.location = Some(MapLocation {
                        id: map.id,
                        name: map.name,
                        x: Some(update.x),
                        y: Some(update.y),
                        width: map.width,
                        height: map.height,
                    });
                } else {
                    let location = character
                        .location
                        .as_mut()
                        .ok_or(ApplyEventError::LocationUnavailable)?;
                    location.x = Some(update.x);
                    location.y = Some(update.y);
                }
            }
        }
        self.revision = event.revision;
        self.event_sequence = event.sequence;
        self.updated_tick_ms = event.tick_ms;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyEventError {
    UnexpectedSequence { expected: u32, actual: u32 },
    UnexpectedRevision { expected: u32, actual: u32 },
    CharacterUnavailable,
    LocationUnavailable,
}

impl fmt::Display for ApplyEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedSequence { expected, actual } => {
                write!(
                    formatter,
                    "expected event sequence {expected}, received {actual}"
                )
            }
            Self::UnexpectedRevision { expected, actual } => {
                write!(
                    formatter,
                    "expected state revision {expected}, received {actual}"
                )
            }
            Self::CharacterUnavailable => {
                formatter.write_str("state event has no retained character state")
            }
            Self::LocationUnavailable => {
                formatter.write_str("location event has no retained map state")
            }
        }
    }
}

impl Error for ApplyEventError {}

const fn next_nonzero(value: u32) -> u32 {
    let next = value.wrapping_add(1);
    if next == 0 { 1 } else { next }
}
