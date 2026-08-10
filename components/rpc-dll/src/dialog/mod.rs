#![cfg_attr(test, allow(dead_code))]

mod decode;

use darpc_model::{DialogCloseReason, DialogState, DialogSubmission, DialogUpdate};
use decode::{decode, decode_text};
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

pub(crate) const MAX_DIALOG_PACKET_BYTES: usize = 8 * 1024;
const EVENT_SLOTS: usize = 32;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

static CURRENT: CurrentDialog = CurrentDialog(UnsafeCell::new(RawDialog::empty()));
static EVENTS: DialogEvents = DialogEvents::new();
static REVISION: AtomicU32 = AtomicU32::new(0);

struct CurrentDialog(UnsafeCell<RawDialog>);

// SAFETY: CURRENT is read and written only by the client main thread. A copy
// is transferred to the IPC thread through snapshot publication or EVENTS.
unsafe impl Sync for CurrentDialog {}

#[derive(Clone, Copy)]
pub(crate) struct RawDialog {
    pub(super) revision: u32,
    pub(super) response_pending: bool,
    pub(super) length: u16,
    pub(super) bytes: [u8; MAX_DIALOG_PACKET_BYTES],
}

impl RawDialog {
    pub(crate) const fn empty() -> Self {
        Self {
            revision: 0,
            response_pending: false,
            length: 0,
            bytes: [0; MAX_DIALOG_PACKET_BYTES],
        }
    }

    fn active(self) -> bool {
        self.length != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedDialog(u8);

#[derive(Clone, Copy)]
// Fixed storage keeps dialog observation allocation-free in the packet hook.
#[allow(clippy::large_enum_variant)]
enum EventKind {
    Opened,
    Changed,
    Submitted {
        previous_revision: u32,
        submission: RawSubmission,
    },
    Closed(DialogCloseReason),
}

#[derive(Clone, Copy)]
struct RawDialogEvent {
    kind: EventKind,
    dialog: RawDialog,
}

#[derive(Clone, Copy)]
// Input text is stored inline so the main-thread submission path does not allocate.
#[allow(clippy::large_enum_variant)]
enum RawSubmission {
    Select { index: u16, quantity: u8 },
    Input(DialogText),
    Previous,
    Next,
    Close,
}

#[derive(Clone, Copy)]
struct DialogText {
    length: u8,
    bytes: [u8; u8::MAX as usize],
}

impl DialogText {
    fn new(value: &[u8]) -> Option<Self> {
        if value.is_empty() || value.len() > u8::MAX as usize {
            return None;
        }
        let mut bytes = [0; u8::MAX as usize];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self {
            length: value.len() as u8,
            bytes,
        })
    }

    fn model(self) -> String {
        decode_text(&self.bytes[..usize::from(self.length)])
    }
}

struct DialogEventSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<RawDialogEvent>>,
}

// SAFETY: the main thread owns WRITING slots and the IPC thread owns READING
// slots. State transitions publish initialized bytes before ownership moves.
unsafe impl Sync for DialogEventSlot {}

impl DialogEventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

struct DialogEvents {
    slots: [DialogEventSlot; EVENT_SLOTS],
}

impl DialogEvents {
    const fn new() -> Self {
        Self {
            slots: [const { DialogEventSlot::new() }; EVENT_SLOTS],
        }
    }

