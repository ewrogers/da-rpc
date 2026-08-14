pub const MAX_WORLD_OBJECTS: usize = 512;
pub const MAX_OBJECT_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawHumanVisual {
    pub resource_prefix: u8,
    pub head_sprite: u16,
    pub body_sprite: u16,
    pub arms_sprite: u16,
    pub boots_sprite: u16,
    pub pants_sprite: u16,
    pub armor_sprite: u16,
    pub weapon_sprite: u16,
    pub shield_sprite: u16,
    pub overcoat_sprite: u16,
    pub accessory1_sprite: u16,
    pub accessory2_sprite: u16,
    pub accessory3_sprite: u16,
    pub hair_color: u8,
    pub skin_color: u8,
    pub boots_color: u8,
    pub pants_color: u8,
    pub overcoat_color: u8,
    pub accessory1_color: u8,
    pub accessory2_color: u8,
    pub accessory3_color: u8,
    pub rest_position: u8,
    pub face_shape: u8,
    pub is_translucent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawPlayerVisual {
    Human(RawHumanVisual),
    Creature {
        sprite: u16,
        color: u8,
        boots_color: u8,
        pants_color: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawWorldObject {
    Player {
        id: u32,
        name: [u8; MAX_OBJECT_NAME_BYTES],
        name_len: u8,
        x: i32,
        y: i32,
        direction: u8,
        is_hidden: bool,
        visual: Option<RawPlayerVisual>,
    },
    Creature {
        id: u32,
        is_npc: bool,
        sprite: Option<u16>,
        name: [u8; MAX_OBJECT_NAME_BYTES],
        name_len: u8,
        x: i32,
        y: i32,
        direction: u8,
    },
    Item {
        id: u32,
        sprite: u16,
        x: i32,
        y: i32,
        z_index: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawObjects {
    pub entries: [Option<RawWorldObject>; MAX_WORLD_OBJECTS],
    pub count: u16,
}

impl RawObjects {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: [None; MAX_WORLD_OBJECTS],
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
        self.count = 0;
    }

    pub fn push(&mut self, mut object: RawWorldObject) -> bool {
        let index = usize::from(self.count);
        if index >= self.entries.len() {
            return false;
        }
        if let RawWorldObject::Item { x, y, z_index, .. } = &mut object {
            *z_index = u16::try_from(
                self.entries[..index]
                    .iter()
                    .filter(|entry| {
                        matches!(
                            entry,
                            Some(RawWorldObject::Item {
                                x: item_x,
                                y: item_y,
                                ..
                            }) if (*item_x, *item_y) == (*x, *y)
                        )
                    })
                    .count(),
            )
            .unwrap_or(u16::MAX);
        }
        self.entries[index] = Some(object);
        self.count += 1;
        true
    }

    pub fn name_player(&mut self, id: u32, name: &[u8]) {
        let Some(RawWorldObject::Player {
            name: player_name,
            name_len,
            ..
        }) = self.entries[..usize::from(self.count)]
            .iter_mut()
            .flatten()
            .find(|object| matches!(object, RawWorldObject::Player { id: player_id, .. } if *player_id == id))
        else {
            return;
        };
        let length = name.len().min(player_name.len());
        player_name.fill(0);
        player_name[..length].copy_from_slice(&name[..length]);
        *name_len = u8::try_from(length).expect("world object name buffer length fits u8");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_per_tile_item_stack_indexes_in_observation_order() {
        let mut objects = RawObjects::empty();
        for (id, x) in [(1, 10), (2, 11), (3, 10)] {
            assert!(objects.push(RawWorldObject::Item {
                id,
                sprite: 7,
                x,
                y: 20,
                z_index: u16::MAX,
            }));
        }

        assert!(matches!(
            objects.entries[0],
            Some(RawWorldObject::Item { z_index: 0, .. })
        ));
        assert!(matches!(
            objects.entries[1],
            Some(RawWorldObject::Item { z_index: 0, .. })
        ));
        assert!(matches!(
            objects.entries[2],
            Some(RawWorldObject::Item { z_index: 1, .. })
        ));
    }

    #[test]
    fn fills_the_local_player_name_from_character_state() {
        let mut objects = RawObjects::empty();
        assert!(objects.push(RawWorldObject::Player {
            id: 7,
            name: [0; MAX_OBJECT_NAME_BYTES],
            name_len: 0,
            x: 1,
            y: 2,
            direction: 0,
            is_hidden: false,
            visual: Some(RawPlayerVisual::Creature {
                sprite: 1,
                color: 0,
                boots_color: 0,
                pants_color: 0,
            }),
        }));

        objects.name_player(7, b"Monitor");

        let Some(RawWorldObject::Player { name, name_len, .. }) = objects.entries[0] else {
            panic!("expected player");
        };
        assert_eq!(&name[..usize::from(name_len)], b"Monitor");
    }
}
