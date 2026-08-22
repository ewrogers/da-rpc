use super::{QueuedStateUpdate, begin_object_reconciliation, push_event};
use darpc_model::{ActionUpdate, Direction, EquipmentSlot, TilePosition};
use std::cell::UnsafeCell;

const PENDING_RESYNC_CAPACITY: usize = crate::commands::COMMAND_CAPACITY;

static PENDING_RESYNCS: MainThreadPendingResyncs = MainThreadPendingResyncs::new();

pub(super) fn observe_outgoing(body: &[u8], tick_ms: u32) {
    let update = if is_resync_request(body) {
        let resync_id = crate::commands::outgoing_resync_id();
        // SAFETY: outgoing packet observation runs on the client main thread,
        // which is the sole owner of pending resync correlation state.
        unsafe { PENDING_RESYNCS.push(resync_id) };
        begin_object_reconciliation();
        ActionUpdate::Resync { resync_id }
    } else {
        let Some(update) = parse(body) else {
            return;
        };
        update
    };
    push_event(QueuedStateUpdate::Action(update), tick_ms);
}

pub(super) fn observe_resync_completed(tick_ms: u32) {
    // SAFETY: decoded server events run on the client main thread, which is
    // the sole owner of pending resync correlation state.
    let Some(resync_id) = (unsafe { PENDING_RESYNCS.pop() }) else {
        return;
    };
    push_event(
        QueuedStateUpdate::Action(ActionUpdate::ResyncCompleted { resync_id }),
        tick_ms,
    );
}

pub(super) fn reset() {
    // SAFETY: state reset runs while the producer hook is absent.
    unsafe { PENDING_RESYNCS.reset() };
}

fn is_resync_request(body: &[u8]) -> bool {
    body == [0x38]
}

fn parse(body: &[u8]) -> Option<ActionUpdate> {
    match *body.first()? {
        0x07 if body.len() == 6 => Some(ActionUpdate::ItemPickedUp {
            destination_slot: body[1],
            position: position(body, 2, 4)?,
        }),
        0x08 if body.len() == 10 => Some(ActionUpdate::ItemDropped {
            slot: body[1],
            quantity: u32::from_be_bytes(body[6..10].try_into().ok()?),
            position: position(body, 2, 4)?,
        }),
        0x11 if body.len() == 2 => {
            Direction::from_raw(body[1]).map(|direction| ActionUpdate::Turned { direction })
        }
        0x1C if body.len() == 2 => Some(ActionUpdate::ItemUsed { slot: body[1] }),
        0x1D if body.len() == 2 => Some(ActionUpdate::Emoted { code: body[1] }),
        0x24 if body.len() == 9 => Some(ActionUpdate::GoldDropped {
            amount: u32::from_be_bytes(body[1..5].try_into().ok()?),
            position: position(body, 5, 7)?,
        }),
        0x29 if body.len() == 10 => Some(ActionUpdate::ItemGiven {
            slot: body[1],
            object_id: u32::from_be_bytes(body[2..6].try_into().ok()?),
            quantity: u32::from_be_bytes(body[6..10].try_into().ok()?),
        }),
        0x2A if body.len() == 9 => Some(ActionUpdate::GoldGiven {
            amount: u32::from_be_bytes(body[1..5].try_into().ok()?),
            object_id: u32::from_be_bytes(body[5..9].try_into().ok()?),
        }),
        0x44 if body.len() == 2 => {
            EquipmentSlot::from_raw(body[1]).map(|slot| ActionUpdate::EquipmentUnequipped { slot })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingResyncs {
    ids: [u32; PENDING_RESYNC_CAPACITY],
    head: usize,
    len: usize,
}

impl PendingResyncs {
    const fn new() -> Self {
        Self {
            ids: [0; PENDING_RESYNC_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, resync_id: u32) {
        if self.len == self.ids.len() {
            return;
        }
        let tail = (self.head + self.len) % self.ids.len();
        self.ids[tail] = resync_id;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let resync_id = self.ids[self.head];
        self.ids[self.head] = 0;
        self.head = (self.head + 1) % self.ids.len();
        self.len -= 1;
        Some(resync_id)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

struct MainThreadPendingResyncs(UnsafeCell<PendingResyncs>);

// SAFETY: access is restricted to the client main thread except during reset,
// which runs only while the producer hook is absent.
unsafe impl Sync for MainThreadPendingResyncs {}

impl MainThreadPendingResyncs {
    const fn new() -> Self {
        Self(UnsafeCell::new(PendingResyncs::new()))
    }

    unsafe fn push(&self, resync_id: u32) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).push(resync_id) };
    }

    unsafe fn pop(&self) -> Option<u32> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).pop() }
    }

    unsafe fn reset(&self) {
        // SAFETY: the caller guarantees exclusive lifecycle access.
        unsafe { (&mut *self.0.get()).reset() };
    }
}

fn position(body: &[u8], x: usize, y: usize) -> Option<TilePosition> {
    Some(TilePosition {
        x: i32::from(u16::from_be_bytes(body[x..x + 2].try_into().ok()?)),
        y: i32::from(u16::from_be_bytes(body[y..y + 2].try_into().ok()?)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_and_gold_drops() {
        assert_eq!(
            parse(&[0x08, 3, 0, 2, 0, 8, 0, 0, 0, 4]),
            Some(ActionUpdate::ItemDropped {
                slot: 3,
                quantity: 4,
                position: TilePosition { x: 2, y: 8 },
            })
        );
        assert_eq!(
            parse(&[0x24, 0, 0, 0, 100, 0, 2, 0, 8]),
            Some(ActionUpdate::GoldDropped {
                amount: 100,
                position: TilePosition { x: 2, y: 8 },
            })
        );
    }

    #[test]
    fn parses_client_resync() {
        assert!(is_resync_request(&[0x38]));
        assert!(!is_resync_request(&[0x38, 0]));
    }

    #[test]
    fn correlates_refresh_responses_in_request_order() {
        let mut pending = PendingResyncs::new();
        pending.push(7);
        pending.push(9);

        assert_eq!(pending.pop(), Some(7));
        assert_eq!(pending.pop(), Some(9));
        assert_eq!(pending.pop(), None);
    }
}
