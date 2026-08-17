#[cfg(all(windows, not(test)))]
use darpc_game_client::{RawObjects, RawWorldObject};
use darpc_model::TilePosition;
use darpc_protocol::MAX_RAW_PACKET_PAYLOAD_LEN;
#[cfg(all(windows, not(test)))]
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_BODY_LENGTH: usize = MAX_RAW_PACKET_PAYLOAD_LEN + 1;
const REPLAY_TTL_MS: u32 = 2_000;

#[cfg(all(windows, not(test)))]
static REPLAY_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
#[cfg(all(windows, not(test)))]
static PENDING: PendingCell = PendingCell(UnsafeCell::new(PendingPlayerDraw::new()));

#[cfg(all(windows, not(test)))]
struct PendingCell(UnsafeCell<PendingPlayerDraw>);

// SAFETY: capture and replay run on the client main thread. Installation resets
// this cell before the event detour becomes active.
#[cfg(all(windows, not(test)))]
unsafe impl Sync for PendingCell {}

#[derive(Clone, Copy)]
struct ReplayBody {
    bytes: [u8; MAX_BODY_LENGTH],
    len: u16,
}

impl ReplayBody {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

struct PendingPlayerDraw {
    bytes: [u8; MAX_BODY_LENGTH],
    len: u16,
    packet_position: TilePosition,
    captured_tick_ms: u32,
    valid: bool,
}

impl PendingPlayerDraw {
    const fn new() -> Self {
        Self {
            bytes: [0; MAX_BODY_LENGTH],
            len: 0,
            packet_position: TilePosition { x: 0, y: 0 },
            captured_tick_ms: 0,
            valid: false,
        }
    }

    fn reset(&mut self) {
        self.len = 0;
        self.valid = false;
    }

    fn capture(&mut self, body: &[u8], packet_position: TilePosition, tick_ms: u32) {
        let Ok(len) = u16::try_from(body.len()) else {
            return;
        };
        if body.len() < 5 || body.len() > self.bytes.len() {
            return;
        }
        self.bytes[..body.len()].copy_from_slice(body);
        self.len = len;
        self.packet_position = packet_position;
        self.captured_tick_ms = tick_ms;
        self.valid = true;
    }

