#![cfg_attr(not(windows), allow(dead_code))]

use darpc_game_client::{MAX_WORLD_OBJECTS, RawObjects, RawWorldObject};
use darpc_protocol::MAX_PATH_EXCLUSION_DIMENSION;
use std::cell::UnsafeCell;

const BITS_PER_WORD: usize = u32::BITS as usize;
const WORD_COUNT: usize =
    MAX_PATH_EXCLUSION_DIMENSION * MAX_PATH_EXCLUSION_DIMENSION / BITS_PER_WORD;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlayerTile {
    id: u32,
    x: i32,
    y: i32,
}

struct Occupancy {
    players: [Option<PlayerTile>; MAX_WORLD_OBJECTS],
    occupied: [u32; WORD_COUNT],
}

impl Occupancy {
    const fn empty() -> Self {
        Self {
            players: [None; MAX_WORLD_OBJECTS],
            occupied: [0; WORD_COUNT],
        }
    }

    fn replace(&mut self, objects: &RawObjects, self_id: Option<u32>) {
        self.clear();
        for object in objects.entries.iter().flatten().copied() {
            self.upsert(object, self_id);
        }
    }

    fn upsert(&mut self, object: RawWorldObject, self_id: Option<u32>) {
        let RawWorldObject::Player { id, x, y, .. } = object else {
            return;
        };
        if self_id == Some(id) {
            self.remove(id);
            return;
        }
        if let Some(index) = self.find(id) {
            let previous = self.players[index].expect("located player entry is populated");
            self.players[index] = Some(PlayerTile { id, x, y });
            self.refresh(previous.x, previous.y);
            self.set(x, y, true);
            return;
        }
        let Some(slot) = self.players.iter_mut().find(|entry| entry.is_none()) else {
            return;
        };
        *slot = Some(PlayerTile { id, x, y });
        self.set(x, y, true);
    }

    fn move_player(&mut self, id: u32, x: i32, y: i32) {
        let Some(index) = self.find(id) else {
            return;
        };
        let previous = self.players[index].expect("located player entry is populated");
        self.players[index] = Some(PlayerTile { id, x, y });
        self.refresh(previous.x, previous.y);
        self.set(x, y, true);
    }

    fn remove(&mut self, id: u32) {
        let Some(index) = self.find(id) else {
            return;
        };
        let previous = self.players[index]
            .take()
            .expect("located player entry is populated");
        self.refresh(previous.x, previous.y);
    }

    fn clear(&mut self) {
        self.players.fill(None);
        self.occupied.fill(0);
    }

    fn blocked(&self, x: i32, y: i32) -> bool {
        let Some(index) = tile_index(x, y) else {
            return false;
        };
        self.occupied[index / BITS_PER_WORD] & (1 << (index % BITS_PER_WORD)) != 0
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.players
            .iter()
            .position(|entry| entry.is_some_and(|player| player.id == id))
    }

    fn refresh(&mut self, x: i32, y: i32) {
        let occupied = self
            .players
            .iter()
            .flatten()
            .any(|player| (player.x, player.y) == (x, y));
        self.set(x, y, occupied);
    }

    fn set(&mut self, x: i32, y: i32, occupied: bool) {
        let Some(index) = tile_index(x, y) else {
            return;
        };
        let bit = 1 << (index % BITS_PER_WORD);
        let word = &mut self.occupied[index / BITS_PER_WORD];
        if occupied {
            *word |= bit;
        } else {
            *word &= !bit;
        }
    }
}

struct MainThreadOccupancy(UnsafeCell<Occupancy>);

// SAFETY: snapshot capture, object observation, map transitions, and native
// path construction are serialized on the client main thread. Reset runs only
// while those hooks are absent.
unsafe impl Sync for MainThreadOccupancy {}

static OCCUPANCY: MainThreadOccupancy = MainThreadOccupancy(UnsafeCell::new(Occupancy::empty()));

pub(crate) fn replace(objects: &RawObjects, self_id: Option<u32>) {
    // SAFETY: callers run on the client main thread.
    unsafe { (&mut *OCCUPANCY.0.get()).replace(objects, self_id) };
}

pub(crate) fn upsert(object: RawWorldObject, self_id: Option<u32>) {
    // SAFETY: callers run on the client main thread.
    unsafe { (&mut *OCCUPANCY.0.get()).upsert(object, self_id) };
}

pub(crate) fn move_player(id: u32, x: i32, y: i32) {
    // SAFETY: callers run on the client main thread.
    unsafe { (&mut *OCCUPANCY.0.get()).move_player(id, x, y) };
}

pub(crate) fn remove(id: u32) {
    // SAFETY: callers run on the client main thread.
    unsafe { (&mut *OCCUPANCY.0.get()).remove(id) };
}

pub(crate) fn clear() {
    // SAFETY: callers have exclusive main-thread or lifecycle access.
    unsafe { (&mut *OCCUPANCY.0.get()).clear() };
}

pub(crate) fn blocked(x: i32, y: i32) -> bool {
    // SAFETY: native path construction runs on the client main thread.
    unsafe { (&*OCCUPANCY.0.get()).blocked(x, y) }
}

fn tile_index(x: i32, y: i32) -> Option<usize> {
    let (x, y) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
    (x < MAX_PATH_EXCLUSION_DIMENSION && y < MAX_PATH_EXCLUSION_DIMENSION)
        .then_some(y * MAX_PATH_EXCLUSION_DIMENSION + x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_game_client::MAX_OBJECT_NAME_BYTES;

    #[test]
    fn tracks_snapshot_draw_move_remove_and_self_exclusion() {
        let mut occupancy = Occupancy::empty();
        let mut objects = RawObjects::empty();
        assert!(objects.push(player(1, 3, 30)));
        assert!(objects.push(player(2, 3, 31)));

        occupancy.replace(&objects, Some(2));
        assert!(occupancy.blocked(3, 30));
        assert!(!occupancy.blocked(3, 31));

        occupancy.move_player(1, 4, 30);
        assert!(!occupancy.blocked(3, 30));
        assert!(occupancy.blocked(4, 30));

        occupancy.upsert(player(3, 4, 30), Some(2));
        occupancy.remove(1);
        assert!(occupancy.blocked(4, 30));
        occupancy.remove(3);
        assert!(!occupancy.blocked(4, 30));
    }

    fn player(id: u32, x: i32, y: i32) -> RawWorldObject {
        RawWorldObject::Player {
            id,
            name: [0; MAX_OBJECT_NAME_BYTES],
            name_len: 0,
            x,
            y,
            direction: 0,
            is_hidden: false,
        }
    }
}
