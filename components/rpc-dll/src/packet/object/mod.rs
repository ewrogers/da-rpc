use crate::packet::{PacketReader as Reader, ParseError};
use darpc_game_client::{
    MAX_OBJECT_NAME_BYTES, RawHumanVisual, RawObjects, RawPlayerVisual, RawWorldObject,
};

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
    DrawPlayer,
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
        Some(DRAW_PLAYER_OPCODE) => {
            parse_player(body, objects).map(|()| Some(WorldUpdate::DrawPlayer))
        }
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
                    is_solid: creature_type != 1,
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
    let visual = if head_sprite == u16::MAX {
        let sprite = reader.u16_be()? & SPRITE_MASK;
        let color = reader.u8()?;
        let boots_color = reader.u8()?;
        let pants_color = reader.u8()?;
        reader.skip(5)?;
        RawPlayerVisual::Creature {
            sprite,
            color,
            boots_color,
            pants_color,
        }
    } else {
        let body_and_pants = reader.u8()?;
        let arms_sprite = reader.u16_be()?;
        let boots_sprite = u16::from(reader.u8()?);
        let armor_sprite = reader.u16_be()?;
        let shield_sprite = u16::from(reader.u8()?);
        let weapon_sprite = reader.u16_be()?;
        let hair_color = reader.u8()?;
        let boots_color = reader.u8()?;
        let accessory1_color = reader.u8()?;
        let accessory1_sprite = reader.u16_be()?;
        let accessory2_color = reader.u8()?;
        let accessory2_sprite = reader.u16_be()?;
        let accessory3_color = reader.u8()?;
        let accessory3_sprite = reader.u16_be()?;
        reader.skip(1)?;
        let rest_position = reader.u8()?;
        let overcoat_sprite = reader.u16_be()?;
        let overcoat_color = reader.u8()?;
        let skin_color = reader.u8()?;
        let packet_translucent = reader.u8()? != 0;
        let face_shape = reader.u8()?;
        let (gender, body_sprite, style_translucent) = decode_body_style(body_and_pants >> 4);
        let pants_color = body_and_pants & 0x0F;
        RawPlayerVisual::Human(RawHumanVisual {
            gender,
            head_sprite,
            body_sprite,
            arms_sprite,
            boots_sprite,
            pants_sprite: u16::from(pants_color != 0),
            armor_sprite,
            weapon_sprite,
            shield_sprite,
            overcoat_sprite,
            accessory1_sprite,
            accessory2_sprite,
            accessory3_sprite,
            hair_color,
            skin_color,
            boots_color,
            pants_color,
            overcoat_color,
            accessory1_color,
            accessory2_color,
            accessory3_color,
            rest_position,
            face_shape,
            is_translucent: packet_translucent || style_translucent,
        })
    };
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
        is_hidden: matches!(
            visual,
            RawPlayerVisual::Human(RawHumanVisual { body_sprite: 0, .. })
                | RawPlayerVisual::Human(RawHumanVisual {
                    is_translucent: true,
                    ..
                })
        ),
        visual: Some(visual),
    });
    debug_assert!(pushed);
    Ok(())
}

