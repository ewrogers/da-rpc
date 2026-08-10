use std::sync::atomic::{AtomicBool, AtomicU32, Ordering, fence};

use crate::client_text;

const MAX_NAME_BYTES: usize = u8::MAX as usize;
const NAME_WORDS: usize = MAX_NAME_BYTES.div_ceil(4);

static VERSION: AtomicU32 = AtomicU32::new(0);
static READY: AtomicBool = AtomicBool::new(false);
static WORLD_TOKEN: AtomicU32 = AtomicU32::new(0);
static MAP_ID: AtomicU32 = AtomicU32::new(0);
static NAME_LENGTH: AtomicU32 = AtomicU32::new(0);
static NAME: [AtomicU32; NAME_WORDS] = [const { AtomicU32::new(0) }; NAME_WORDS];

pub(crate) fn reset() {
    READY.store(false, Ordering::Release);
}

pub(crate) fn publish(world_token: u32, map_id: u32, name: &[u8]) {
    let name = &name[..name.len().min(MAX_NAME_BYTES)];
    VERSION.fetch_add(1, Ordering::AcqRel);
    WORLD_TOKEN.store(world_token, Ordering::Relaxed);
    MAP_ID.store(map_id, Ordering::Relaxed);
    NAME_LENGTH.store(name.len() as u32, Ordering::Relaxed);
    for (index, slot) in NAME.iter().enumerate() {
        let start = index * 4;
        let mut bytes = [0_u8; 4];
        if start < name.len() {
            let end = (start + 4).min(name.len());
            bytes[..end - start].copy_from_slice(&name[start..end]);
        }
        slot.store(u32::from_le_bytes(bytes), Ordering::Relaxed);
    }
    VERSION.fetch_add(1, Ordering::Release);
    READY.store(true, Ordering::Release);
}

pub(crate) fn read(expected_world_token: u32, expected_map_id: u32) -> Option<String> {
    if !READY.load(Ordering::Acquire) {
        return None;
    }
    for _ in 0..8 {
        let before = VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            continue;
        }
        let world_token = WORLD_TOKEN.load(Ordering::Relaxed);
        let map_id = MAP_ID.load(Ordering::Relaxed);
        let length = NAME_LENGTH.load(Ordering::Relaxed) as usize;
        if length > MAX_NAME_BYTES {
            return None;
        }
        let mut bytes = [0_u8; MAX_NAME_BYTES];
        for (index, slot) in NAME.iter().enumerate() {
            let start = index * 4;
            let end = (start + 4).min(bytes.len());
            bytes[start..end]
                .copy_from_slice(&slot.load(Ordering::Relaxed).to_le_bytes()[..end - start]);
        }
        fence(Ordering::Acquire);
        if VERSION.load(Ordering::Acquire) == before {
            return (world_token == expected_world_token && map_id == expected_map_id)
                .then(|| client_text::decode(&bytes[..length]))
                .flatten();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{publish, read, reset};

    #[test]
    fn publishes_only_for_the_matching_world_and_map() {
        reset();
        publish(0x1234, 600, b"Mileth");

        assert_eq!(read(0x1234, 600).as_deref(), Some("Mileth"));
        assert_eq!(read(0x1235, 600), None);
        assert_eq!(read(0x1234, 601), None);
    }
}
