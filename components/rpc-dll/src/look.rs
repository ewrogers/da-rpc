use darpc_model::{Direction, LookResultTarget, LookTarget, TilePosition};
use darpc_protocol::{CommandFailure, MAX_LOOK_RESULT_TEXT_LEN};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

const MESSAGE_OPCODE: u8 = 0x0a;
const MESSAGE_DIALOG_TYPES: core::ops::RangeInclusive<u8> = 8..=10;

// Phase and owner move atomically: IPC cancellation races the main-thread
// response hook. No locks, allocation, or retry loops are needed in either hook.
const IDLE: u64 = 0;
const ARMED: u64 = 1 << 32;
const PENDING: u64 = 2 << 32;
const MANUAL: u64 = 3 << 32;
pub(crate) const QUARANTINED: u64 = 4 << 32;
const RESOLVING: u64 = 5 << 32;
const PHASE_MASK: u64 = u64::MAX << 32;
// The x86 event detour reads the high word only as an advisory fast-path gate.
// intercept_response always acquires and validates the complete atomic state.
pub(crate) static CHANNEL: AtomicU64 = AtomicU64::new(IDLE);
static TARGET_IS_TILE: AtomicBool = AtomicBool::new(false);
static TARGET_COORDINATES: AtomicU32 = AtomicU32::new(0);

