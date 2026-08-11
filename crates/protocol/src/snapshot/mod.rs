use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_i32, push_u16, push_u32},
};
use darpc_model::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot, Element,
    Gender, MapLocation, PlannedRoute, TilePosition,
};

pub const MAX_CHARACTER_NAME_LEN: usize = 15;
pub const MAX_MAP_NAME_LEN: usize = 255;
pub const MAX_PLANNED_ROUTE_TILES: usize = 400 * 400 + 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotRequest {
    pub request_id: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotResponse {
    pub request_id: u32,
    pub result: SnapshotResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotResult {
    Ready(Box<ClientSnapshot>),
    Unavailable(SnapshotUnavailableReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotUnavailableReason {
    HookUnavailable,
    CaptureTimedOut,
    CaptureFailed,
}

impl SnapshotUnavailableReason {
    pub(crate) const fn wire_value(self) -> u8 {
        match self {
            Self::HookUnavailable => 1,
            Self::CaptureTimedOut => 2,
            Self::CaptureFailed => 3,
        }
    }

    pub(crate) fn from_wire(value: u8) -> Result<Self, DecodeError> {
        match value {
            1 => Ok(Self::HookUnavailable),
            2 => Ok(Self::CaptureTimedOut),
            3 => Ok(Self::CaptureFailed),
            actual => Err(DecodeError::InvalidSnapshotUnavailableReason { actual }),
        }
    }
}

pub(crate) fn encode(output: &mut Vec<u8>, snapshot: &ClientSnapshot) -> Result<(), EncodeError> {
    push_u32(output, snapshot.revision);
    push_u32(output, snapshot.event_sequence);
    push_u32(output, snapshot.captured_tick_ms);
    push_u32(output, snapshot.updated_tick_ms);
    push_u32(output, snapshot.capture_duration_us);
    push_u32(output, snapshot.world_generation);
    output.push(lifecycle_wire(snapshot.lifecycle));
    push_bool(output, snapshot.character.is_some());
    if let Some(character) = &snapshot.character {
        encode_character(output, character)?;
    }
    objects::encode(output, snapshot.objects.as_deref())?;
    crate::dialog::encode_optional_state(output, snapshot.dialog.as_ref())?;
    crate::group::encode_optional_state(output, snapshot.group.as_ref())?;
    crate::exchange::encode_optional_state(output, snapshot.exchange.as_ref())?;
    crate::legend::encode_optional(output, snapshot.legend.as_deref())?;
    crate::player::encode_optional_identity(
        output,
        snapshot
            .character
            .as_ref()
            .and_then(|character| character.identity.as_ref()),
    )?;
    encode_player_profiles(output, snapshot.objects.as_deref())?;
    encode_optional_planned_route(output, snapshot.planned_route.as_ref())?;
    Ok(())
}

pub(crate) fn decode(reader: &mut PayloadReader<'_>) -> Result<ClientSnapshot, DecodeError> {
    let revision = reader.read_u32()?;
    let event_sequence = reader.read_u32()?;
    let captured_tick_ms = reader.read_u32()?;
    let updated_tick_ms = reader.read_u32()?;
    let capture_duration_us = reader.read_u32()?;
    let world_generation = reader.read_u32()?;
    let lifecycle = lifecycle_from_wire(reader.read_u8()?)?;
    let character = if reader.read_bool()? {
        Some(decode_character(reader)?)
    } else {
        None
    };
    let objects = objects::decode(reader)?;
    let dialog = if reader.is_empty() {
        None
    } else {
        crate::dialog::decode_optional_state(reader)?
    };
    let group = if reader.is_empty() {
        None
    } else {
        crate::group::decode_optional_state(reader)?
    };
    let exchange = if reader.is_empty() {
        None
    } else {
        crate::exchange::decode_optional_state(reader)?
    };
    let legend = if reader.is_empty() {
        None
    } else {
        crate::legend::decode_optional(reader)?
    };
    let identity = if reader.is_empty() {
        None
    } else {
        crate::player::decode_optional_identity(reader)?
    };
    let mut objects = objects;
    if !reader.is_empty() {
        decode_player_profiles(reader, objects.as_deref_mut())?;
    }
    let planned_route = if reader.is_empty() {
        None
    } else {
        decode_optional_planned_route(reader)?
    };
    let mut snapshot = ClientSnapshot {
        revision,
        event_sequence,
        captured_tick_ms,
        updated_tick_ms,
        capture_duration_us,
        world_generation,
        lifecycle,
        character,
        objects,
        dialog,
        group,
        exchange,
        legend,
        planned_route,
    };
    if let Some(character) = snapshot.character.as_mut() {
        character.identity = identity;
    }
    Ok(snapshot)
}

pub(crate) fn encode_planned_route(
    output: &mut Vec<u8>,
    route: &PlannedRoute,
) -> Result<(), EncodeError> {
    if route.tiles.len() > MAX_PLANNED_ROUTE_TILES {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: route.tiles.len(),
            max: MAX_PLANNED_ROUTE_TILES,
        });
    }
    push_u32(output, route.generation);
    push_u32(
        output,
        u32::try_from(route.tiles.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    for tile in &route.tiles {
        push_u16(
            output,
            u16::try_from(tile.x).map_err(|_| EncodeError::LengthOverflow)?,
        );
        push_u16(
            output,
            u16::try_from(tile.y).map_err(|_| EncodeError::LengthOverflow)?,
        );
    }
    Ok(())
}

pub(crate) fn decode_planned_route(
    reader: &mut PayloadReader<'_>,
) -> Result<PlannedRoute, DecodeError> {
    let generation = reader.read_u32()?;
    let length = usize::try_from(reader.read_u32()?).map_err(|_| DecodeError::LengthOverflow)?;
    if length > MAX_PLANNED_ROUTE_TILES {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length,
            max: MAX_PLANNED_ROUTE_TILES,
        });
    }
    let mut tiles = Vec::with_capacity(length);
    for _ in 0..length {
        tiles.push(TilePosition {
            x: i32::from(reader.read_u16()?),
            y: i32::from(reader.read_u16()?),
        });
    }
    Ok(PlannedRoute { generation, tiles })
}

fn encode_optional_planned_route(
    output: &mut Vec<u8>,
    route: Option<&PlannedRoute>,
) -> Result<(), EncodeError> {
    push_bool(output, route.is_some());
    if let Some(route) = route {
        encode_planned_route(output, route)?;
    }
    Ok(())
}

fn decode_optional_planned_route(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<PlannedRoute>, DecodeError> {
    if reader.read_bool()? {
        decode_planned_route(reader).map(Some)
    } else {
        Ok(None)
    }
}

fn encode_character(
    output: &mut Vec<u8>,
    character: &CharacterSnapshot,
) -> Result<(), EncodeError> {
    encode_optional_u32(output, character.id);
    encode_optional_string(output, character.name.as_deref(), MAX_CHARACTER_NAME_LEN)?;
    match character.appearance {
        Some(appearance) => {
            output.push(1);
            output.push(appearance.gender.raw());
            push_u16(output, appearance.hair_style);
            output.push(appearance.hair_color);
            push_u16(output, appearance.body_sprite);
        }
        None => output.push(0),
    }
    output.push(character.class.raw());
    push_bool(output, character.is_action_restricted);
    push_bool(output, character.is_blinded);
    push_bool(output, character.is_casting);
    push_bool(output, character.is_walking);
    push_u32(output, character.gold);
    push_u32(output, character.weight);
    push_u32(output, character.max_weight);

    output.push(character.progression.level);
    output.push(character.progression.ability_level);
    push_u32(output, character.progression.experience);
    let pane_progression = character.progression.ability_points.zip(
        character
            .progression
            .experience_to_next_level
            .zip(character.progression.ability_to_next_level),
    );
    push_bool(output, pane_progression.is_some());
    if let Some((ability_points, (experience_to_next_level, ability_to_next_level))) =
        pane_progression
    {
        push_u32(output, ability_points);
        push_u32(output, experience_to_next_level);
        push_u32(output, ability_to_next_level);
    }

    push_u16(output, character.stats.strength);
    push_u16(output, character.stats.intelligence);
    push_u16(output, character.stats.wisdom);
    push_u16(output, character.stats.constitution);
    push_u16(output, character.stats.dexterity);
    push_u32(output, character.vitals.health);
    push_u32(output, character.vitals.max_health);
    push_u32(output, character.vitals.mana);
    push_u32(output, character.vitals.max_mana);

    push_bool(output, character.modifiers.is_some());
    if let Some(modifiers) = character.modifiers {
        output.push(modifiers.armor_class as u8);
        output.push(modifiers.damage);
        output.push(modifiers.hit);
        push_u16(output, modifiers.magic_resistance);
        push_u16(output, modifiers.attack_element.raw());
        push_u16(output, modifiers.defense_element.raw());
    }

    push_bool(output, character.location.is_some());
    if let Some(location) = &character.location {
        push_u32(output, location.id);
        encode_optional_string(output, location.name.as_deref(), MAX_MAP_NAME_LEN)?;
        let position = location.x.zip(location.y);
        push_bool(output, position.is_some());
        if let Some((x, y)) = position {
            push_i32(output, x);
            push_i32(output, y);
        }
        push_i32(output, location.width);
        push_i32(output, location.height);
    }
    collections::encode(output, character)?;
    Ok(())
}

fn decode_character(reader: &mut PayloadReader<'_>) -> Result<CharacterSnapshot, DecodeError> {
    let id = decode_optional_u32(reader)?;
    let name = decode_optional_string(reader, MAX_CHARACTER_NAME_LEN)?;
    let appearance = if reader.read_bool()? {
        Some(CharacterAppearance {
            gender: Gender::from_raw(reader.read_u8()?),
            hair_style: reader.read_u16()?,
            hair_color: reader.read_u8()?,
            body_sprite: reader.read_u16()?,
        })
    } else {
        None
    };
    let class = CharacterClass::from_raw(reader.read_u8()?);
    let is_action_restricted = reader.read_bool()?;
    let is_blinded = reader.read_bool()?;
    let is_casting = reader.read_bool()?;
    let is_walking = reader.read_bool()?;
    let gold = reader.read_u32()?;
    let weight = reader.read_u32()?;
    let max_weight = reader.read_u32()?;
    let level = reader.read_u8()?;
    let ability_level = reader.read_u8()?;
    let experience = reader.read_u32()?;
    let (ability_points, experience_to_next_level, ability_to_next_level) = if reader.read_bool()? {
        (
            Some(reader.read_u32()?),
            Some(reader.read_u32()?),
            Some(reader.read_u32()?),
        )
    } else {
        (None, None, None)
    };
    let stats = CharacterStats {
        strength: reader.read_u16()?,
        intelligence: reader.read_u16()?,
        wisdom: reader.read_u16()?,
        constitution: reader.read_u16()?,
        dexterity: reader.read_u16()?,
    };
    let vitals = CharacterVitals {
        health: reader.read_u32()?,
        max_health: reader.read_u32()?,
        mana: reader.read_u32()?,
        max_mana: reader.read_u32()?,
    };
    let modifiers = if reader.read_bool()? {
        Some(CharacterModifiers {
            armor_class: reader.read_i8()?,
            damage: reader.read_u8()?,
            hit: reader.read_u8()?,
            magic_resistance: reader.read_u16()?,
            attack_element: Element::from_raw(reader.read_u16()?),
            defense_element: Element::from_raw(reader.read_u16()?),
        })
    } else {
        None
    };
    let location = if reader.read_bool()? {
        let id = reader.read_u32()?;
        let name = decode_optional_string(reader, MAX_MAP_NAME_LEN)?;
        let (x, y) = if reader.read_bool()? {
            (Some(reader.read_i32()?), Some(reader.read_i32()?))
        } else {
            (None, None)
        };
        Some(MapLocation {
            id,
            name,
            x,
            y,
            width: reader.read_i32()?,
            height: reader.read_i32()?,
        })
    } else {
        None
    };
    let collections = collections::decode(reader)?;
    Ok(CharacterSnapshot {
        id,
        name,
        identity: None,
        appearance,
        class,
        is_action_restricted,
        is_blinded,
        is_casting,
        is_walking,
        gold,
        weight,
        max_weight,
        progression: CharacterProgression {
            level,
            ability_level,
            experience,
            ability_points,
            experience_to_next_level,
            ability_to_next_level,
        },
        stats,
        vitals,
        modifiers,
        location,
        inventory: collections.inventory,
        equipment: collections.equipment,
        spellbook: collections.spellbook,
        skillbook: collections.skillbook,
        effects: collections.effects,
    })
}

fn encode_player_profiles(
    output: &mut Vec<u8>,
    objects: Option<&[darpc_model::WorldObject]>,
) -> Result<(), EncodeError> {
    let profiles = objects
        .into_iter()
        .flatten()
        .filter_map(|object| match object {
            darpc_model::WorldObject::Player {
                id,
                profile: Some(profile),
                ..
            } => Some((*id, profile)),
            _ => None,
        })
        .collect::<Vec<_>>();
    push_u16(
        output,
        u16::try_from(profiles.len()).map_err(|_| EncodeError::LengthOverflow)?,
    );
    for (id, profile) in profiles {
        push_u32(output, id);
        crate::player::encode_profile(output, profile)?;
    }
    Ok(())
}

fn decode_player_profiles(
    reader: &mut PayloadReader<'_>,
    objects: Option<&mut [darpc_model::WorldObject]>,
) -> Result<(), DecodeError> {
    let count = usize::from(reader.read_u16()?);
    let mut seen = Vec::with_capacity(count);
    let mut objects = objects;
    for _ in 0..count {
        let id = reader.read_u32()?;
        if seen.contains(&id) {
            return Err(DecodeError::DuplicateWorldObjectId { id });
        }
        seen.push(id);
        let profile = crate::player::decode_profile(reader)?;
        let player = objects
            .as_deref_mut()
            .and_then(|objects| objects.iter_mut().find(|object| object.id() == id));
        match player {
            Some(darpc_model::WorldObject::Player {
                profile: current, ..
            }) => *current = Some(Box::new(profile)),
            _ => return Err(DecodeError::InvalidPlayerProfileTarget { id }),
        }
    }
    Ok(())
}

pub(crate) fn lifecycle_wire(lifecycle: ClientLifecycle) -> u8 {
    match lifecycle {
        ClientLifecycle::Unknown => 0,
        ClientLifecycle::Title => 1,
        ClientLifecycle::Transition => 2,
        ClientLifecycle::InGame => 3,
        ClientLifecycle::Disconnected => 4,
    }
}

pub(crate) fn lifecycle_from_wire(value: u8) -> Result<ClientLifecycle, DecodeError> {
    match value {
        0 => Ok(ClientLifecycle::Unknown),
        1 => Ok(ClientLifecycle::Title),
        2 => Ok(ClientLifecycle::Transition),
        3 => Ok(ClientLifecycle::InGame),
        4 => Ok(ClientLifecycle::Disconnected),
        actual => Err(DecodeError::InvalidClientLifecycle { actual }),
    }
}

fn encode_optional_u32(output: &mut Vec<u8>, value: Option<u32>) {
    push_bool(output, value.is_some());
    if let Some(value) = value {
        push_u32(output, value);
    }
}

fn decode_optional_u32(reader: &mut PayloadReader<'_>) -> Result<Option<u32>, DecodeError> {
    if reader.read_bool()? {
        Ok(Some(reader.read_u32()?))
    } else {
        Ok(None)
    }
}

pub(crate) fn encode_optional_string(
    output: &mut Vec<u8>,
    value: Option<&str>,
    max: usize,
) -> Result<(), EncodeError> {
    push_bool(output, value.is_some());
    let Some(value) = value else {
        return Ok(());
    };
    let bytes = value.as_bytes();
    if bytes.len() > max {
        return Err(EncodeError::SnapshotStringTooLong {
            length: bytes.len(),
            max,
        });
    }
    let length = u16::try_from(bytes.len()).map_err(|_| EncodeError::LengthOverflow)?;
    push_u16(output, length);
    output.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn decode_optional_string(
    reader: &mut PayloadReader<'_>,
    max: usize,
) -> Result<Option<String>, DecodeError> {
    if !reader.read_bool()? {
        return Ok(None);
    }
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::SnapshotStringTooLong { length, max });
    }
    let value = std::str::from_utf8(reader.take(length)?)
        .map_err(|_| DecodeError::InvalidUtf8)?
        .to_owned();
    Ok(Some(value))
}
pub(crate) mod collections;
pub(crate) mod objects;
