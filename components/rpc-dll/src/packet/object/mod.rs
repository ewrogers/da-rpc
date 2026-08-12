use crate::packet::{PacketReader as Reader, ParseError};
use darpc_game_client::{MAX_OBJECT_NAME_BYTES, RawObjects, RawWorldObject};

const DRAW_OBJECTS_OPCODE: u8 = 0x07;
const MOVE_OBJECT_OPCODE: u8 = 0x0C;
const REMOVE_OBJECT_OPCODE: u8 = 0x0E;
const CHANGE_DIRECTION_OPCODE: u8 = 0x11;
const DRAW_PLAYER_OPCODE: u8 = 0x33;
const CREATURE_TAG: u16 = 0x4000;
const ITEM_TAG: u16 = 0x8000;
const SPRITE_MASK: u16 = 0x3FFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorldUpdate {
    Draw,
    Move {
        id: u32,
        x: i32,
        y: i32,
        direction: Option<u8>,
    },
    Direction {
        id: u32,
        direction: u8,
    },
    Remove {
        id: u32,
    },
}

pub(crate) fn update(
    body: &[u8],
    objects: &mut RawObjects,
) -> Result<Option<WorldUpdate>, ParseError> {
    objects.clear();
    match body.first().copied() {
        Some(DRAW_OBJECTS_OPCODE) => parse_objects(body, objects).map(|()| Some(WorldUpdate::Draw)),
        Some(DRAW_PLAYER_OPCODE) => parse_player(body, objects).map(|()| Some(WorldUpdate::Draw)),
        Some(MOVE_OBJECT_OPCODE) => parse_move(body).map(Some),
        Some(REMOVE_OBJECT_OPCODE) => parse_remove(body).map(Some),
        Some(CHANGE_DIRECTION_OPCODE) => parse_direction(body).map(Some),
        _ => Ok(None),
    }
}

fn parse_objects(body: &[u8], objects: &mut RawObjects) -> Result<(), ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(DRAW_OBJECTS_OPCODE)?;
    let count = usize::from(reader.u16_be()?);
    for _ in 0..count {
        let x = i32::from(reader.u16_be()?);
        let y = i32::from(reader.u16_be()?);
        let id = reader.u32_be()?;
        let tagged_sprite = reader.u16_be()?;
        let object = match tagged_sprite & 0xC000 {
            CREATURE_TAG => {
                reader.skip(4)?;
                let direction = reader.direction()?;
                reader.skip(1)?;
                let creature_type = reader.u8()?;
                let (name, name_len) = if creature_type == 2 {
                    reader.name()?
                } else {
                    ([0; MAX_OBJECT_NAME_BYTES], 0)
                };
                RawWorldObject::Creature {
                    id,
                    is_npc: creature_type == 2,
                    sprite: Some(tagged_sprite & SPRITE_MASK),
                    name,
                    name_len,
                    x,
                    y,
                    direction,
                }
            }
            ITEM_TAG => {
                reader.skip(3)?;
                RawWorldObject::Item {
                    id,
                    sprite: tagged_sprite & SPRITE_MASK,
                    x,
                    y,
                    z_index: 0,
                }
            }
            _ => continue,
        };
        if !objects.push(object) {
            return Err(reader.invalid_usize(count));
        }
    }
    Ok(())
}

fn parse_player(body: &[u8], objects: &mut RawObjects) -> Result<(), ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(DRAW_PLAYER_OPCODE)?;
    let x = i32::from(reader.u16_be()?);
    let y = i32::from(reader.u16_be()?);
    let direction = reader.direction()?;
    let id = reader.u32_be()?;
    let head_sprite = reader.u16_be()?;
    if head_sprite == u16::MAX {
        reader.skip(10)?;
    } else {
        reader.skip(28)?;
    }
    reader.skip(1)?;
    let (name, name_len) = reader.name()?;
    reader.skip_string8()?;

    let pushed = objects.push(RawWorldObject::Player {
        id,
        name,
        name_len,
        x,
        y,
        direction,
    });
    debug_assert!(pushed);
    Ok(())
}

fn parse_move(body: &[u8]) -> Result<WorldUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(MOVE_OBJECT_OPCODE)?;
    let id = reader.u32_be()?;
    let source_x = i32::from(reader.u16_be()?);
    let source_y = i32::from(reader.u16_be()?);
    let raw_direction = reader.u8()?;
    let (direction, dx, dy) = match raw_direction {
        0..=3 => {
            let (dx, dy) = direction_delta(raw_direction);
            (Some(raw_direction), dx, dy)
        }
        4 => (None, 0, 0),
        actual => return Err(reader.invalid_u8(actual)),
    };
    Ok(WorldUpdate::Move {
        id,
        x: source_x + dx,
        y: source_y + dy,
        direction,
    })
}

fn parse_remove(body: &[u8]) -> Result<WorldUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(REMOVE_OBJECT_OPCODE)?;
    Ok(WorldUpdate::Remove {
        id: reader.u32_be()?,
    })
}

fn parse_direction(body: &[u8]) -> Result<WorldUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(CHANGE_DIRECTION_OPCODE)?;
    Ok(WorldUpdate::Direction {
        id: reader.u32_be()?,
        direction: reader.direction()?,
    })
}

const fn direction_delta(direction: u8) -> (i32, i32) {
    match direction {
        0 => (0, -1),
        1 => (1, 0),
        2 => (0, 1),
        3 => (-1, 0),
        _ => (0, 0),
    }
}

