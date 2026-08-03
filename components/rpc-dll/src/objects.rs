use darpc_game_client::{MAX_OBJECT_NAME_BYTES, MAX_WORLD_OBJECTS, RawObjects, RawWorldObject};
use darpc_model::{CreatureKind, Direction, ObjectUpdate, WorldObject};

const VIEW_DISTANCE: u32 = 18;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedObjectUpdate {
    Appeared(RawWorldObject),
    Disappeared(RawWorldObject),
    Moved(RawWorldObject),
    DirectionChanged(RawWorldObject),
    Cleared,
}

impl QueuedObjectUpdate {
    pub(crate) fn into_model(self) -> ObjectUpdate {
        match self {
            Self::Appeared(object) => ObjectUpdate::Appeared(object_model(object)),
            Self::Disappeared(object) => ObjectUpdate::Disappeared(object_model(object)),
            Self::Moved(object) => ObjectUpdate::Moved(object_model(object)),
            Self::DirectionChanged(object) => ObjectUpdate::DirectionChanged(object_model(object)),
            Self::Cleared => ObjectUpdate::Cleared,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObjectCache {
    entries: [Option<RawWorldObject>; MAX_WORLD_OBJECTS],
}

impl Default for ObjectCache {
    fn default() -> Self {
        Self::empty()
    }
}

impl ObjectCache {
    pub(crate) const fn empty() -> Self {
        Self {
            entries: [None; MAX_WORLD_OBJECTS],
        }
    }

    pub(crate) fn replace(&mut self, objects: &RawObjects) {
        for (destination, source) in self.entries.iter_mut().zip(objects.entries.iter()) {
            *destination = *source;
        }
    }

    pub(crate) fn name(&self, id: u32) -> Option<([u8; MAX_OBJECT_NAME_BYTES], u8)> {
        match self.entries[self.find(id)?]? {
            RawWorldObject::Player { name, name_len, .. }
            | RawWorldObject::Creature { name, name_len, .. }
                if name_len != 0 =>
            {
                Some((name, name_len))
            }
            _ => None,
        }
    }

    #[cfg(not(test))]
    pub(crate) fn position(&self, id: u32) -> Option<(i32, i32)> {
        self.entries[self.find(id)?].map(object_position)
    }

    pub(crate) fn upsert(&mut self, mut object: RawWorldObject) -> Option<QueuedObjectUpdate> {
        if let Some(index) = self.find(object_id(object)) {
            let current = self.entries[index].expect("located object entry is populated");
            if same_observation(current, object) {
                return None;
            }
            self.remove_at(index);
        }
        if let RawWorldObject::Item { x, y, z_index, .. } = &mut object {
            *z_index = self.item_count_at(*x, *y);
        }
        let slot = self.entries.iter_mut().find(|entry| entry.is_none())?;
        *slot = Some(object);
        Some(QueuedObjectUpdate::Appeared(object))
    }

    pub(crate) fn move_object(
        &mut self,
        id: u32,
        x: i32,
        y: i32,
        direction: Option<u8>,
    ) -> Option<QueuedObjectUpdate> {
        let index = self.find(id)?;
        let mut object = self.entries[index]?;
        match &mut object {
            RawWorldObject::Player {
                x: current_x,
                y: current_y,
                direction: current_direction,
                ..
            }
            | RawWorldObject::Creature {
                x: current_x,
                y: current_y,
                direction: current_direction,
                ..
            } => {
                if (*current_x, *current_y) == (x, y)
                    && direction.is_none_or(|direction| *current_direction == direction)
                {
                    return None;
                }
                *current_x = x;
                *current_y = y;
                if let Some(direction) = direction {
                    *current_direction = direction;
                }
            }
            RawWorldObject::Item { .. } => return None,
        }
        self.entries[index] = Some(object);
        Some(QueuedObjectUpdate::Moved(object))
    }

    pub(crate) fn move_self(
        &mut self,
        id: Option<u32>,
        x: i32,
        y: i32,
    ) -> Option<QueuedObjectUpdate> {
        let index = self.find(id?)?;
        let mut object = self.entries[index]?;
        match &mut object {
            RawWorldObject::Player {
                x: current_x,
                y: current_y,
                ..
            } => {
                if (*current_x, *current_y) == (x, y) {
                    return None;
                }
                *current_x = x;
                *current_y = y;
            }
            _ => return None,
        }
        self.entries[index] = Some(object);
        Some(QueuedObjectUpdate::Moved(object))
    }

    pub(crate) fn change_direction(
        &mut self,
        id: u32,
        direction: u8,
    ) -> Option<QueuedObjectUpdate> {
        let index = self.find(id)?;
        let mut object = self.entries[index]?;
        match &mut object {
            RawWorldObject::Player {
                direction: current, ..
            }
            | RawWorldObject::Creature {
                direction: current, ..
            } => {
                if *current == direction {
                    return None;
                }
                *current = direction;
            }
            RawWorldObject::Item { .. } => return None,
        }
        self.entries[index] = Some(object);
        Some(QueuedObjectUpdate::DirectionChanged(object))
    }

    pub(crate) fn remove(&mut self, id: u32) -> Option<QueuedObjectUpdate> {
        let index = self.find(id)?;
        let object = self.entries[index]?;
        self.remove_at(index);
        Some(QueuedObjectUpdate::Disappeared(object))
    }

    pub(crate) fn take_outside(
        &mut self,
        center_x: i32,
        center_y: i32,
    ) -> Option<QueuedObjectUpdate> {
        let index = self.entries.iter().position(|entry| {
            entry.is_some_and(|object| {
                let (x, y) = object_position(object);
                x.abs_diff(center_x).saturating_add(y.abs_diff(center_y)) > VIEW_DISTANCE
            })
        })?;
        let object = self.entries[index]?;
        self.remove_at(index);
        Some(QueuedObjectUpdate::Disappeared(object))
    }

    pub(crate) fn clear(&mut self) -> Option<QueuedObjectUpdate> {
        if self.entries.iter().all(Option::is_none) {
            return None;
        }
        self.entries.fill(None);
        Some(QueuedObjectUpdate::Cleared)
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.is_some_and(|object| object_id(object) == id))
    }

    fn remove_at(&mut self, index: usize) {
        let Some(RawWorldObject::Item { x, y, z_index, .. }) = self.entries[index] else {
            self.entries[index] = None;
            return;
        };
        self.entries[index] = None;
        for entry in self.entries.iter_mut().flatten() {
            if let RawWorldObject::Item {
                x: item_x,
                y: item_y,
                z_index: item_z,
                ..
            } = entry
                && (*item_x, *item_y) == (x, y)
                && *item_z > z_index
            {
                *item_z -= 1;
            }
        }
    }

    fn item_count_at(&self, x: i32, y: i32) -> u16 {
        u16::try_from(
            self.entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry,
                        Some(RawWorldObject::Item {
                            x: item_x,
                            y: item_y,
                            ..
                        }) if (*item_x, *item_y) == (x, y)
                    )
                })
                .count(),
        )
        .unwrap_or(u16::MAX)
    }
}

