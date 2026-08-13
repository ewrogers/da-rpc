#![cfg_attr(not(windows), allow(dead_code))]

use darpc_model::{MapExclusions, MapExclusionsUpdate, TilePosition};
use darpc_protocol::{
    MAX_PATH_EXCLUSION_DIMENSION, MAX_PATH_EXCLUSION_MAPS, MAX_PATH_EXCLUSION_TOTAL_TILES,
    PathExclusions, RouteTile,
};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

const BITS_PER_WORD: usize = u32::BITS as usize;
const WORD_COUNT: usize =
    MAX_PATH_EXCLUSION_DIMENSION * MAX_PATH_EXCLUSION_DIMENSION / BITS_PER_WORD;
const NO_ACTIVE_SNAPSHOT: u8 = 2;

struct Registry {
    maps: BTreeMap<u32, Box<[RouteTile]>>,
    total_tiles: usize,
}

impl Registry {
    const fn new() -> Self {
        Self {
            maps: BTreeMap::new(),
            total_tiles: 0,
        }
    }
}

struct ActiveSnapshot {
    map_id: AtomicU32,
    blocked: [AtomicU32; WORD_COUNT],
}

impl ActiveSnapshot {
    const fn new() -> Self {
        Self {
            map_id: AtomicU32::new(0),
            blocked: [const { AtomicU32::new(0) }; WORD_COUNT],
        }
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
static ACTIVE: [ActiveSnapshot; 2] = [const { ActiveSnapshot::new() }; 2];
static ACTIVE_INDEX: AtomicU8 = AtomicU8::new(NO_ACTIVE_SNAPSHOT);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Replacement events stay inline and pointer-free so the main-thread event
// path performs no allocation. The enclosing byte-budgeted queue accounts for
// this fixed size.
#[allow(clippy::large_enum_variant)]
pub(crate) enum QueuedPathExclusionsUpdate {
    Replaced {
        exclusions: PathExclusions,
        map_count: u16,
    },
    Removed {
        map_id: u32,
        map_count: u16,
    },
    Cleared {
        removed_map_count: u16,
    },
}

impl QueuedPathExclusionsUpdate {
    pub(crate) fn into_model(self) -> MapExclusionsUpdate {
        match self {
            Self::Replaced {
                exclusions,
                map_count,
            } => MapExclusionsUpdate::Replaced {
                exclusions: model_map(exclusions.map_id(), exclusions.tiles()),
                map_count,
            },
            Self::Removed { map_id, map_count } => {
                MapExclusionsUpdate::Removed { map_id, map_count }
            }
            Self::Cleared { removed_map_count } => {
                MapExclusionsUpdate::Cleared { removed_map_count }
            }
        }
    }
}

pub(crate) fn replace(
    exclusions: PathExclusions,
    current_map_id: Option<u32>,
) -> Result<Option<QueuedPathExclusionsUpdate>, ()> {
    if exclusions.map_id() > u32::from(u16::MAX) {
        return Err(());
    }
    let mut tiles = exclusions.tiles().to_vec();
    if tiles.is_empty()
        || tiles.iter().any(|tile| {
            usize::from(tile.x) >= MAX_PATH_EXCLUSION_DIMENSION
                || usize::from(tile.y) >= MAX_PATH_EXCLUSION_DIMENSION
        })
    {
        return Err(());
    }
    tiles.sort_unstable_by_key(|tile| (tile.y, tile.x));
    tiles.dedup();
    let exclusions = PathExclusions::new(exclusions.map_id(), &tiles).ok_or(())?;

    let map_count = {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous_count = registry
            .maps
            .get(&exclusions.map_id())
            .map_or(0, |previous| previous.len());
        if registry
            .maps
            .get(&exclusions.map_id())
            .is_some_and(|previous| previous.as_ref() == tiles.as_slice())
        {
            return Ok(None);
        }
        if previous_count == 0 && registry.maps.len() >= MAX_PATH_EXCLUSION_MAPS {
            return Err(());
        }
        let total_tiles = registry
            .total_tiles
            .checked_sub(previous_count)
            .and_then(|total| total.checked_add(tiles.len()))
            .ok_or(())?;
        if total_tiles > MAX_PATH_EXCLUSION_TOTAL_TILES {
            return Err(());
        }
        registry.total_tiles = total_tiles;
        registry
            .maps
            .insert(exclusions.map_id(), tiles.into_boxed_slice());
        u16::try_from(registry.maps.len()).expect("bounded exclusion map count fits u16")
    };
    if current_map_id == Some(exclusions.map_id()) {
        activate(exclusions.map_id());
    }
    Ok(Some(QueuedPathExclusionsUpdate::Replaced {
        exclusions,
        map_count,
    }))
}

pub(crate) fn remove(
    map_id: u32,
    current_map_id: Option<u32>,
) -> Option<QueuedPathExclusionsUpdate> {
    let map_count = {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = registry.maps.remove(&map_id)?;
        registry.total_tiles -= removed.len();
        u16::try_from(registry.maps.len()).expect("bounded exclusion map count fits u16")
    };
    if current_map_id == Some(map_id) {
        activate(map_id);
    }
    Some(QueuedPathExclusionsUpdate::Removed { map_id, map_count })
}

pub(crate) fn clear() -> Option<QueuedPathExclusionsUpdate> {
    let removed_map_count = {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if registry.maps.is_empty() {
            return None;
        }
        let removed =
            u16::try_from(registry.maps.len()).expect("bounded exclusion map count fits u16");
        registry.maps.clear();
        registry.total_tiles = 0;
        removed
    };
    ACTIVE_INDEX.store(NO_ACTIVE_SNAPSHOT, Ordering::Release);
    Some(QueuedPathExclusionsUpdate::Cleared { removed_map_count })
}

pub(crate) fn activate(map_id: u32) {
    let registry = REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(tiles) = registry.maps.get(&map_id) else {
        ACTIVE_INDEX.store(NO_ACTIVE_SNAPSHOT, Ordering::Release);
        return;
    };
    let current = ACTIVE_INDEX.load(Ordering::Acquire);
    let next = usize::from(current == 0);
    let snapshot = &ACTIVE[next];
    for word in &snapshot.blocked {
        word.store(0, Ordering::Relaxed);
    }
    for tile in tiles {
        let index = usize::from(tile.y) * MAX_PATH_EXCLUSION_DIMENSION + usize::from(tile.x);
        snapshot.blocked[index / BITS_PER_WORD]
            .fetch_or(1 << (index % BITS_PER_WORD), Ordering::Relaxed);
    }
    snapshot.map_id.store(map_id, Ordering::Relaxed);
    ACTIVE_INDEX.store(
        u8::try_from(next).expect("active buffer index fits u8"),
        Ordering::Release,
    );
}

pub(crate) fn blocked(map_id: u32, x: i32, y: i32) -> bool {
    let index = ACTIVE_INDEX.load(Ordering::Acquire);
    let Some(snapshot) = ACTIVE.get(usize::from(index)) else {
        return false;
    };
    if snapshot.map_id.load(Ordering::Relaxed) != map_id || x < 0 || y < 0 {
        return false;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= MAX_PATH_EXCLUSION_DIMENSION || y >= MAX_PATH_EXCLUSION_DIMENSION {
        return false;
    }
    let index = y * MAX_PATH_EXCLUSION_DIMENSION + x;
    snapshot.blocked[index / BITS_PER_WORD].load(Ordering::Relaxed) & (1 << (index % BITS_PER_WORD))
        != 0
}

pub(crate) fn model_state() -> Vec<MapExclusions> {
    REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .maps
        .iter()
        .map(|(&map_id, tiles)| model_map(map_id, tiles))
        .collect()
}

pub(crate) fn reset() {
    let mut registry = REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    registry.maps.clear();
    registry.total_tiles = 0;
    ACTIVE_INDEX.store(NO_ACTIVE_SNAPSHOT, Ordering::Release);
}

fn model_map(map_id: u32, tiles: &[RouteTile]) -> MapExclusions {
    MapExclusions {
        map_id,
        tiles: tiles
            .iter()
            .map(|tile| TilePosition {
                x: i32::from(tile.x),
                y: i32::from(tile.y),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn exclusions_persist_across_map_activation() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        let first =
            PathExclusions::new(3000, &[RouteTile { x: 9, y: 2 }, RouteTile { x: 4, y: 5 }])
                .unwrap();
        let second = PathExclusions::new(3001, &[RouteTile { x: 7, y: 8 }]).unwrap();

        assert!(replace(first, Some(3000)).unwrap().is_some());
        assert!(replace(first, Some(3000)).unwrap().is_none());
        assert!(replace(second, Some(3000)).is_ok());
        assert!(blocked(3000, 4, 5));
        assert!(!blocked(3000, 7, 8));

        activate(3001);
        assert!(blocked(3001, 7, 8));
        assert!(!blocked(3000, 4, 5));

        activate(3000);
        assert!(blocked(3000, 9, 2));
        assert_eq!(
            model_state()
                .iter()
                .map(|entry| entry.map_id)
                .collect::<Vec<_>>(),
            vec![3000, 3001]
        );
    }

    #[test]
    fn removing_and_clearing_maps_deactivates_exclusions() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        let first = PathExclusions::new(3000, &[RouteTile { x: 4, y: 5 }]).unwrap();
        let second = PathExclusions::new(3001, &[RouteTile { x: 7, y: 8 }]).unwrap();
        replace(first, Some(3000)).unwrap();
        replace(second, Some(3000)).unwrap();

        assert!(remove(3000, Some(3000)).is_some());
        assert!(!blocked(3000, 4, 5));
        assert!(clear().is_some());
        activate(3001);
        assert!(!blocked(3001, 7, 8));
        assert!(model_state().is_empty());
    }
}