impl Reader<'_> {
    fn direction(&mut self) -> Result<u8, ParseError> {
        let direction = self.u8()?;
        if direction > 3 {
            return Err(self.invalid_u8(direction));
        }
        Ok(direction)
    }

    fn name(&mut self) -> Result<([u8; MAX_OBJECT_NAME_BYTES], u8), ParseError> {
        let length = usize::from(self.u8()?);
        let bytes = self.take(length)?;
        let copied = bytes.len().min(MAX_OBJECT_NAME_BYTES);
        let mut name = [0; MAX_OBJECT_NAME_BYTES];
        name[..copied].copy_from_slice(&bytes[..copied]);
        Ok((
            name,
            u8::try_from(copied).expect("object name length fits u8"),
        ))
    }

    fn skip_string8(&mut self) -> Result<(), ParseError> {
        self.string8().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_draw_move_direction_and_remove_updates() {
        let mut objects = RawObjects::empty();
        let draw = update(
            &[
                0x07, 0, 2, 0, 10, 0, 20, 0, 0, 0, 1, 0x40, 5, 0, 0, 0, 0, 1, 0, 0, 0, 11, 0, 21,
                0, 0, 0, 2, 0x80, 7, 0, 0, 0,
            ],
            &mut objects,
        )
        .unwrap()
        .unwrap();
        assert_eq!(draw, WorldUpdate::Draw);
        assert_eq!(objects.count, 2);

        assert_eq!(
            update(&[0x0C, 0, 0, 0, 1, 0, 10, 0, 20, 1], &mut objects).unwrap(),
            Some(WorldUpdate::Move {
                id: 1,
                x: 11,
                y: 20,
                direction: Some(1),
            })
        );
        assert_eq!(
            update(&[0x0C, 0, 0, 0, 1, 0, 10, 0, 20, 4], &mut objects).unwrap(),
            Some(WorldUpdate::Move {
                id: 1,
                x: 10,
                y: 20,
                direction: None,
            })
        );
        assert_eq!(
            update(&[0x11, 0, 0, 0, 1, 2], &mut objects).unwrap(),
            Some(WorldUpdate::Direction {
                id: 1,
                direction: 2,
            })
        );
        assert_eq!(
            update(&[0x0E, 0, 0, 0, 1], &mut objects).unwrap(),
            Some(WorldUpdate::Remove { id: 1 })
        );
    }

    #[test]
    fn parses_named_npcs_and_players() {
        let mut objects = RawObjects::empty();
        let draw = update(
            &[
                0x07, 0, 1, 0, 10, 0, 20, 0, 0, 0, 1, 0x40, 5, 0, 0, 0, 0, 3, 0, 2, 4, b'M', b'i',
                b'l', b'e',
            ],
            &mut objects,
        )
        .unwrap()
        .unwrap();
        assert_eq!(draw, WorldUpdate::Draw);
        let Some(RawWorldObject::Creature {
            is_npc,
            name,
            name_len,
            direction,
            ..
        }) = objects.entries[0]
        else {
            panic!("expected NPC object");
        };
        assert!(is_npc);
        assert_eq!(direction, 3);
        assert_eq!(&name[..usize::from(name_len)], b"Mile");

        let mut player = vec![0x33, 0, 10, 0, 20, 1, 0, 0, 0, 7, 0, 1];
        player.extend_from_slice(&[0; 28]);
        player.extend_from_slice(&[0, 4, b'S', b'i', b'L', b'o', 0]);
        let draw = update(&player, &mut objects).unwrap().unwrap();
        assert_eq!(draw, WorldUpdate::Draw);
        let Some(RawWorldObject::Player {
            id,
            name,
            name_len,
            x,
            y,
            direction,
        }) = objects.entries[0]
        else {
            panic!("expected player object");
        };
        assert_eq!((id, x, y, direction), (7, 10, 20, 1));
        assert_eq!(&name[..usize::from(name_len)], b"SiLo");
    }

    #[test]
    fn parses_disguised_players_and_ignores_unsupported_draw_ranges() {
        let mut objects = RawObjects::empty();
        let mut player = vec![0x33, 0, 1, 0, 2, 0, 0, 0, 0, 9, 0xFF, 0xFF];
        player.extend_from_slice(&[0; 10]);
        player.extend_from_slice(&[0, 0, 0]);
        let draw = update(&player, &mut objects).unwrap().unwrap();
        assert_eq!(draw, WorldUpdate::Draw);
        assert_eq!(objects.count, 1);

        let draw = update(
            &[
                0x07, 0, 2, 0, 1, 0, 2, 0, 0, 0, 1, 0x00, 5, 0, 3, 0, 4, 0, 0, 0, 2, 0x80, 7, 0, 0,
                0,
            ],
            &mut objects,
        )
        .unwrap()
        .unwrap();
        assert_eq!(draw, WorldUpdate::Draw);
        assert_eq!(objects.count, 1);
        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Item { id: 2, .. })
        ));
    }

    #[test]
    fn rejects_truncated_records_and_invalid_directions() {
        let mut objects = RawObjects::empty();
        assert!(update(&[0x07, 0, 1], &mut objects).is_err());
        assert!(
            update(
                &[
                    0x07, 0, 1, 0, 10, 0, 20, 0, 0, 0, 1, 0x40, 5, 0, 0, 0, 0, 4, 0, 1,
                ],
                &mut objects
            )
            .is_err()
        );
        assert!(update(&[0x0C, 0, 0, 0, 1, 0, 10, 0, 20, 5], &mut objects).is_err());
        assert!(update(&[0x11, 0, 0, 0, 1, 4], &mut objects).is_err());
        assert!(update(&[0x33, 0, 1], &mut objects).is_err());
    }
}