    fn push(&self, event: RawDialogEvent) -> Option<QueuedDialog> {
        for (index, slot) in self.slots.iter().enumerate() {
            if slot
                .state
                .compare_exchange(EMPTY, WRITING, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            // SAFETY: this producer owns WRITING until the release store.
            unsafe { (*slot.value.get()).write(event) };
            slot.state.store(READY, Ordering::Release);
            return Some(QueuedDialog(index as u8));
        }
        None
    }

    fn take(&self, queued: QueuedDialog) -> Option<RawDialogEvent> {
        let slot = self.slots.get(usize::from(queued.0))?;
        slot.state
            .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        // SAFETY: this consumer owns READING and READY publishes a complete Copy value.
        let event = unsafe { *(*slot.value.get()).assume_init_ref() };
        slot.state.store(EMPTY, Ordering::Release);
        Some(event)
    }

    fn release(&self, queued: QueuedDialog) {
        if let Some(slot) = self.slots.get(usize::from(queued.0)) {
            slot.state.store(EMPTY, Ordering::Release);
        }
    }

    fn reset(&self) {
        for slot in &self.slots {
            slot.state.store(EMPTY, Ordering::Release);
        }
    }
}

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe { *CURRENT.0.get() = RawDialog::empty() };
    EVENTS.reset();
    REVISION.store(0, Ordering::Release);
}

pub(crate) fn observe_server(body: &[u8]) -> Option<QueuedDialog> {
    if body.first().copied() == Some(0x30) && body.get(1).copied() == Some(10) {
        return close(DialogCloseReason::Server);
    }
    if !matches!(body.first(), Some(0x2F | 0x30)) || body.len() > MAX_DIALOG_PACKET_BYTES {
        return None;
    }
    let current = current_mut();
    let opened = !current.active();
    let revision = next_revision();
    *current = raw(body, revision, false)?;
    EVENTS.push(RawDialogEvent {
        kind: if opened {
            EventKind::Opened
        } else {
            EventKind::Changed
        },
        dialog: *current,
    })
}

pub(crate) fn submit(submission: DialogSubmission) -> Option<QueuedDialog> {
    let current = current_mut();
    if !current.active() {
        return None;
    }
    let previous_revision = current.revision;
    current.revision = next_revision();
    current.response_pending = !matches!(submission, DialogSubmission::Close);
    let submission = RawSubmission::from_model(submission)?;
    EVENTS.push(RawDialogEvent {
        kind: EventKind::Submitted {
            previous_revision,
            submission,
        },
        dialog: *current,
    })
}

pub(crate) fn close(reason: DialogCloseReason) -> Option<QueuedDialog> {
    let current = current_mut();
    if !current.active() {
        return None;
    }
    current.revision = next_revision();
    let previous = *current;
    *current = RawDialog::empty();
    EVENTS.push(RawDialogEvent {
        kind: EventKind::Closed(reason),
        dialog: previous,
    })
}

pub(crate) fn revision() -> Option<u32> {
    let current = current_mut();
    current.active().then_some(current.revision)
}

pub(crate) fn is_active() -> bool {
    current_mut().active()
}

pub(crate) fn is_pending() -> bool {
    let current = current_mut();
    current.active() && current.response_pending
}

pub(crate) fn copy_current(output: &mut RawDialog) {
    *output = *current_mut();
}

pub(crate) fn decode_current(raw: RawDialog) -> Option<DialogState> {
    raw.active().then(|| decode(raw)).transpose().ok().flatten()
}

pub(crate) fn take(queued: QueuedDialog) -> Option<DialogUpdate> {
    let event = EVENTS.take(queued)?;
    let state = decode(event.dialog).ok()?;
    Some(match event.kind {
        EventKind::Opened => DialogUpdate::Opened(state),
        EventKind::Changed => DialogUpdate::Changed(state),
        EventKind::Submitted {
            previous_revision,
            submission,
        } => DialogUpdate::Submitted {
            state,
            previous_revision,
            submission: submission.model(),
        },
        EventKind::Closed(reason) => DialogUpdate::Closed {
            previous: Some(state),
            reason,
        },
    })
}

pub(crate) fn release(queued: QueuedDialog) {
    EVENTS.release(queued);
}

fn current_mut() -> &'static mut RawDialog {
    // SAFETY: every caller runs on the client main thread.
    unsafe { &mut *CURRENT.0.get() }
}

