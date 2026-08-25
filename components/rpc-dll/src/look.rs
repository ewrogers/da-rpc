use darpc_model::LookTarget;
use darpc_protocol::{CommandFailure, MAX_LOOK_RESULT_TEXT_LEN};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const MESSAGE_OPCODE: u8 = 0x0a;
const MESSAGE_DIALOG_TYPES: core::ops::RangeInclusive<u8> = 8..=10;

pub(crate) static INTERCEPT_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static TARGET_IS_TILE: AtomicBool = AtomicBool::new(false);
static TARGET_COORDINATES: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingLook {
    command_id: u32,
    target: LookTarget,
}

#[cfg(all(windows, not(test)))]
pub(crate) fn request(command_id: u32, target: LookTarget) -> Result<(), CommandFailure> {
    begin(command_id, target)?;
    let (body, length) = encode_request(target);
    let result = crate::actions::network::submit(&body[..length]);
    if result.is_err() {
        cancel(command_id);
    }
    result
}

fn encode_request(target: LookTarget) -> ([u8; 5], usize) {
    let mut body = [0; 5];
    match target {
        LookTarget::Ahead => {
            body[0] = 0x09;
            (body, 1)
        }
        LookTarget::Tile { x, y } => {
            body[0] = 0x0a;
            body[1..3].copy_from_slice(&x.to_be_bytes());
            body[3..5].copy_from_slice(&y.to_be_bytes());
            (body, 5)
        }
    }
}

pub(crate) fn begin(command_id: u32, target: LookTarget) -> Result<(), CommandFailure> {
    if command_id == 0 || INTERCEPT_COMMAND_ID.load(Ordering::Acquire) != 0 {
        return Err(CommandFailure::Rejected);
    }
    match target {
        LookTarget::Ahead => {
            TARGET_IS_TILE.store(false, Ordering::Relaxed);
            TARGET_COORDINATES.store(0, Ordering::Relaxed);
        }
        LookTarget::Tile { x, y } => {
            TARGET_COORDINATES.store(u32::from(x) | (u32::from(y) << 16), Ordering::Relaxed);
            TARGET_IS_TILE.store(true, Ordering::Relaxed);
        }
    }
    INTERCEPT_COMMAND_ID.store(command_id, Ordering::Release);
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn intercept_response(body: &[u8], tick_ms: u32) -> bool {
    if INTERCEPT_COMMAND_ID.load(Ordering::Acquire) == 0 {
        return false;
    }
    let Some(text) = popup_text(body) else {
        return false;
    };
    let Some(pending) = take() else {
        return false;
    };
    let published = crate::state::observe_look(pending.command_id, pending.target, text, tick_ms);
    crate::commands::complete_look(
        pending.command_id,
        published.then_some(()).ok_or(CommandFailure::Internal),
    );
    true
}

fn popup_text(body: &[u8]) -> Option<&[u8]> {
    if body.len() < 4 || body[0] != MESSAGE_OPCODE || !MESSAGE_DIALOG_TYPES.contains(&body[1]) {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([body[2], body[3]]));
    if length == 0 || length > MAX_LOOK_RESULT_TEXT_LEN {
        return None;
    }
    body.get(4..4_usize.checked_add(length)?)
}

fn take() -> Option<PendingLook> {
    let command_id = INTERCEPT_COMMAND_ID.swap(0, Ordering::AcqRel);
    if command_id == 0 {
        return None;
    }
    let target = if TARGET_IS_TILE.load(Ordering::Relaxed) {
        let coordinates = TARGET_COORDINATES.load(Ordering::Relaxed);
        LookTarget::Tile {
            x: coordinates as u16,
            y: (coordinates >> 16) as u16,
        }
    } else {
        LookTarget::Ahead
    };
    Some(PendingLook { command_id, target })
}

pub(crate) fn cancel(command_id: u32) {
    if command_id == 0 {
        return;
    }
    let _ =
        INTERCEPT_COMMAND_ID.compare_exchange(command_id, 0, Ordering::AcqRel, Ordering::Relaxed);
}

pub(crate) fn reset() {
    INTERCEPT_COMMAND_ID.store(0, Ordering::Release);
    TARGET_IS_TILE.store(false, Ordering::Relaxed);
    TARGET_COORDINATES.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_bounded_popup_messages() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        assert_eq!(
            popup_text(b"\x0a\x09\x00\x08fior sal"),
            Some(&b"fior sal"[..])
        );
        assert_eq!(popup_text(b"\x0a\x08\x00\x04name"), Some(&b"name"[..]));
        assert_eq!(popup_text(b"\x0a\x0a\x00\x04sign"), Some(&b"sign"[..]));
        assert_eq!(popup_text(b"\x0a\x07\x00\x04name"), None);
        assert_eq!(popup_text(b"\x0a\x09\x00\x00"), None);
        assert_eq!(popup_text(b"\x0a\x09\x00\x05four"), None);
    }

    #[test]
    fn encodes_game_look_packets_in_network_byte_order() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        let (ahead, ahead_length) = encode_request(LookTarget::Ahead);
        assert_eq!(&ahead[..ahead_length], &[0x09]);

        let (tile, tile_length) = encode_request(LookTarget::Tile { x: 40, y: 19 });
        assert_eq!(&tile[..tile_length], &[0x0a, 0x00, 0x28, 0x00, 0x13]);
    }

    #[test]
    fn allows_only_one_uncorrelated_response_in_flight() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        reset();
        assert_eq!(begin(7, LookTarget::Tile { x: 40, y: 19 }), Ok(()));
        assert_eq!(begin(8, LookTarget::Ahead), Err(CommandFailure::Rejected));
        assert_eq!(
            take(),
            Some(PendingLook {
                command_id: 7,
                target: LookTarget::Tile { x: 40, y: 19 },
            })
        );
        assert_eq!(INTERCEPT_COMMAND_ID.load(Ordering::Acquire), 0);
    }
}
