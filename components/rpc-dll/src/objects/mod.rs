use darpc_game_client::{
    MAX_OBJECT_NAME_BYTES, MAX_WORLD_OBJECTS, RawHumanVisual, RawObjects, RawPlayerVisual,
    RawWorldObject,
};
use darpc_model::{CreatureKind, Direction, HumanVisual, ObjectUpdate, PlayerVisual, WorldObject};

const VIEW_DISTANCE: u32 = 18;
const REFRESH_RECONCILIATION_QUIET_MS: u32 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedObjectUpdate {
    Appeared(RawWorldObject),
    Disappeared(RawWorldObject),
    Moved(RawWorldObject),
    DirectionChanged(RawWorldObject),
}

impl QueuedObjectUpdate {
    pub(crate) fn into_model(self) -> ObjectUpdate {
        match self {
            Self::Appeared(object) => ObjectUpdate::Appeared(object_model(object)),
            Self::Disappeared(object) => ObjectUpdate::Disappeared(object_model(object)),
            Self::Moved(object) => ObjectUpdate::Moved(object_model(object)),
            Self::DirectionChanged(object) => ObjectUpdate::DirectionChanged(object_model(object)),
        }
    }

    pub(crate) const fn object_id(self) -> u32 {
        let (Self::Appeared(object)
        | Self::Disappeared(object)
        | Self::Moved(object)
        | Self::DirectionChanged(object)) = self;
        object_id(object)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObjectCache {
    entries: [Option<RawWorldObject>; MAX_WORLD_OBJECTS],
    reconciliation: ObjectReconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectReconciliation {
    phase: ReconciliationPhase,
    pending: [bool; MAX_WORLD_OBJECTS],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReconciliationPhase {
    Idle,
    AwaitingResponse,
    QuietUntil(u32),
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
            reconciliation: ObjectReconciliation::idle(),
        }
    }

    pub(crate) fn replace(&mut self, objects: &RawObjects) {
        self.reconciliation.reset();
        for (destination, source) in self.entries.iter_mut().zip(objects.entries.iter()) {
            *destination = *source;
        }
    }

    pub(crate) fn begin_reconciliation(&mut self) {
        self.reconciliation.phase = ReconciliationPhase::AwaitingResponse;
        for (pending, entry) in self
            .reconciliation
            .pending
            .iter_mut()
            .zip(self.entries.iter())
        {
            *pending = entry.is_some();
        }
    }

    pub(crate) fn observe_reconciliation_activity(&mut self, tick_ms: u32) {
        if !matches!(self.reconciliation.phase, ReconciliationPhase::Idle) {
            self.reconciliation.phase = ReconciliationPhase::QuietUntil(
                tick_ms.wrapping_add(REFRESH_RECONCILIATION_QUIET_MS),
            );
        }
    }

    pub(crate) fn draw(&mut self, object: RawWorldObject) -> Option<QueuedObjectUpdate> {
        if let Some(index) = self.find(object_id(object)) {
            self.reconciliation.pending[index] = false;
        }
        self.upsert(object)
    }

    pub(crate) fn finish_reconciliation(
        &mut self,
        tick_ms: u32,
        mut emit: impl FnMut(QueuedObjectUpdate),
    ) {
        let ReconciliationPhase::QuietUntil(deadline) = self.reconciliation.phase else {
            return;
        };
        if !crate::wrapping_time::deadline_reached(tick_ms, deadline) {
            return;
        }

        self.reconciliation.phase = ReconciliationPhase::Idle;
        for index in 0..self.entries.len() {
            if !self.reconciliation.pending[index] {
                continue;
            }
            let Some(object) = self.entries[index] else {
                continue;
            };
            self.remove_at(index);
            emit(QueuedObjectUpdate::Disappeared(object));
        }
        self.reconciliation.pending.fill(false);
    }

    pub(crate) fn reset(&mut self) {
        self.entries.fill(None);
        self.reconciliation.reset();
    }

    pub(crate) fn merge_snapshot_sprites(&self, objects: &mut RawObjects) {
        for object in objects
            .entries
            .iter_mut()
            .take(usize::from(objects.count))
            .flatten()
        {
            let RawWorldObject::Creature {
                id, is_npc, sprite, ..
            } = object
            else {
                continue;
            };
            if sprite.is_some() {
                continue;
            }
            if let Some(RawWorldObject::Creature {
                is_npc: current_is_npc,
                sprite: Some(current_sprite),
                ..
            }) = self.get(*id)
                && *is_npc == current_is_npc
            {
                *sprite = Some(current_sprite);
            }
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

    pub(crate) fn get(&self, id: u32) -> Option<RawWorldObject> {
        self.entries[self.find(id)?]
    }

    pub(crate) fn player_with_name(&self, observed: RawWorldObject) -> Option<u32> {
        let RawWorldObject::Player {
            id, name, name_len, ..
        } = observed
        else {
            return None;
        };
        let name_len = usize::from(name_len);
        if name_len == 0 {
            return None;
        }
        self.entries.iter().flatten().find_map(|current| {
            let RawWorldObject::Player {
                id: current_id,
                name: current_name,
                name_len: current_name_len,
                ..
            } = current
            else {
                return None;
            };
            (*current_id != id
                && usize::from(*current_name_len) == name_len
                && current_name[..name_len] == name[..name_len])
                .then_some(*current_id)
        })
    }

    pub(crate) fn remove_player_with_name(&mut self, observed: RawWorldObject) -> Option<u32> {
        let id = self.player_with_name(observed)?;
        let index = self.find(id)?;
        self.remove_at(index);
        Some(id)
    }

    #[cfg(not(test))]
    pub(crate) fn position(&self, id: u32) -> Option<(i32, i32)> {
        self.entries[self.find(id)?].map(object_position)
    }

    pub(crate) fn upsert(&mut self, mut object: RawWorldObject) -> Option<QueuedObjectUpdate> {
        if let RawWorldObject::Player {
            id,
            is_hidden: true,
            name,
            name_len,
            ..
        } = &mut object
            && *name_len == 0
            && let Some(RawWorldObject::Player {
                name: current_name,
                name_len: current_name_len,
                ..
            }) = self.get(*id)
        {
            *name = current_name;
            *name_len = current_name_len;
        }
        while self.remove_player_with_name(object).is_some() {}
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

    pub(crate) fn clear(&mut self, mut emit: impl FnMut(QueuedObjectUpdate)) {
        self.reconciliation.reset();
        for entry in &mut self.entries {
            if let Some(object) = entry.take() {
                emit(QueuedObjectUpdate::Disappeared(object));
            }
        }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.is_some_and(|object| object_id(object) == id))
    }

    fn remove_at(&mut self, index: usize) {
        self.reconciliation.pending[index] = false;
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

impl ObjectReconciliation {
    const fn idle() -> Self {
        Self {
            phase: ReconciliationPhase::Idle,
            pending: [false; MAX_WORLD_OBJECTS],
        }
    }

    fn reset(&mut self) {
        *self = Self::idle();
    }
}

pub(crate) fn object_model(raw: RawWorldObject) -> WorldObject {
    match raw {
        RawWorldObject::Player {
            id,
            name,
            name_len,
            x,
            y,
            direction,
            is_hidden,
            visual,
        } => WorldObject::Player {
            id,
            name: decode_name(&name[..usize::from(name_len)]),
            x,
            y,
            direction: Direction::from_raw(direction).expect("observed player direction is valid"),
            is_hidden,
            visual: visual.map(visual_model),
            profile: None,
        },
        RawWorldObject::Creature {
            id,
            is_npc,
            is_solid,
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
            is_solid,
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
            dye_color,
            x,
            y,
            z_index,
        } => WorldObject::Item {
            id,
            sprite,
            dye_color,
            x,
            y,
            z_index,
        },
    }
}

pub(crate) const fn visual_model(raw: RawPlayerVisual) -> PlayerVisual {
    match raw {
        RawPlayerVisual::Human(RawHumanVisual {
            gender,
            head_sprite,
            body_sprite,
            arms_sprite,
            boots_sprite,
            pants_sprite,
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
            is_translucent,
        }) => PlayerVisual::Human(HumanVisual {
            gender: darpc_model::Gender::from_raw(gender),
            head_sprite,
            body_sprite,
            arms_sprite,
            boots_sprite,
            pants_sprite,
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
            is_translucent,
        }),
        RawPlayerVisual::Creature {
            sprite,
            color,
            boots_color,
            pants_color,
        } => PlayerVisual::Creature {
            sprite,
            color,
            boots_color,
            pants_color,
        },
    }
}

fn decode_name(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
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
    fn replaces_a_player_with_the_same_name_and_a_new_id() {
        let mut cache = ObjectCache::empty();
        let original = named_player(1, b"Silo", 10, 20, 0);
        let duplicate = named_player(3, b"Silo", 12, 22, 2);
        let replacement = named_player(2, b"Silo", 11, 21, 1);
        let mut snapshot = RawObjects::empty();
        assert!(snapshot.push(original));
        assert!(snapshot.push(duplicate));
        cache.replace(&snapshot);

        assert_eq!(cache.player_with_name(replacement), Some(1));
        assert_eq!(
            cache.upsert(replacement),
            Some(QueuedObjectUpdate::Appeared(replacement))
        );
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(3), None);
        assert_eq!(cache.get(2), Some(replacement));
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
    fn culls_one_out_of_range_object_at_a_time_and_clears_with_disappearances() {
        let mut cache = ObjectCache::empty();
        let retained = player(1, 10, 10, 0);
        cache.upsert(retained);
        cache.upsert(creature(2, 40, 40, 0));

        assert!(matches!(
            cache.take_outside(10, 10),
            Some(QueuedObjectUpdate::Disappeared(RawWorldObject::Creature {
                id: 2,
                ..
            }))
        ));
        assert_eq!(cache.take_outside(10, 10), None);
        let mut updates = Vec::new();
        cache.clear(|update| updates.push(update));
        assert_eq!(updates, [QueuedObjectUpdate::Disappeared(retained)]);
        cache.clear(|update| updates.push(update));
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn refresh_reconciliation_emits_only_natural_lifecycle_changes() {
        let mut cache = ObjectCache::empty();
        let retained = player(1, 10, 10, 0);
        let stale = creature(2, 12, 10, 1);
        let appeared = item(3, 11, 10);
        cache.upsert(retained);
        cache.upsert(stale);

        cache.begin_reconciliation();
        cache.observe_reconciliation_activity(100);
        assert_eq!(cache.draw(retained), None);
        assert_eq!(
            cache.draw(appeared),
            Some(QueuedObjectUpdate::Appeared(appeared))
        );

        let mut updates = Vec::new();
        cache.finish_reconciliation(1_099, |update| updates.push(update));
        assert!(updates.is_empty());
        cache.finish_reconciliation(1_100, |update| updates.push(update));
        assert_eq!(updates, [QueuedObjectUpdate::Disappeared(stale)]);
        assert_eq!(cache.get(1), Some(retained));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(appeared));
    }

    #[test]
    fn refresh_without_a_response_retains_last_known_objects() {
        let mut cache = ObjectCache::empty();
        let retained = player(1, 10, 10, 0);
        cache.upsert(retained);
        cache.begin_reconciliation();

        let mut updates = Vec::new();
        cache.finish_reconciliation(u32::MAX, |update| updates.push(update));
        assert!(updates.is_empty());
        assert_eq!(cache.get(1), Some(retained));
    }

    #[test]
    fn refresh_response_without_draws_removes_stale_objects() {
        let mut cache = ObjectCache::empty();
        let stale = creature(2, 12, 10, 1);
        cache.upsert(stale);
        cache.begin_reconciliation();
        cache.observe_reconciliation_activity(100);

        let mut updates = Vec::new();
        cache.finish_reconciliation(1_100, |update| updates.push(update));
        assert_eq!(updates, [QueuedObjectUpdate::Disappeared(stale)]);
    }

    #[test]
    fn refresh_activity_extends_the_quiet_deadline() {
        let mut cache = ObjectCache::empty();
        let stale = creature(2, 12, 10, 1);
        cache.upsert(stale);
        cache.begin_reconciliation();
        cache.observe_reconciliation_activity(100);
        cache.observe_reconciliation_activity(900);

        let mut updates = Vec::new();
        cache.finish_reconciliation(1_899, |update| updates.push(update));
        assert!(updates.is_empty());
        cache.finish_reconciliation(1_900, |update| updates.push(update));
        assert_eq!(updates, [QueuedObjectUpdate::Disappeared(stale)]);
    }

    #[test]
    fn refresh_quiet_deadline_wraps_with_the_client_tick() {
        let mut cache = ObjectCache::empty();
        let stale = creature(2, 12, 10, 1);
        cache.upsert(stale);
        cache.begin_reconciliation();
        cache.observe_reconciliation_activity(u32::MAX - 500);

        let mut updates = Vec::new();
        cache.finish_reconciliation(498, |update| updates.push(update));
        assert!(updates.is_empty());
        cache.finish_reconciliation(499, |update| updates.push(update));
        assert_eq!(updates, [QueuedObjectUpdate::Disappeared(stale)]);
    }

    #[test]
    fn snapshot_retains_a_packet_observed_creature_sprite() {
        let mut cache = ObjectCache::empty();
        cache.upsert(creature(2, 12, 20, 1));
        let mut captured = creature(2, 13, 20, 2);
        let RawWorldObject::Creature { sprite, .. } = &mut captured else {
            unreachable!();
        };
        *sprite = None;
        let mut snapshot = RawObjects::empty();
        assert!(snapshot.push(captured));

        cache.merge_snapshot_sprites(&mut snapshot);

        let Some(RawWorldObject::Creature {
            sprite,
            x,
            y,
            direction,
            ..
        }) = snapshot.entries[0]
        else {
            panic!("expected creature");
        };
        assert_eq!(sprite, Some(5));
        assert_eq!((x, y, direction), (13, 20, 2));
    }

    #[test]
    fn hidden_player_observation_retains_the_last_known_name() {
        let mut cache = ObjectCache::empty();
        cache.upsert(named_player(7, b"Monitor", 10, 20, 1));
        let mut hidden = player(7, 11, 20, 1);
        let RawWorldObject::Player { is_hidden, .. } = &mut hidden else {
            unreachable!();
        };
        *is_hidden = true;

        let Some(QueuedObjectUpdate::Appeared(updated)) = cache.upsert(hidden) else {
            panic!("expected hidden player update");
        };
        let RawWorldObject::Player {
            name,
            name_len,
            is_hidden,
            ..
        } = updated
        else {
            panic!("expected player");
        };
        assert!(is_hidden);
        assert_eq!(&name[..usize::from(name_len)], b"Monitor");
    }

    fn item(id: u32, x: i32, y: i32) -> RawWorldObject {
        RawWorldObject::Item {
            id,
            sprite: 7,
            dye_color: 2,
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
            is_hidden: false,
            visual: None,
        }
    }

    fn named_player(id: u32, value: &[u8], x: i32, y: i32, direction: u8) -> RawWorldObject {
        let mut name = [0; MAX_OBJECT_NAME_BYTES];
        name[..value.len()].copy_from_slice(value);
        RawWorldObject::Player {
            id,
            name,
            name_len: u8::try_from(value.len()).unwrap(),
            x,
            y,
            direction,
            is_hidden: false,
            visual: None,
        }
    }

    fn creature(id: u32, x: i32, y: i32, direction: u8) -> RawWorldObject {
        RawWorldObject::Creature {
            id,
            is_npc: false,
            is_solid: true,
            sprite: Some(5),
            name: [0; MAX_OBJECT_NAME_BYTES],
            name_len: 0,
            x,
            y,
            direction,
        }
    }
}
