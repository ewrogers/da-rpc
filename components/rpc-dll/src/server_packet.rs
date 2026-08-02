use darpc_model::{
    CharacterModifiers, CharacterStats, CoreStatus, CurrentVitals, Element, ProgressionStatus,
    StatusUpdate,
};
use std::{error::Error, fmt};

const USER_APPEARANCE_OPCODE: u8 = 0x05;
const USER_POSITION_OPCODE: u8 = 0x04;
const STATUS_OPCODE: u8 = 0x08;
const MOVE_OPCODE: u8 = 0x0B;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerUpdate {
    Status(StatusUpdate),
    UserPosition(Position),
    Move(Position),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Position {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

pub(crate) fn update(body: &[u8]) -> Result<Option<ServerUpdate>, ParseError> {
    match body.first().copied() {
        Some(USER_POSITION_OPCODE) => parse_user_position(body)
            .map(ServerUpdate::UserPosition)
            .map(Some),
        Some(USER_APPEARANCE_OPCODE) => parse_user_appearance(body)
            .map(ServerUpdate::Status)
            .map(Some),
        Some(STATUS_OPCODE) => parse_status(body).map(ServerUpdate::Status).map(Some),
        Some(MOVE_OPCODE) => parse_move(body).map(|position| position.map(ServerUpdate::Move)),
        _ => Ok(None),
    }
}

fn parse_user_position(body: &[u8]) -> Result<Position, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(USER_POSITION_OPCODE)?;
    Ok(Position {
        x: i32::from(reader.i16_be()?),
        y: i32::from(reader.i16_be()?),
    })
}

fn parse_move(body: &[u8]) -> Result<Option<Position>, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(MOVE_OPCODE)?;
    let direction = reader.u8()?;
    let previous_x = i32::from(reader.i16_be()?);
    let previous_y = i32::from(reader.i16_be()?);
    reader.skip(2 + 2 + 1)?;
    let (dx, dy) = match direction {
        0 => (0, -1),
        1 => (1, 0),
        2 => (0, 1),
        3 => (-1, 0),
        4 => (0, 0),
        _ => return Ok(None),
    };
    Ok(Some(Position {
        x: previous_x + dx,
        y: previous_y + dy,
    }))
}

fn parse_status(body: &[u8]) -> Result<StatusUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(STATUS_OPCODE)?;
    let fields = reader.u8()?;
    let core = if fields & 0x20 != 0 {
        reader.skip(3)?;
        let level = reader.u8()?;
        let ability_level = reader.u8()?;
        let max_health = reader.u32_be()?;
        let max_mana = reader.u32_be()?;
        let stats = CharacterStats {
            strength: u16::from(reader.u8()?),
            intelligence: u16::from(reader.u8()?),
            wisdom: u16::from(reader.u8()?),
            constitution: u16::from(reader.u8()?),
            dexterity: u16::from(reader.u8()?),
        };
        reader.skip(2)?;
        let max_weight = u32::from(reader.u16_be()?);
        let weight = u32::from(reader.u16_be()?);
        reader.skip(4)?;
        Some(CoreStatus {
            level,
            ability_level,
            max_health,
            max_mana,
            weight,
            max_weight,
            stats,
        })
    } else {
        None
    };
    let vitals = if fields & 0x10 != 0 {
        Some(CurrentVitals {
            health: reader.u32_be()?,
            mana: reader.u32_be()?,
        })
    } else {
        None
    };
    let (progression, gold) = if fields & 0x08 != 0 {
        let experience = reader.u32_be()?;
        let experience_to_next_level = reader.u32_be()?;
        let ability_points = reader.u32_be()?;
        let ability_to_next_level = reader.u32_be()?;
        reader.skip(4)?;
        let gold = reader.u32_be()?;
        (
            Some(ProgressionStatus {
                experience,
                ability_points,
                experience_to_next_level,
                ability_to_next_level,
            }),
            Some(gold),
        )
    } else {
        (None, None)
    };
    let (modifiers, is_blinded) = if fields & 0x04 != 0 {
        reader.skip(1)?;
        let blind_code = reader.u8()?;
        reader.skip(4)?;
        let attack_element = Element::from_raw(u16::from(reader.u8()?));
        let defense_element = Element::from_raw(u16::from(reader.u8()?));
        let magic_resistance = u16::from(reader.u8()?).saturating_mul(10);
        reader.skip(1)?;
        let armor_class = reader.u8()? as i8;
        let damage = reader.u8()?;
        let hit = reader.u8()?;
        (
            Some(CharacterModifiers {
                armor_class,
                damage,
                hit,
                magic_resistance,
                attack_element,
                defense_element,
            }),
            Some(blind_code == 0x08),
        )
    } else {
        (None, None)
    };

    Ok(StatusUpdate {
        core,
        vitals,
        progression,
        gold,
        modifiers,
        is_blinded,
        is_action_restricted: None,
    })
}

fn parse_user_appearance(body: &[u8]) -> Result<StatusUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(USER_APPEARANCE_OPCODE)?;
    reader.skip(4 + 3)?;
    let action_state = reader.u8()?;
    reader.skip(1)?;
    Ok(StatusUpdate {
        is_action_restricted: Some(action_state & 0x01 != 0),
        ..StatusUpdate::default()
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseError {
    offset: usize,
    needed: usize,
    remaining: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "server packet truncated at offset {}: need {} bytes, {} remain",
            self.offset, self.needed, self.remaining
        )
    }
}

impl Error for ParseError {}