fn object_model(raw: RawWorldObject) -> WorldObject {
    match raw {
        RawWorldObject::Player {
            id,
            name,
            name_len,
            x,
            y,
            direction,
        } => WorldObject::Player {
            id,
            name: decode_name(&name[..usize::from(name_len)]),
            x,
            y,
            direction: Direction::from_raw(direction).expect("observed player direction is valid"),
        },
        RawWorldObject::Creature {
            id,
            is_npc,
            sprite,
            name,
            name_len,
            x,
            y,
            direction,
        } => WorldObject::Creature {
            id,
            kind: if is_npc {
                CreatureKind::Npc
            } else {
                CreatureKind::Monster
            },
            sprite,
            name: decode_name(&name[..usize::from(name_len)]),
            x,
            y,
            direction: Direction::from_raw(direction)
                .expect("observed creature direction is valid"),
        },
        RawWorldObject::Item {
            id,
            sprite,
            x,
            y,
            z_index,
        } => WorldObject::Item {
            id,
            sprite,
            x,
            y,
            z_index,
        },
    }
}

#[cfg(windows)]
fn decode_name(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode_name(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

const fn object_id(object: RawWorldObject) -> u32 {
    match object {
        RawWorldObject::Player { id, .. }
        | RawWorldObject::Creature { id, .. }
        | RawWorldObject::Item { id, .. } => id,
    }
}

const fn object_position(object: RawWorldObject) -> (i32, i32) {
    match object {
        RawWorldObject::Player { x, y, .. }
        | RawWorldObject::Creature { x, y, .. }
        | RawWorldObject::Item { x, y, .. } => (x, y),
    }
}

fn same_observation(current: RawWorldObject, observed: RawWorldObject) -> bool {
    match (current, observed) {
        (
            RawWorldObject::Item {
                id: left_id,
                sprite: left_sprite,
                x: left_x,
                y: left_y,
                ..
            },
            RawWorldObject::Item {
                id: right_id,
                sprite: right_sprite,
                x: right_x,
                y: right_y,
                ..
            },
        ) => (left_id, left_sprite, left_x, left_y) == (right_id, right_sprite, right_x, right_y),
        _ => current == observed,
    }
}

const _: () = assert!(MAX_OBJECT_NAME_BYTES <= u8::MAX as usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_and_compacts_item_stack_indexes() {
        let mut cache = ObjectCache::empty();
        for id in 1..=3 {
            cache.upsert(item(id, 10, 20));
        }
        let QueuedObjectUpdate::Disappeared(RawWorldObject::Item { z_index, .. }) =
            cache.remove(2).unwrap()
        else {
            panic!("expected item removal");
        };
        assert_eq!(z_index, 1);
        let QueuedObjectUpdate::Appeared(RawWorldObject::Item { z_index, .. }) =
            cache.upsert(item(4, 10, 20)).unwrap()
        else {
            panic!("expected item appearance");
        };
        assert_eq!(z_index, 2);
    }

    #[test]
    fn suppresses_duplicate_draws_and_replaces_changed_observations() {
        let mut cache = ObjectCache::empty();
        let original = player(1, 10, 20, 0);
        assert!(matches!(
            cache.upsert(original),
            Some(QueuedObjectUpdate::Appeared(_))
        ));
        assert_eq!(cache.upsert(original), None);

        let changed = player(1, 11, 20, 0);
        assert_eq!(
            cache.upsert(changed),
            Some(QueuedObjectUpdate::Appeared(changed))
        );
    }

    #[test]
    fn tracks_living_movement_direction_and_self_position() {
        let mut cache = ObjectCache::empty();
        cache.upsert(player(1, 10, 20, 0));
        cache.upsert(creature(2, 12, 20, 1));

        assert_eq!(
            cache.move_object(2, 13, 20, Some(2)),
            Some(QueuedObjectUpdate::Moved(creature(2, 13, 20, 2)))
        );
        assert_eq!(
            cache.move_object(2, 12, 20, None),
            Some(QueuedObjectUpdate::Moved(creature(2, 12, 20, 2)))
        );
        assert_eq!(
            cache.change_direction(2, 3),
            Some(QueuedObjectUpdate::DirectionChanged(creature(2, 12, 20, 3)))
        );
        assert_eq!(
            cache.move_self(Some(1), 11, 20),
            Some(QueuedObjectUpdate::Moved(player(1, 11, 20, 0)))
        );
        assert_eq!(cache.move_self(Some(1), 11, 20), None);
    }

    #[test]
    fn culls_one_out_of_range_object_at_a_time_and_clears_atomically() {
        let mut cache = ObjectCache::empty();
        cache.upsert(player(1, 10, 10, 0));
        cache.upsert(creature(2, 40, 40, 0));

        assert!(matches!(
            cache.take_outside(10, 10),
            Some(QueuedObjectUpdate::Disappeared(RawWorldObject::Creature {
                id: 2,
                ..
            }))
        ));
        assert_eq!(cache.take_outside(10, 10), None);
        assert_eq!(cache.clear(), Some(QueuedObjectUpdate::Cleared));
        assert_eq!(cache.clear(), None);
    }

    fn item(id: u32, x: i32, y: i32) -> RawWorldObject {
        RawWorldObject::Item {
            id,
            sprite: 7,
            x,
            y,
            z_index: 0,
        }
    }

    fn player(id: u32, x: i32, y: i32, direction: u8) -> RawWorldObject {
        RawWorldObject::Player {
            id,
            name: [0; MAX_OBJECT_NAME_BYTES],
            name_len: 0,
            x,
            y,
            direction,
        }
    }

    fn creature(id: u32, x: i32, y: i32, direction: u8) -> RawWorldObject {
        RawWorldObject::Creature {
            id,
            is_npc: false,
            sprite: Some(5),
            name: [0; MAX_OBJECT_NAME_BYTES],
            name_len: 0,
            x,
            y,
            direction,
        }
    }
}