#[cfg(all(windows, not(test)))]
pub(crate) fn request(command_id: u32, target: LookTarget) -> Result<(), CommandFailure> {
    let result_target = resolve_target(target, crate::state::confirmed_pose())?;
    begin(command_id, result_target)?;
    let (body, length) = encode_request(target);
    let result = crate::actions::network::submit(&body[..length]);
    if result.is_err() {
        // submit returns an error only before calling the native sender.
        let _ = CHANNEL.compare_exchange(
            ARMED | u64::from(command_id),
            IDLE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    result
}

fn resolve_target(
    target: LookTarget,
    pose: Option<(TilePosition, Direction)>,
) -> Result<LookResultTarget, CommandFailure> {
    match target {
        LookTarget::Tile { x, y } => Ok(LookResultTarget::Tile { x, y }),
        LookTarget::Ahead => {
            let (position, direction) = pose.ok_or(CommandFailure::InvalidState)?;
            let position =
                step_position(position, direction).ok_or(CommandFailure::InvalidDestination)?;
            let x = u16::try_from(position.x).map_err(|_| CommandFailure::InvalidDestination)?;
            let y = u16::try_from(position.y).map_err(|_| CommandFailure::InvalidDestination)?;
            Ok(LookResultTarget::Ahead { x, y })
        }
    }
}

fn step_position(position: TilePosition, direction: Direction) -> Option<TilePosition> {
    let (x, y) = match direction {
        Direction::North => (position.x, position.y.checked_sub(1)?),
        Direction::East => (position.x.checked_add(1)?, position.y),
        Direction::South => (position.x, position.y.checked_add(1)?),
        Direction::West => (position.x.checked_sub(1)?, position.y),
    };
    Some(TilePosition { x, y })
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

pub(crate) fn begin(command_id: u32, target: LookResultTarget) -> Result<(), CommandFailure> {
    if command_id == 0 {
        return Err(CommandFailure::Rejected);
    }
    CHANNEL
        .compare_exchange(
            IDLE,
            ARMED | u64::from(command_id),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| CommandFailure::Rejected)?;
    let (tile, x, y) = match target {
        LookResultTarget::Ahead { x, y } => (false, x, y),
        LookResultTarget::Tile { x, y } => (true, x, y),
    };
    TARGET_COORDINATES.store(u32::from(x) | (u32::from(y) << 16), Ordering::Relaxed);
    TARGET_IS_TILE.store(tile, Ordering::Release);
    Ok(())
}

/// Called before the native sender. A matching typed request arms exactly one
/// response; manual/raw requests retain their normal game behavior.
pub(crate) fn observe_outgoing(body: &[u8], source: darpc_model::ActionSource) {
    if !matches!(body.first(), Some(0x09 | 0x0a)) {
        return;
    }
    let current = CHANNEL.load(Ordering::Acquire);
    let expected = match target() {
        LookResultTarget::Ahead { .. } => LookTarget::Ahead,
        LookResultTarget::Tile { x, y } => LookTarget::Tile { x, y },
    };
    let (packet, length) = encode_request(expected);
    let owner = command_id(current);
    if current & PHASE_MASK == ARMED
        && owner != 0
        && source == (darpc_model::ActionSource::Command { command_id: owner })
        && body == &packet[..length]
    {
        // Cancellation may already have quarantined the lane. Never overwrite it.
        let _ = CHANNEL.compare_exchange(
            current,
            PENDING | u64::from(owner),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        return;
    }
    let valid = matches!(body, [0x09] | [0x0a, _, _, _, _]);
    if current == IDLE
        && valid
        && CHANNEL
            .compare_exchange(IDLE, MANUAL, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        return;
    }
    quarantine();
}

/// Failed observation or synthetic response injection cannot establish ownership.
pub(crate) fn quarantine() {
    let previous = CHANNEL.swap(QUARANTINED, Ordering::AcqRel);
    if matches!(previous & PHASE_MASK, ARMED | PENDING) {
        crate::commands::complete_look(command_id(previous), Err(CommandFailure::InvalidState));
    }
}

pub(crate) fn active_command_id() -> u32 {
    let current = CHANNEL.load(Ordering::Acquire);
    if matches!(current & PHASE_MASK, ARMED | PENDING) {
        command_id(current)
    } else {
        0
    }
}

fn command_id(channel: u64) -> u32 {
    (channel & u64::from(u32::MAX)) as u32
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn intercept_response(body: &[u8], tick_ms: u32) -> bool {
    if body.len() < 2 || body[0] != MESSAGE_OPCODE || !MESSAGE_DIALOG_TYPES.contains(&body[1]) {
        return false;
    }
    let current = CHANNEL.load(Ordering::Acquire);
    if matches!(current, IDLE | QUARANTINED) {
        return false;
    }
    let Some(text) = popup_text(body) else {
        quarantine();
        return false;
    };
    if current == MANUAL {
        let _ = CHANNEL.compare_exchange(MANUAL, IDLE, Ordering::AcqRel, Ordering::Acquire);
        return false;
    }
    if current & PHASE_MASK != PENDING {
        quarantine();
        return false;
    }
    let resolving = RESOLVING | u64::from(command_id(current));
    if CHANNEL
        .compare_exchange(current, resolving, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let published = crate::state::observe_look(command_id(current), target(), text, tick_ms);
    crate::commands::complete_look(
        command_id(current),
        published.then_some(()).ok_or(CommandFailure::Internal),
    );
    // Do not erase quarantine from a concurrent reset or observation failure.
    let next = if published { IDLE } else { QUARANTINED };
    let _ = CHANNEL.compare_exchange(resolving, next, Ordering::AcqRel, Ordering::Acquire);
    published
}

fn popup_text(body: &[u8]) -> Option<&[u8]> {
    if body.len() < 4 || body[0] != MESSAGE_OPCODE || !MESSAGE_DIALOG_TYPES.contains(&body[1]) {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([body[2], body[3]]));
    if length > MAX_LOOK_RESULT_TEXT_LEN || body.len() != 4 + length {
        return None;
    }
    body.get(4..)
}

fn target() -> LookResultTarget {
    let coordinates = TARGET_COORDINATES.load(Ordering::Acquire);
    let x = (coordinates & u32::from(u16::MAX)) as u16;
    let y = (coordinates >> 16) as u16;
    if TARGET_IS_TILE.load(Ordering::Acquire) {
        LookResultTarget::Tile { x, y }
    } else {
        LookResultTarget::Ahead { x, y }
    }
}

pub(crate) fn cancel(command_id: u32) {
    if command_id == 0 {
        return;
    }
    for phase in [ARMED, PENDING] {
        let _ = CHANNEL.compare_exchange(
            phase | u64::from(command_id),
            QUARANTINED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Hook/IPC reinitialization is not a game-connection boundary. Keep uncertainty
/// for this DLL lifetime; recovery needs a fresh client process, not reinjection.
pub(crate) fn reset() {
    let current = CHANNEL.load(Ordering::Acquire);
    if current != IDLE {
        CHANNEL.store(QUARANTINED, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    CHANNEL.store(IDLE, Ordering::Release);
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
        assert_eq!(popup_text(b"\x0a\x09\x00\x00"), Some(&b""[..]));
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
    fn resolves_the_ahead_tile_from_the_confirmed_pose() {
        let position = TilePosition { x: 40, y: 19 };
        for (direction, x, y) in [
            (Direction::North, 40, 18),
            (Direction::East, 41, 19),
            (Direction::South, 40, 20),
            (Direction::West, 39, 19),
        ] {
            assert_eq!(
                resolve_target(LookTarget::Ahead, Some((position, direction))),
                Ok(LookResultTarget::Ahead { x, y })
            );
        }
        assert_eq!(
            resolve_target(LookTarget::Ahead, None),
            Err(CommandFailure::InvalidState)
        );
        assert_eq!(
            resolve_target(
                LookTarget::Ahead,
                Some((TilePosition { x: 0, y: 0 }, Direction::North)),
            ),
            Err(CommandFailure::InvalidDestination)
        );
    }

    #[test]
    fn one_owner_is_armed_until_its_exact_outgoing_packet_is_seen() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        reset_for_test();
        assert_eq!(begin(7, LookResultTarget::Tile { x: 40, y: 19 }), Ok(()));
        assert_eq!(
            begin(8, LookResultTarget::Ahead { x: 40, y: 18 }),
            Err(CommandFailure::Rejected)
        );
        observe_outgoing(
            &[0x0a, 0, 40, 0, 19],
            darpc_model::ActionSource::Command { command_id: 7 },
        );
        assert_eq!(CHANNEL.load(Ordering::Acquire), PENDING | 7);
        assert_eq!(active_command_id(), 7);
    }
    fn arm(id: u32) {
        begin(id, LookResultTarget::Tile { x: 40, y: 19 }).unwrap();
        observe_outgoing(
            &[0x0a, 0, 40, 0, 19],
            darpc_model::ActionSource::Command { command_id: id },
        );
    }

    fn clear() {
        reset_for_test();
        crate::state::reset();
    }

    #[test]
    fn normal_and_empty_responses_keep_their_original_owner() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        for text in [b"Grimlok Prole5\tGold (25)".as_slice(), b""] {
            clear();
            arm(7);
            let mut popup = vec![0x0a, 9];
            popup.extend_from_slice(&(text.len() as u16).to_be_bytes());
            popup.extend_from_slice(text);
            assert!(intercept_response(&popup, 50));
            let darpc_protocol::EventPollResult::Events(events) =
                crate::state::poll(0, 8, std::time::Duration::ZERO)
            else {
                panic!("look event");
            };
            assert_eq!(events.len(), 1);
            let darpc_model::StateUpdate::Look(result) = &events[0].update else {
                panic!("look update");
            };
            assert_eq!(result.command_id, 7);
            assert_eq!(result.target, LookResultTarget::Tile { x: 40, y: 19 });
            assert_eq!(result.text.as_bytes(), text);
            assert_eq!(CHANNEL.load(Ordering::Acquire), IDLE);
            assert!(begin(8, LookResultTarget::Tile { x: 41, y: 19 }).is_ok());
        }
    }

    #[test]
    fn cancelled_late_responses_never_become_a_new_look() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        clear();
        arm(7);
        cancel(7);
        for _ in 0..3 {
            assert_eq!(
                begin(8, LookResultTarget::Tile { x: 41, y: 19 }),
                Err(CommandFailure::Rejected)
            );
            assert!(!intercept_response(b"\x0a\x09\x00\x04name", 90));
            reset();
        }
        assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
        assert!(matches!(
            crate::state::poll(0, 8, std::time::Duration::ZERO),
            darpc_protocol::EventPollResult::Events(events) if events.is_empty()
        ));
    }

    #[test]
    fn queued_cancellation_cannot_poison_another_owner() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        clear();
        cancel(3);
        arm(7);
        cancel(8);
        assert_eq!(active_command_id(), 7);
        assert!(intercept_response(b"\x0a\x09\x00\x04name", 90));
    }

    #[test]
    fn cancellation_before_native_submission_and_early_popups_fail_closed() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        for cancel_first in [true, false] {
            clear();
            begin(7, LookResultTarget::Tile { x: 40, y: 19 }).unwrap();
            if cancel_first {
                cancel(7);
            }
            assert!(!intercept_response(b"\x0a\x09\x00\x04name", 90));
            observe_outgoing(
                &[0x0a, 0, 40, 0, 19],
                darpc_model::ActionSource::Command { command_id: 7 },
            );
            assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
            assert!(begin(8, LookResultTarget::Tile { x: 41, y: 19 }).is_err());
        }
    }

    #[test]
    fn cancellation_racing_a_response_never_changes_its_owner() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        for _ in 0..100 {
            clear();
            arm(7);
            let published = std::thread::scope(|scope| {
                let cancel = scope.spawn(|| cancel(7));
                let published = intercept_response(b"\x0a\x09\x00\x04name", 90);
                cancel.join().unwrap();
                published
            });
            let darpc_protocol::EventPollResult::Events(events) =
                crate::state::poll(0, 8, std::time::Duration::ZERO)
            else {
                panic!("look events");
            };
            assert_eq!(events.len(), usize::from(published));
            for event in events {
                let darpc_model::StateUpdate::Look(result) = event.update else {
                    panic!("look update");
                };
                assert_eq!(result.command_id, 7);
            }
            // Response acquisition and cancellation have one atomic winner.
            assert_eq!(
                begin(8, LookResultTarget::Tile { x: 41, y: 19 }).is_ok(),
                published
            );
        }
    }

    #[test]
    fn manual_looks_are_visible_and_defer_typed_requests() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        clear();
        observe_outgoing(&[0x09], darpc_model::ActionSource::Client);
        assert_eq!(
            begin(7, LookResultTarget::Tile { x: 40, y: 19 }),
            Err(CommandFailure::Rejected)
        );
        assert!(!intercept_response(b"\x0a\x09\x00\x04name", 90));
        assert_eq!(CHANNEL.load(Ordering::Acquire), IDLE);
        arm(7);
    }

    #[test]
    fn overlapping_manual_or_raw_looks_quarantine_both_response_orders() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        for source in [
            darpc_model::ActionSource::Client,
            darpc_model::ActionSource::Command { command_id: 8 },
        ] {
            for reply in [b"AAAA", b"BBBB"] {
                clear();
                arm(7);
                observe_outgoing(&[0x09], source);
                let mut popup = vec![0x0a, 9, 0, 4];
                popup.extend_from_slice(reply);
                assert!(!intercept_response(&popup, 90));
                assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
                assert_eq!(
                    begin(9, LookResultTarget::Tile { x: 41, y: 19 }),
                    Err(CommandFailure::Rejected)
                );
            }
        }
    }

    #[test]
    fn wrong_tile_or_duplicate_submission_cannot_claim_ownership() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        clear();
        begin(7, LookResultTarget::Tile { x: 40, y: 19 }).unwrap();
        observe_outgoing(
            &[0x0a, 0, 41, 0, 19],
            darpc_model::ActionSource::Command { command_id: 7 },
        );
        assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
        clear();
        arm(7);
        observe_outgoing(
            &[0x0a, 0, 40, 0, 19],
            darpc_model::ActionSource::Command { command_id: 7 },
        );
        assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
    }

    #[test]
    fn malformed_popup_or_publication_loss_keeps_the_lane_quarantined() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        for body in [
            b"\x0a\x09".as_slice(),
            b"\x0a\x09\x00\x05four",
            b"\x0a\x09\x00\x04name-extra",
        ] {
            clear();
            arm(7);
            assert!(!intercept_response(body, 90));
            assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
        }
        clear();
        arm(7);
        let mut full = false;
        for _ in 0..4096 {
            if !crate::state::observe_look(99, LookResultTarget::Ahead { x: 0, y: 0 }, b"x", 1) {
                full = true;
                break;
            }
        }
        assert!(full);
        assert!(!intercept_response(b"\x0a\x09\x00\x04name", 90));
        assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
    }

    #[test]
    fn unrelated_messages_pass_without_consuming_ownership() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        clear();
        arm(7);
        assert!(!intercept_response(b"\x0a\x07\x00\x04name", 90));
        assert_eq!(active_command_id(), 7);
        // Synthetic popup injection and failed packet observation use this same fence.
        quarantine();
        assert!(!intercept_response(b"\x0a\x09\x00\x04name", 91));
        assert_eq!(CHANNEL.load(Ordering::Acquire), QUARANTINED);
    }
}