fn raw(body: &[u8], revision: u32, response_pending: bool) -> Option<RawDialog> {
    let length = u16::try_from(body.len()).ok()?;
    let mut raw = RawDialog::empty();
    raw.revision = revision;
    raw.response_pending = response_pending;
    raw.length = length;
    raw.bytes[..body.len()].copy_from_slice(body);
    Some(raw)
}

impl RawSubmission {
    fn from_model(value: DialogSubmission) -> Option<Self> {
        Some(match value {
            DialogSubmission::Select { index, quantity } => Self::Select { index, quantity },
            DialogSubmission::Input { input } => Self::Input(DialogText::new(input.as_bytes())?),
            DialogSubmission::Previous => Self::Previous,
            DialogSubmission::Next => Self::Next,
            DialogSubmission::Close => Self::Close,
        })
    }

    fn model(self) -> DialogSubmission {
        match self {
            Self::Select { index, quantity } => DialogSubmission::Select { index, quantity },
            Self::Input(input) => DialogSubmission::Input {
                input: input.model(),
            },
            Self::Previous => DialogSubmission::Previous,
            Self::Next => DialogSubmission::Next,
            Self::Close => DialogSubmission::Close,
        }
    }
}

fn next_revision() -> u32 {
    let previous = REVISION.fetch_add(1, Ordering::AcqRel);
    let next = previous.wrapping_add(1);
    if next == 0 {
        REVISION.store(1, Ordering::Release);
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::{DialogChoice, DialogInteraction};

    #[test]
    fn decodes_merchant_choices() {
        let body = [
            0x2F, 0, 1, 0, 0, 0, 7, 0, 0x40, 0x1E, 2, 0, 0, 0, 0, 0, 5, b'G', b'u', b'i', b'd',
            b'e', 0, 7, b'C', b'h', b'o', b'o', b's', b'e', b'.', 1, 3, b'B', b'u', b'y', 1, 1,
        ];
        let state = decode(raw(&body, 1, false).unwrap()).unwrap();
        assert_eq!(state.revision, 1);
        assert_eq!(state.speaker.name.as_deref(), Some("Guide"));
        assert_eq!(state.speaker.sprite, 30);
        assert_eq!(
            state.interaction,
            DialogInteraction::Choices(vec![DialogChoice {
                index: 0,
                text: "Buy".into()
            }])
        );
    }

    #[test]
    fn decodes_pursuit_input() {
        let body = [
            0x30, 4, 1, 0, 0, 0, 7, 0, 0x40, 0x1E, 2, 0, 0, 0, 0, 0, 5, 0, 3, 1, 1, 0, 5, b'G',
            b'u', b'i', b'd', b'e', 0, 4, b'N', b'a', b'm', b'e', 1, b'>', 13, 1, b'.',
        ];
        let state = decode(raw(&body, 9, true).unwrap()).unwrap();
        assert!(state.response_pending);
        assert_eq!(state.content.as_deref(), Some("Name"));
        assert!(!state.navigation.previous);
        assert!(matches!(state.interaction, DialogInteraction::Input(_)));
    }

    #[test]
    fn fixed_event_slots_are_reusable_after_take_or_release() {
        let events = DialogEvents::new();
        let dialog = raw(&[0x30, 10], 1, false).unwrap();
        let event = RawDialogEvent {
            kind: EventKind::Closed(DialogCloseReason::Server),
            dialog,
        };

        for _ in 0..EVENT_SLOTS * 2 {
            let queued = events.push(event).unwrap();
            assert!(events.take(queued).is_some());
        }
        for _ in 0..EVENT_SLOTS * 2 {
            let queued = events.push(event).unwrap();
            events.release(queued);
        }
    }

    #[test]
    fn rejects_oversized_server_row_counts_before_allocation() {
        let body = [
            0x2F, 4, 1, 0, 0, 0, 7, 0, 0x40, 0x1E, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0xFF, 0xFF,
        ];
        assert!(decode(raw(&body, 1, false).unwrap()).is_err());
    }
}