    fn take_if_ready(
        &mut self,
        tick_ms: u32,
        position: Option<TilePosition>,
        walking: Option<bool>,
    ) -> Option<ReplayBody> {
        if !self.valid {
            return None;
        }
        if tick_ms.wrapping_sub(self.captured_tick_ms) > REPLAY_TTL_MS {
            self.reset();
            return None;
        }
        let position = position?;
        if walking != Some(false) || position == self.packet_position {
            return None;
        }
        let (Ok(x), Ok(y)) = (u16::try_from(position.x), u16::try_from(position.y)) else {
            return None;
        };
        let mut replay = ReplayBody {
            bytes: self.bytes,
            len: self.len,
        };
        replay.bytes[1..3].copy_from_slice(&x.to_be_bytes());
        replay.bytes[3..5].copy_from_slice(&y.to_be_bytes());
        self.reset();
        Some(replay)
    }
}

#[cfg(all(windows, not(test)))]
struct ReplayGuard;

#[cfg(all(windows, not(test)))]
impl ReplayGuard {
    fn enter() -> Self {
        REPLAY_IN_PROGRESS.store(true, Ordering::Release);
        Self
    }
}

#[cfg(all(windows, not(test)))]
impl Drop for ReplayGuard {
    fn drop(&mut self) {
        REPLAY_IN_PROGRESS.store(false, Ordering::Release);
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) fn reset() {
    REPLAY_IN_PROGRESS.store(false, Ordering::Release);
    // SAFETY: the caller invokes reset only while the event detour is inactive.
    unsafe { &mut *PENDING.0.get() }.reset();
}

#[cfg(all(windows, not(test)))]
pub(crate) fn is_replaying() -> bool {
    REPLAY_IN_PROGRESS.load(Ordering::Acquire)
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe(body: &[u8], objects: &RawObjects, tick_ms: u32) {
    if is_replaying() {
        return;
    }
    let Some(self_id) = crate::state::self_id() else {
        return;
    };
    let Some(RawWorldObject::Player {
        id,
        x,
        y,
        is_hidden,
        ..
    }) = objects.entries[0]
    else {
        return;
    };
    if id != self_id
        || !hidden_state_was_lost(crate::actions::movement::local_is_hidden(), is_hidden)
    {
        return;
    }
    // SAFETY: decoded event observation runs on the client main thread.
    unsafe { &mut *PENDING.0.get() }.capture(body, TilePosition { x, y }, tick_ms);
}

fn hidden_state_was_lost(native: Option<bool>, incoming: bool) -> bool {
    matches!(native, Some(current) if current != incoming)
}

#[cfg(all(windows, not(test)))]
pub(crate) fn observe_tick(tick_ms: u32) {
    // SAFETY: tick observation is the only reader and runs on the same client
    // main thread as capture. Keep the common empty path to one bounded read.
    if !unsafe { (&*PENDING.0.get()).valid } {
        return;
    }
    let position = crate::actions::movement::local_position();
    let walking = crate::actions::movement::is_walking();
    // SAFETY: tick observation runs on the same client main thread as capture.
    let replay = unsafe { &mut *PENDING.0.get() }.take_if_ready(tick_ms, position, walking);
    let Some(replay) = replay else {
        return;
    };
    let _guard = ReplayGuard::enter();
    let _ = crate::actions::network::dispatch(replay.as_slice());
}

#[cfg(test)]
mod tests {
    use super::{PendingPlayerDraw, REPLAY_TTL_MS, hidden_state_was_lost};
    use darpc_model::TilePosition;

    const PACKET_POSITION: TilePosition = TilePosition { x: 12, y: 34 };
    const COMMITTED_POSITION: TilePosition = TilePosition { x: 13, y: 34 };

    #[test]
    fn captures_only_a_post_handler_hidden_state_mismatch() {
        assert!(hidden_state_was_lost(Some(false), true));
        assert!(hidden_state_was_lost(Some(true), false));
        assert!(!hidden_state_was_lost(Some(true), true));
        assert!(!hidden_state_was_lost(Some(false), false));
        assert!(!hidden_state_was_lost(None, true));
    }

    #[test]
    fn waits_for_the_committed_step_and_rewrites_the_stale_coordinates() {
        let mut pending = PendingPlayerDraw::new();
        pending.capture(&[0x33, 0, 12, 0, 34, 3], PACKET_POSITION, 100);

        assert!(
            pending
                .take_if_ready(110, Some(COMMITTED_POSITION), Some(true))
                .is_none()
        );
        assert!(
            pending
                .take_if_ready(120, Some(PACKET_POSITION), Some(false))
                .is_none()
        );

        let replay = pending
            .take_if_ready(130, Some(COMMITTED_POSITION), Some(false))
            .expect("committed idle step should replay");
        assert_eq!(replay.as_slice(), &[0x33, 0, 13, 0, 34, 3]);
        assert!(
            pending
                .take_if_ready(140, Some(COMMITTED_POSITION), Some(false))
                .is_none()
        );
    }

    #[test]
    fn expires_a_packet_that_never_reaches_its_destination() {
        let mut pending = PendingPlayerDraw::new();
        pending.capture(&[0x33, 0, 12, 0, 34], PACKET_POSITION, u32::MAX - 50);

        assert!(
            pending
                .take_if_ready(REPLAY_TTL_MS - 40, Some(COMMITTED_POSITION), Some(false),)
                .is_none()
        );
        assert!(!pending.valid);
    }

    #[test]
    fn coalesces_to_the_latest_hidden_state_packet() {
        let mut pending = PendingPlayerDraw::new();
        pending.capture(&[0x33, 0, 12, 0, 34, 1], PACKET_POSITION, 100);
        pending.capture(&[0x33, 0, 12, 0, 34, 2], PACKET_POSITION, 110);

        let replay = pending
            .take_if_ready(120, Some(COMMITTED_POSITION), Some(false))
            .expect("latest packet should replay");
        assert_eq!(replay.as_slice(), &[0x33, 0, 13, 0, 34, 2]);
    }
}