const fn decode_body_style(style: u8) -> (u8, u16, bool) {
    match style {
        0 => (0, 0, false),
        1 => (0, 1, false),
        2 => (1, 1, false),
        3 => (0, 2, false),
        4 => (1, 2, false),
        5 => (0, 1, true),
        6 => (1, 1, true),
        7 => (0, 4, false),
        8 => (0, 5, false),
        9 => (1, 5, false),
        other => (0, other as u16, false),
    }
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
            is_solid,
            name,
            name_len,
            direction,
            ..
        }) = objects.entries[0]
        else {
            panic!("expected NPC object");
        };
        assert!(is_npc);
        assert!(is_solid);
        assert_eq!(direction, 3);
        assert_eq!(&name[..usize::from(name_len)], b"Mile");

        let mut player = vec![0x33, 0, 10, 0, 20, 1, 0, 0, 0, 7, 0, 1];
        player.extend_from_slice(&[0; 28]);
        player.extend_from_slice(&[0, 4, b'S', b'i', b'L', b'o', 0]);
        let draw = update(&player, &mut objects).unwrap().unwrap();
        assert_eq!(draw, WorldUpdate::DrawPlayer);
        let Some(RawWorldObject::Player {
            id,
            name,
            name_len,
            x,
            y,
            direction,
            is_hidden,
            ..
        }) = objects.entries[0]
        else {
            panic!("expected player object");
        };
        assert_eq!((id, x, y, direction), (7, 10, 20, 1));
        assert!(is_hidden);
        assert_eq!(&name[..usize::from(name_len)], b"SiLo");
    }

    #[test]
    fn preserves_monster_solidity() {
        let mut objects = RawObjects::empty();
        update(
            &[
                0x07, 0, 2, 0, 10, 0, 20, 0, 0, 0, 1, 0x40, 5, 0, 0, 0, 0, 1, 0, 0, 0, 11, 0, 20,
                0, 0, 0, 2, 0x40, 6, 0, 0, 0, 0, 2, 0, 1,
            ],
            &mut objects,
        )
        .unwrap();

        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Creature {
                is_npc: false,
                is_solid: true,
                ..
            })
        ));
        assert!(matches!(
            objects.entries[1],
            Some(RawWorldObject::Creature {
                is_npc: false,
                is_solid: false,
                ..
            })
        ));
    }

    #[test]
    fn parses_disguised_players_and_ignores_unsupported_draw_ranges() {
        let mut objects = RawObjects::empty();
        let mut player = vec![0x33, 0, 1, 0, 2, 0, 0, 0, 0, 9, 0xFF, 0xFF];
        player.extend_from_slice(&[0; 10]);
        player.extend_from_slice(&[0, 0, 0]);
        let draw = update(&player, &mut objects).unwrap().unwrap();
        assert_eq!(draw, WorldUpdate::DrawPlayer);
        assert_eq!(objects.count, 1);
        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Player {
                is_hidden: false,
                ..
            })
        ));

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
    fn parses_translucent_players_as_hidden() {
        let mut objects = RawObjects::empty();
        let mut player = vec![0x33, 0, 10, 0, 20, 1, 0, 0, 0, 7, 0, 1];
        player.extend_from_slice(&[0; 28]);
        player[12] = 80;
        player[38] = 1;
        player.extend_from_slice(&[0, 0, 0]);

        assert_eq!(
            update(&player, &mut objects).unwrap(),
            Some(WorldUpdate::DrawPlayer)
        );
        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Player {
                is_hidden: true,
                ..
            })
        ));
    }

    #[test]
    fn parses_every_human_visual_field() {
        let mut objects = RawObjects::empty();
        let player = [
            0x33, 0, 10, 0, 20, 1, 0, 0, 0, 7, 0x01, 0x23, 0x4A, 0x02, 0x03, 4, 0x05, 0x06, 7,
            0x08, 0x09, 10, 11, 12, 0x0D, 0x0E, 15, 0x10, 0x11, 18, 0x13, 0x14, 21, 22, 0x17, 0x18,
            25, 26, 0, 27, 28, 1, b'X', 0,
        ];

        assert_eq!(
            update(&player, &mut objects).unwrap(),
            Some(WorldUpdate::DrawPlayer)
        );
        let Some(RawWorldObject::Player {
            visual: Some(visual),
            is_hidden,
            ..
        }) = objects.entries[0]
        else {
            panic!("expected player visual");
        };
        assert!(!is_hidden);
        assert_eq!(
            visual,
            RawPlayerVisual::Human(RawHumanVisual {
                gender: 1,
                head_sprite: 0x0123,
                body_sprite: 2,
                arms_sprite: 0x0203,
                boots_sprite: 4,
                pants_sprite: 1,
                armor_sprite: 0x0506,
                weapon_sprite: 0x0809,
                shield_sprite: 7,
                overcoat_sprite: 0x1718,
                accessory1_sprite: 0x0D0E,
                accessory2_sprite: 0x1011,
                accessory3_sprite: 0x1314,
                hair_color: 10,
                skin_color: 26,
                boots_color: 11,
                pants_color: 10,
                overcoat_color: 25,
                accessory1_color: 12,
                accessory2_color: 15,
                accessory3_color: 18,
                rest_position: 22,
                face_shape: 27,
                is_translucent: false,
            })
        );
    }

    #[test]
    fn parses_transformed_player_visual_fields() {
        let mut objects = RawObjects::empty();
        let player = [
            0x33, 0, 1, 0, 2, 0, 0, 0, 0, 9, 0xFF, 0xFF, 0x41, 0x23, 4, 5, 6, 0, 0, 0, 0, 0, 7, 0,
            0,
        ];

        update(&player, &mut objects).unwrap();
        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Player {
                visual: Some(RawPlayerVisual::Creature {
                    sprite: 0x0123,
                    color: 4,
                    boots_color: 5,
                    pants_color: 6,
                }),
                is_hidden: false,
                ..
            })
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