impl ParseError {
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

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        let actual = self.u8()?;
        debug_assert_eq!(actual, expected);
        Ok(())
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    fn u32_be(&mut self) -> Result<u32, ParseError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("four-byte slice");
        Ok(u32::from_be_bytes(bytes))
    }

    fn u16_be(&mut self) -> Result<u16, ParseError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two-byte slice");
        Ok(u16::from_be_bytes(bytes))
    }

    fn i16_be(&mut self) -> Result<i16, ParseError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two-byte slice");
        Ok(i16::from_be_bytes(bytes))
    }

    fn skip(&mut self, length: usize) -> Result<(), ParseError> {
        self.take(length).map(|_| ())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ParseError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if length > remaining {
            return Err(ParseError {
                offset: self.offset,
                needed: length,
                remaining,
            });
        }
        let end = self.offset + length;
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(body: &[u8]) -> StatusUpdate {
        let ServerUpdate::Status(update) = update(body).unwrap().unwrap() else {
            panic!("expected status update");
        };
        update
    }

    #[test]
    fn parses_every_status_group() {
        let mut body = vec![0x08, 0x3C];
        body.extend_from_slice(&[2, 0, 0, 99, 7]);
        body.extend_from_slice(&1_024_u32.to_be_bytes());
        body.extend_from_slice(&768_u32.to_be_bytes());
        body.extend_from_slice(&[11, 12, 13, 14, 15, 1, 4]);
        body.extend_from_slice(&100_u16.to_be_bytes());
        body.extend_from_slice(&50_u16.to_be_bytes());
        body.extend_from_slice(&0_u32.to_be_bytes());
        body.extend_from_slice(&900_u32.to_be_bytes());
        body.extend_from_slice(&700_u32.to_be_bytes());
        body.extend_from_slice(&100_u32.to_be_bytes());
        body.extend_from_slice(&20_u32.to_be_bytes());
        body.extend_from_slice(&30_u32.to_be_bytes());
        body.extend_from_slice(&40_u32.to_be_bytes());
        body.extend_from_slice(&50_u32.to_be_bytes());
        body.extend_from_slice(&60_u32.to_be_bytes());
        body.extend_from_slice(&[0, 0x08, 0, 0, 0, 0, 1, 2, 6, 0, 0xF6, 8, 9, 0]);
        let update = status(&body);
        assert_eq!(update.core.unwrap().level, 99);
        assert_eq!(update.core.unwrap().weight, 50);
        assert_eq!(update.core.unwrap().max_weight, 100);
        assert_eq!(update.vitals.unwrap().health, 900);
        assert_eq!(update.progression.unwrap().ability_points, 30);
        assert_eq!(update.gold, Some(60));
        assert_eq!(update.modifiers.unwrap().armor_class, -10);
        assert_eq!(update.modifiers.unwrap().magic_resistance, 60);
        assert_eq!(update.is_blinded, Some(true));
        assert_eq!(update.is_action_restricted, None);
    }

    #[test]
    fn parses_full_and_partial_action_state() {
        let unlocked = [0x05, 0, 0, 0, 1, 2, 0, 3, 0, 0];
        let locked = [0x05, 0, 0, 0, 1, 2, 0, 3, 0x81, 0];
        assert_eq!(status(&unlocked).is_action_restricted, Some(false));
        assert_eq!(status(&locked).is_action_restricted, Some(true));
    }

    #[test]
    fn parses_vitals_modifiers_and_active_mail_state() {
        let mut body = vec![0x08, 0x15];
        body.extend_from_slice(&900_u32.to_be_bytes());
        body.extend_from_slice(&700_u32.to_be_bytes());
        body.extend_from_slice(&[0, 0x08, 0, 0, 0, 0x30, 1, 2, 6, 0, 0xF6, 8, 9]);
        assert_eq!(body.len(), 23);

        let update = status(&body);
        assert_eq!(update.vitals.unwrap().health, 900);
        assert_eq!(update.vitals.unwrap().mana, 700);
        assert_eq!(update.modifiers.unwrap().armor_class, -10);
        assert_eq!(update.modifiers.unwrap().magic_resistance, 60);
        assert_eq!(update.is_blinded, Some(true));
    }

    #[test]
    fn rejects_every_truncated_status_prefix() {
        let body = [0x08, 0x10, 0, 0, 0, 1, 0, 0, 0, 2];
        for length in 0..body.len() {
            if length == 0 {
                assert_eq!(update(&body[..length]).unwrap(), None);
            } else {
                assert!(update(&body[..length]).is_err());
            }
        }
        assert!(update(&body).is_ok());

        let error = update(&body[..body.len() - 1]).unwrap_err();
        assert_eq!(error.offset(), 6);
        assert_eq!(error.needed(), 4);
        assert_eq!(error.remaining(), 3);
    }
    #[test]
    fn parses_authoritative_and_acknowledged_positions() {
        assert_eq!(
            update(&[0x04, 0, 43, 0, 40, 0, 11, 0, 11]).unwrap(),
            Some(ServerUpdate::UserPosition(Position { x: 43, y: 40 }))
        );
        assert_eq!(
            update(&[0x0B, 1, 0, 43, 0, 40, 0, 0, 0, 0, 7]).unwrap(),
            Some(ServerUpdate::Move(Position { x: 44, y: 40 }))
        );
        assert_eq!(
            update(&[0x0B, 4, 0, 43, 0, 40, 0, 0, 0, 0, 7]).unwrap(),
            Some(ServerUpdate::Move(Position { x: 43, y: 40 }))
        );
        assert_eq!(
            update(&[0x0B, 5, 0, 43, 0, 40, 0, 0, 0, 0, 7]).unwrap(),
            None
        );
    }
}
