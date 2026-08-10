#![cfg_attr(not(windows), allow(dead_code))]

use darpc_model::{LegendIcon, LegendMark, LegendUpdate};
use darpc_protocol::{MAX_LEGEND_MARKS, MAX_LEGEND_TEXT_LEN};
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;
const TRACKER_EMPTY: u8 = 0;
const TRACKER_ACTIVE: u8 = 1;
const TRACKER_READING: u8 = 2;
const TRACKER_WRITING: u8 = 3;

#[derive(Clone, Copy)]
struct RawLegendText {
    bytes: [u8; MAX_LEGEND_TEXT_LEN],
    length: u8,
}

impl RawLegendText {
    const fn empty() -> Self {
        Self {
            bytes: [0; MAX_LEGEND_TEXT_LEN],
            length: 0,
        }
    }

    fn from_bytes(value: &[u8]) -> Option<Self> {
        if value.len() > MAX_LEGEND_TEXT_LEN {
            return None;
        }
        let mut text = Self::empty();
        text.bytes[..value.len()].copy_from_slice(value);
        text.length = u8::try_from(value.len()).ok()?;
        Some(text)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RawLegendMark {
    icon: u8,
    color: u8,
    tag: RawLegendText,
    text: RawLegendText,
}

impl RawLegendMark {
    const fn empty() -> Self {
        Self {
            icon: 0,
            color: 0,
            tag: RawLegendText::empty(),
            text: RawLegendText::empty(),
        }
    }
}

impl PartialEq for RawLegendMark {
    fn eq(&self, other: &Self) -> bool {
        self.icon == other.icon
            && self.color == other.color
            && self.tag.as_bytes() == other.tag.as_bytes()
            && self.text.as_bytes() == other.text.as_bytes()
    }
}

impl Eq for RawLegendMark {}

#[derive(Clone, Copy)]
pub(crate) struct RawLegendState {
    marks: [RawLegendMark; MAX_LEGEND_MARKS],
    count: u8,
}

impl RawLegendState {
    pub(crate) const fn empty() -> Self {
        Self {
            marks: [RawLegendMark::empty(); MAX_LEGEND_MARKS],
            count: 0,
        }
    }
}

struct TrackerCell([UnsafeCell<RawLegendState>; 3]);

// SAFETY: packet observation is the sole writer. The active index is published
// only after the inactive state has been completely populated.
unsafe impl Sync for TrackerCell {}

static TRACKERS: TrackerCell = TrackerCell([
    UnsafeCell::new(RawLegendState::empty()),
    UnsafeCell::new(RawLegendState::empty()),
    UnsafeCell::new(RawLegendState::empty()),
]);
static TRACKER_STATES: [AtomicU8; 3] = [
    AtomicU8::new(TRACKER_ACTIVE),
    AtomicU8::new(TRACKER_EMPTY),
    AtomicU8::new(TRACKER_EMPTY),
];
static ACTIVE_INDEX: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy)]
// Queue values remain fixed and pointer-free so packet observation never allocates.
#[allow(clippy::large_enum_variant)]
enum RawLegendUpdate {
    Added(RawLegendMark),
    Changed {
        previous: RawLegendMark,
        current: RawLegendMark,
    },
    Removed(RawLegendMark),
}

struct EventSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<RawLegendUpdate>>,
}

// SAFETY: the atomic state transfers exclusive slot ownership between the
// client main-thread producer and IPC consumer.
unsafe impl Sync for EventSlot {}

impl EventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

static EVENTS: [EventSlot; MAX_LEGEND_MARKS] = [const { EventSlot::new() }; MAX_LEGEND_MARKS];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedLegend(u8);

pub(crate) fn reset() {
    ACTIVE_INDEX.store(0, Ordering::Release);
    // SAFETY: reset runs outside the installed producer/consumer lifecycle.
    unsafe {
        *TRACKERS.0[0].get() = RawLegendState::empty();
        *TRACKERS.0[1].get() = RawLegendState::empty();
        *TRACKERS.0[2].get() = RawLegendState::empty();
    }
    TRACKER_STATES[0].store(TRACKER_ACTIVE, Ordering::Release);
    TRACKER_STATES[1].store(TRACKER_EMPTY, Ordering::Release);
    TRACKER_STATES[2].store(TRACKER_EMPTY, Ordering::Release);
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) fn observe_self_look(body: &[u8], tick_ms: u32) {
    let active = usize::from(ACTIVE_INDEX.load(Ordering::Acquire));
    let Some(next) = TRACKER_STATES
        .iter()
        .enumerate()
        .find_map(|(index, state)| {
            (index != active
                && state
                    .compare_exchange(
                        TRACKER_EMPTY,
                        TRACKER_WRITING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok())
            .then_some(index)
        })
    else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: packet observation is the sole writer and writes only inactive state.
    let current = unsafe { &*TRACKERS.0[active].get() };
    // SAFETY: the inactive buffer is not visible to readers until publication.
    let replacement = unsafe { &mut *TRACKERS.0[next].get() };
    if !parse(body, replacement) {
        TRACKER_STATES[next].store(TRACKER_EMPTY, Ordering::Release);
        return;
    }

    let common = usize::from(current.count.min(replacement.count));
    for index in 0..common {
        if current.marks[index] != replacement.marks[index] {
            queue(
                RawLegendUpdate::Changed {
                    previous: current.marks[index],
                    current: replacement.marks[index],
                },
                tick_ms,
            );
        }
    }
    for mark in replacement
        .marks
        .iter()
        .copied()
        .skip(common)
        .take(usize::from(replacement.count) - common)
    {
        queue(RawLegendUpdate::Added(mark), tick_ms);
    }
    for mark in current
        .marks
        .iter()
        .copied()
        .skip(common)
        .take(usize::from(current.count) - common)
    {
        queue(RawLegendUpdate::Removed(mark), tick_ms);
    }

    TRACKER_STATES[next].store(TRACKER_ACTIVE, Ordering::Release);
    ACTIVE_INDEX.store(
        u8::try_from(next).expect("legend tracker index fits u8"),
        Ordering::Release,
    );
    let _ = TRACKER_STATES[active].compare_exchange(
        TRACKER_ACTIVE,
        TRACKER_EMPTY,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    crate::commands::complete_legend();
}

pub(crate) fn copy_current(output: &mut RawLegendState) {
    let active = usize::from(ACTIVE_INDEX.load(Ordering::Acquire));
    // SAFETY: the active buffer is immutable until a later active-index swap.
    *output = unsafe { *TRACKERS.0[active].get() };
}

pub(crate) fn current() -> Vec<LegendMark> {
    loop {
        let active = usize::from(ACTIVE_INDEX.load(Ordering::Acquire));
        if TRACKER_STATES[active]
            .compare_exchange(
                TRACKER_ACTIVE,
                TRACKER_READING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            std::hint::spin_loop();
            continue;
        }
        // SAFETY: READING prevents the producer from reusing this buffer.
        let model = model_state(unsafe { &*TRACKERS.0[active].get() });
        let next_state = if usize::from(ACTIVE_INDEX.load(Ordering::Acquire)) == active {
            TRACKER_ACTIVE
        } else {
            TRACKER_EMPTY
        };
        TRACKER_STATES[active].store(next_state, Ordering::Release);
        return model;
    }
}

pub(crate) fn model_state(raw: &RawLegendState) -> Vec<LegendMark> {
    raw.marks
        .iter()
        .copied()
        .take(usize::from(raw.count))
        .filter_map(mark_model)
        .collect()
}

pub(crate) fn take(queued: QueuedLegend) -> Option<LegendUpdate> {
    let slot = EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive ownership of the value.
    let raw = unsafe { (*slot.value.get()).assume_init_read() };
    slot.state.store(EMPTY, Ordering::Release);
    Some(match raw {
        RawLegendUpdate::Added(mark) => LegendUpdate::MarkAdded {
            mark: mark_model(mark)?,
        },
        RawLegendUpdate::Changed { previous, current } => LegendUpdate::MarkChanged {
            previous: mark_model(previous)?,
            current: mark_model(current)?,
        },
        RawLegendUpdate::Removed(mark) => LegendUpdate::MarkRemoved {
            mark: mark_model(mark)?,
        },
    })
}

pub(crate) fn release(queued: QueuedLegend) {
    if let Some(slot) = EVENTS.get(usize::from(queued.0)) {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

fn queue(update: RawLegendUpdate, tick_ms: u32) {
    let Some((index, slot)) = EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }) else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives this producer exclusive ownership of the slot.
    unsafe { (*slot.value.get()).write(update) };
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedLegend(u8::try_from(index).expect("legend event index fits u8"));
    if !crate::state::observe_legend(queued, tick_ms) {
        release(queued);
    }
}

fn parse(body: &[u8], output: &mut RawLegendState) -> bool {
    if body.first() != Some(&0x39) {
        return false;
    }
    let mut offset = 2;
    for _ in 0..3 {
        if take_string(body, &mut offset).is_none() {
            return false;
        }
    }
    let Some(is_recruiting) = body.get(offset + 1).copied() else {
        return false;
    };
    offset = match offset.checked_add(2) {
        Some(offset) if offset <= body.len() => offset,
        _ => return false,
    };
    if is_recruiting == 1 {
        for _ in 0..3 {
            if take_string(body, &mut offset).is_none() {
                return false;
            }
        }
        offset = match offset.checked_add(12) {
            Some(offset) if offset <= body.len() => offset,
            _ => return false,
        };
    }
    offset = match offset.checked_add(3) {
        Some(offset) if offset <= body.len() => offset,
        _ => return false,
    };
    if take_string(body, &mut offset).is_none() || take_string(body, &mut offset).is_none() {
        return false;
    }
    let Some(count) = body.get(offset).copied() else {
        return false;
    };
    offset += 1;
    output.count = 0;
    for index in 0..usize::from(count) {
        let Some(icon) = body.get(offset).copied() else {
            return false;
        };
        let Some(color) = body.get(offset + 1).copied() else {
            return false;
        };
        offset += 2;
        let Some(tag) = take_string(body, &mut offset).and_then(RawLegendText::from_bytes) else {
            return false;
        };
        let Some(text) = take_string(body, &mut offset).and_then(RawLegendText::from_bytes) else {
            return false;
        };
        output.marks[index] = RawLegendMark {
            icon,
            color,
            tag,
            text,
        };
        output.count = output.count.saturating_add(1);
    }
    true
}

fn take_string<'a>(body: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let length = usize::from(*body.get(*offset)?);
    *offset = offset.checked_add(1)?;
    let end = offset.checked_add(length)?;
    let value = body.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn mark_model(raw: RawLegendMark) -> Option<LegendMark> {
    Some(LegendMark {
        icon: LegendIcon::from_raw(raw.icon),
        color: raw.color,
        tag: decode(raw.tag.as_bytes())?,
        text: decode(raw.text.as_bytes())?,
    })
}

#[cfg(windows)]
fn decode(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes).or_else(|| bytes.is_empty().then(String::new))
}

#[cfg(not(windows))]
fn decode(bytes: &[u8]) -> Option<String> {
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(marks: &[(u8, u8, &[u8], &[u8])]) -> Vec<u8> {
        let mut body = vec![0x39, 1, 0, 7, 0, 0, 0, 0, 0, 0, 1, 0];
        body.extend_from_slice(&[1, 0, 1, 1, 1, 1, b'W', 0]);
        body.push(marks.len() as u8);
        for (icon, color, tag, text) in marks {
            body.extend_from_slice(&[*icon, *color, tag.len() as u8]);
            body.extend_from_slice(tag);
            body.push(text.len() as u8);
            body.extend_from_slice(text);
        }
        body
    }

    #[test]
    fn self_look_parser_reads_legend_fields_in_wire_order() {
        let mut state = RawLegendState::empty();
        assert!(parse(
            &packet(&[(3, 7, b"Quest", b"Found the hidden grove")]),
            &mut state
        ));
        assert_eq!(
            model_state(&state),
            vec![LegendMark {
                text: "Found the hidden grove".into(),
                tag: "Quest".into(),
                color: 7,
                icon: LegendIcon::Wizard,
            }]
        );
    }

    #[test]
    fn self_look_parser_skips_the_optional_recruiting_block() {
        let mut body = vec![0x39, 1, 0, 0, 0, 1, 1];
        for value in [b"Lead".as_slice(), b"Team", b"Note"] {
            body.push(value.len() as u8);
            body.extend_from_slice(value);
        }
        body.extend_from_slice(&[1, 99]);
        body.extend_from_slice(&[0; 10]);
        body.extend_from_slice(&[3, 1, 1, 8]);
        body.extend_from_slice(b"Summoner");
        body.extend_from_slice(&[5]);
        body.extend_from_slice(b"Guild");
        body.extend_from_slice(&[1, 3, 7, 5]);
        body.extend_from_slice(b"Quest");
        body.extend_from_slice(&[4]);
        body.extend_from_slice(b"Done");
        let mut state = RawLegendState::empty();
        assert!(parse(&body, &mut state));
        assert_eq!(model_state(&state)[0].text, "Done");
    }
}
