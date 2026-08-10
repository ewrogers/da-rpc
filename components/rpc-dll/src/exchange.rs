#![cfg_attr(any(not(windows), test), allow(dead_code))]

use darpc_model::{ExchangeItem, ExchangeOffer, ExchangeParty, ExchangeState, ExchangeUpdate};
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

const ITEM_CAPACITY: usize = darpc_protocol::MAX_EXCHANGE_ITEMS;
const TEXT_CAPACITY: usize = darpc_protocol::MAX_EXCHANGE_NAME_LEN;
// One exchange can contain eight items from each participant plus gold and
// acceptance updates. Keep enough retained slots for a complete transaction
// even when the pipe worker briefly falls behind the client thread.
const EVENT_SLOT_COUNT: usize = 32;
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;
const SPRITE_ID_MASK: u16 = 0x3FFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawText {
    length: u8,
    bytes: [u8; TEXT_CAPACITY],
}

impl RawText {
    const fn empty() -> Self {
        Self {
            length: 0,
            bytes: [0; TEXT_CAPACITY],
        }
    }

    fn from_bytes(value: &[u8]) -> Self {
        let length = value.len().min(TEXT_CAPACITY);
        let mut text = Self::empty();
        text.bytes[..length].copy_from_slice(&value[..length]);
        text.length = length as u8;
        text
    }

    fn model(self) -> String {
        decode_text(&self.bytes[..usize::from(self.length)])
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawItem {
    index: u8,
    sprite: u16,
    dye_color: u8,
    quantity: u8,
    name: RawText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawOffer {
    items: [Option<RawItem>; ITEM_CAPACITY],
    gold: u32,
    accepted: bool,
}

impl RawOffer {
    const fn empty() -> Self {
        Self {
            items: [None; ITEM_CAPACITY],
            gold: 0,
            accepted: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RawExchange {
    active: bool,
    id: u32,
    partner: RawText,
    local: RawOffer,
    other: RawOffer,
}

impl RawExchange {
    pub(crate) const fn empty() -> Self {
        Self {
            active: false,
            id: 0,
            partner: RawText::empty(),
            local: RawOffer::empty(),
            other: RawOffer::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PendingItem {
    pub(crate) slot: u8,
    pub(crate) quantity: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingInitialItem {
    item: PendingItem,
    tick_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawUpdateKind {
    Opened,
    ItemAdded {
        party: ExchangeParty,
        item: RawItem,
    },
    GoldChanged {
        party: ExchangeParty,
        gold: u32,
    },
    Accepted {
        party: ExchangeParty,
        message: RawText,
    },
    Completed {
        message: RawText,
    },
    Cancelled {
        message: RawText,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawUpdate {
    state: RawExchange,
    kind: RawUpdateKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedExchange(u8);

struct Tracker {
    state: RawExchange,
    pending_item: Option<PendingItem>,
    gold_pending: bool,
    accept_pending: bool,
    cancel_pending: bool,
    initial_item: Option<PendingInitialItem>,
}

impl Tracker {
    const fn new() -> Self {
        Self {
            state: RawExchange::empty(),
            pending_item: None,
            gold_pending: false,
            accept_pending: false,
            cancel_pending: false,
            initial_item: None,
        }
    }
}

struct TrackerCell(UnsafeCell<Tracker>);

// SAFETY: the client main thread is the only tracker reader and writer.
unsafe impl Sync for TrackerCell {}

struct EventSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<RawUpdate>>,
}

impl EventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

// SAFETY: event ownership moves through the slot state machine.
unsafe impl Sync for EventSlot {}

static TRACKER: TrackerCell = TrackerCell(UnsafeCell::new(Tracker::new()));
static EVENTS: [EventSlot; EVENT_SLOT_COUNT] = [const { EventSlot::new() }; EVENT_SLOT_COUNT];
pub(crate) static INTERCEPT_PENDING: AtomicU8 = AtomicU8::new(0);

pub(crate) fn reset() {
    // SAFETY: lifecycle reset runs without an installed producer or consumer.
    unsafe { *TRACKER.0.get() = Tracker::new() };
    INTERCEPT_PENDING.store(0, Ordering::Release);
    for slot in &EVENTS {
        slot.state.store(EMPTY, Ordering::Release);
    }
}

pub(crate) unsafe fn copy_current(output: &mut RawExchange) {
    // SAFETY: the caller runs on the client main thread during snapshot publication.
    unsafe { *output = (*TRACKER.0.get()).state };
}

pub(crate) fn decode_current(raw: RawExchange) -> Option<ExchangeState> {
    raw.active.then(|| model_state(raw))
}

pub(crate) fn active_id() -> Option<u32> {
    // SAFETY: commands run on the client main thread.
    let state = unsafe { (*TRACKER.0.get()).state };
    state.active.then_some(state.id)
}

pub(crate) fn observe_outgoing(body: &[u8], tick_ms: u32) {
    if body.first() != Some(&0x29) || body.len() != 10 {
        return;
    }
    let quantity = u32::from_be_bytes(body[6..10].try_into().expect("length checked"));
    let Ok(quantity) = u8::try_from(quantity) else {
        return;
    };
    if quantity == 0 {
        return;
    }
    // SAFETY: outgoing packet observation runs on the client main thread.
    unsafe {
        (*TRACKER.0.get()).initial_item = Some(PendingInitialItem {
            item: PendingItem {
                slot: body[1],
                quantity,
            },
            tick_ms,
        });
    }
}

pub(crate) fn begin_item(slot: u8, quantity: u8) -> bool {
    // SAFETY: commands run on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active
        || tracker.pending_item.is_some()
        || tracker.state.local.accepted
        || tracker.state.other.accepted
        || tracker.accept_pending
        || tracker.cancel_pending
    {
        return false;
    }
    tracker.pending_item = Some(PendingItem { slot, quantity });
    INTERCEPT_PENDING.store(1, Ordering::Release);
    true
}

pub(crate) fn abort_item() {
    // SAFETY: commands and packet observations run on the client main thread.
    unsafe { (*TRACKER.0.get()).pending_item = None };
    INTERCEPT_PENDING.store(0, Ordering::Release);
}

pub(crate) fn intercept_quantity(body: &[u8]) -> bool {
    if body.first() != Some(&0x42) || body.get(1) != Some(&0x01) {
        return false;
    }
    let Some(&slot) = body.get(2) else {
        return false;
    };
    // SAFETY: server-event interception runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    let Some(pending) = tracker.pending_item.filter(|pending| pending.slot == slot) else {
        return false;
    };
    #[cfg(all(windows, not(test)))]
    if crate::actions::exchange::continue_item(tracker.state.id, pending).is_err() {
        tracker.pending_item = None;
        INTERCEPT_PENDING.store(0, Ordering::Release);
        crate::state::mark_resync_required();
        return false;
    }
    #[cfg(any(not(windows), test))]
    let _ = pending;
    INTERCEPT_PENDING.store(0, Ordering::Release);
    true
}

pub(crate) fn begin_gold() -> bool {
    // SAFETY: commands run on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active
        || tracker.state.local.gold != 0
        || tracker.gold_pending
        || tracker.state.local.accepted
        || tracker.state.other.accepted
        || tracker.accept_pending
        || tracker.cancel_pending
    {
        return false;
    }
    tracker.gold_pending = true;
    true
}

pub(crate) fn abort_gold() {
    // SAFETY: commands run on the client main thread.
    unsafe { (*TRACKER.0.get()).gold_pending = false };
}

pub(crate) fn begin_accept() -> bool {
    // SAFETY: commands run on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active
        || tracker.state.local.accepted
        || tracker.accept_pending
        || tracker.cancel_pending
    {
        return false;
    }
    tracker.accept_pending = true;
    true
}

pub(crate) fn abort_accept() {
    // SAFETY: commands run on the client main thread.
    unsafe { (*TRACKER.0.get()).accept_pending = false };
}

pub(crate) fn begin_cancel() -> bool {
    // SAFETY: commands run on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active || tracker.cancel_pending {
        return false;
    }
    tracker.cancel_pending = true;
    true
}

pub(crate) fn abort_cancel() {
    // SAFETY: commands run on the client main thread.
    unsafe { (*TRACKER.0.get()).cancel_pending = false };
}

pub(crate) fn observe_server(body: &[u8], tick_ms: u32) {
    let Some(event) = body.get(1).copied() else {
        return;
    };
    match event {
        0x00 => observe_started(body, tick_ms),
        0x01 => {}
        0x02 => observe_item(body, tick_ms),
        0x03 => observe_gold(body, tick_ms),
        0x04 => observe_cancelled(body, tick_ms),
        0x05 => observe_accepted(body, tick_ms),
        _ => {}
    }
}

fn observe_started(body: &[u8], tick_ms: u32) {
    let Some(id) = read_u32(body, 2) else { return };
    let Some((partner, _)) = read_text(body, 6) else {
        return;
    };
    let state = RawExchange {
        active: true,
        id,
        partner,
        local: RawOffer::empty(),
        other: RawOffer::empty(),
    };
    // SAFETY: packet observation runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    let pending_item = tracker
        .initial_item
        .filter(|pending| tick_ms.wrapping_sub(pending.tick_ms) <= 3_000)
        .map(|pending| pending.item);
    *tracker = Tracker {
        state,
        pending_item,
        ..Tracker::new()
    };
    INTERCEPT_PENDING.store(u8::from(pending_item.is_some()), Ordering::Release);
    queue(
        RawUpdate {
            state,
            kind: RawUpdateKind::Opened,
        },
        tick_ms,
    );
}

fn observe_item(body: &[u8], tick_ms: u32) {
    let (Some(&party_raw), Some(&index), Some(sprite), Some(&dye_color)) =
        (body.get(2), body.get(3), read_u16(body, 4), body.get(6))
    else {
        return;
    };
    if usize::from(index) >= ITEM_CAPACITY {
        return;
    }
    let Some((name, _)) = read_text(body, 7) else {
        return;
    };
    let party = if party_raw == 0 {
        ExchangeParty::Local
    } else {
        ExchangeParty::Other
    };
    // SAFETY: packet observation runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active {
        return;
    }
    let quantity = if party == ExchangeParty::Local {
        INTERCEPT_PENDING.store(0, Ordering::Release);
        tracker
            .pending_item
            .take()
            .map_or(0, |pending| pending.quantity)
    } else {
        0
    };
    let item = RawItem {
        index,
        sprite: sprite & SPRITE_ID_MASK,
        dye_color,
        quantity,
        name,
    };
    offer_mut(&mut tracker.state, party).items[usize::from(index)] = Some(item);
    queue(
        RawUpdate {
            state: tracker.state,
            kind: RawUpdateKind::ItemAdded { party, item },
        },
        tick_ms,
    );
}

fn observe_gold(body: &[u8], tick_ms: u32) {
    let (Some(&party_raw), Some(gold)) = (body.get(2), read_u32(body, 3)) else {
        return;
    };
    let party = if party_raw == 0 {
        ExchangeParty::Local
    } else {
        ExchangeParty::Other
    };
    // SAFETY: packet observation runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active {
        return;
    }
    offer_mut(&mut tracker.state, party).gold = gold;
    if party == ExchangeParty::Local {
        tracker.gold_pending = false;
    }
    queue(
        RawUpdate {
            state: tracker.state,
            kind: RawUpdateKind::GoldChanged { party, gold },
        },
        tick_ms,
    );
}

fn observe_cancelled(body: &[u8], tick_ms: u32) {
    let Some((message, _)) = read_text(body, 3) else {
        return;
    };
    // SAFETY: packet observation runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active {
        return;
    }
    let state = tracker.state;
    #[cfg(all(windows, not(test)))]
    crate::actions::exchange::display_result(message.as_bytes(), false);
    queue(
        RawUpdate {
            state,
            kind: RawUpdateKind::Cancelled { message },
        },
        tick_ms,
    );
    INTERCEPT_PENDING.store(0, Ordering::Release);
    *tracker = Tracker::new();
}

fn observe_accepted(body: &[u8], tick_ms: u32) {
    let (Some(&party_raw), Some((message, _))) = (body.get(2), read_text(body, 3)) else {
        return;
    };
    let party = if party_raw == 0 {
        ExchangeParty::Local
    } else {
        ExchangeParty::Other
    };
    // SAFETY: packet observation runs on the client main thread.
    let tracker = unsafe { &mut *TRACKER.0.get() };
    if !tracker.state.active {
        return;
    }
    offer_mut(&mut tracker.state, party).accepted = true;
    if party == ExchangeParty::Local {
        tracker.accept_pending = false;
    }
    let state = tracker.state;
    let complete = state.local.accepted && state.other.accepted;
    let kind = if complete {
        #[cfg(all(windows, not(test)))]
        crate::actions::exchange::display_result(message.as_bytes(), true);
        RawUpdateKind::Completed { message }
    } else {
        RawUpdateKind::Accepted { party, message }
    };
    queue(RawUpdate { state, kind }, tick_ms);
    if complete {
        INTERCEPT_PENDING.store(0, Ordering::Release);
        *tracker = Tracker::new();
    }
}

fn offer_mut(state: &mut RawExchange, party: ExchangeParty) -> &mut RawOffer {
    match party {
        ExchangeParty::Local => &mut state.local,
        ExchangeParty::Other => &mut state.other,
    }
}

fn queue(update: RawUpdate, tick_ms: u32) {
    let Some((index, slot)) = EVENTS.iter().enumerate().find(|(_, slot)| {
        slot.state
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }) else {
        crate::state::mark_resync_required();
        return;
    };
    // SAFETY: WRITING gives the producer exclusive ownership.
    unsafe { (*slot.value.get()).write(update) };
    slot.state.store(READY, Ordering::Release);
    let queued = QueuedExchange(index as u8);
    if !crate::state::observe_exchange(queued, tick_ms) {
        release(queued);
    }
}

pub(crate) fn take(queued: QueuedExchange) -> Option<ExchangeUpdate> {
    let slot = EVENTS.get(usize::from(queued.0))?;
    slot.state
        .compare_exchange(READY, READING, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    // SAFETY: READING gives this consumer exclusive ownership.
    let raw = unsafe { (*slot.value.get()).assume_init_read() };
    slot.state.store(EMPTY, Ordering::Release);
    let state = model_state(raw.state);
    Some(match raw.kind {
        RawUpdateKind::Opened => ExchangeUpdate::Opened(state),
        RawUpdateKind::ItemAdded { party, item } => ExchangeUpdate::ItemAdded {
            state,
            party,
            item: model_item(item),
        },
        RawUpdateKind::GoldChanged { party, gold } => {
            ExchangeUpdate::GoldChanged { state, party, gold }
        }
        RawUpdateKind::Accepted { party, message } => ExchangeUpdate::Accepted {
            state,
            party,
            message: message.model(),
        },
        RawUpdateKind::Completed { message } => ExchangeUpdate::Completed {
            state,
            message: message.model(),
        },
        RawUpdateKind::Cancelled { message } => ExchangeUpdate::Cancelled {
            state,
            message: message.model(),
        },
    })
}

pub(crate) fn release(queued: QueuedExchange) {
    if let Some(slot) = EVENTS.get(usize::from(queued.0)) {
        let _ = slot
            .state
            .compare_exchange(READY, EMPTY, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn model_state(raw: RawExchange) -> ExchangeState {
    ExchangeState {
        id: raw.id,
        partner: raw.partner.model(),
        local: model_offer(raw.local),
        other: model_offer(raw.other),
    }
}

fn model_offer(raw: RawOffer) -> ExchangeOffer {
    ExchangeOffer {
        items: raw.items.into_iter().flatten().map(model_item).collect(),
        gold: raw.gold,
        accepted: raw.accepted,
    }
}

fn model_item(raw: RawItem) -> ExchangeItem {
    let (name, name_quantity) = exchange_item_name(raw.name.model());
    ExchangeItem {
        index: raw.index,
        sprite: raw.sprite,
        dye_color: raw.dye_color,
        quantity: (raw.quantity != 0)
            .then_some(raw.quantity)
            .or(name_quantity)
            .or(Some(1)),
        name,
    }
}

fn exchange_item_name(value: String) -> (String, Option<u8>) {
    let Some(without_close) = value.strip_suffix(')') else {
        return (value, None);
    };
    let Some((name, quantity)) = without_close.rsplit_once('(') else {
        return (value, None);
    };
    let Ok(quantity) = quantity.parse::<u8>() else {
        return (value, None);
    };
    if name.is_empty() || quantity == 0 {
        return (value, None);
    }
    (name.trim_end().to_owned(), Some(quantity))
}

fn read_u16(body: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        body.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(body: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        body.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_text(body: &[u8], offset: usize) -> Option<(RawText, usize)> {
    let length = usize::from(*body.get(offset)?);
    let start = offset + 1;
    let end = start.checked_add(length)?;
    Some((RawText::from_bytes(body.get(start..end)?), end))
}

#[cfg(windows)]
fn decode_text(bytes: &[u8]) -> String {
    crate::client_text::decode(bytes).unwrap_or_default()
}

#[cfg(not(windows))]
fn decode_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::exchange_item_name;

    #[test]
    fn normalizes_server_stack_counts() {
        assert_eq!(
            exchange_item_name("Red Potion(2)".into()),
            ("Red Potion".into(), Some(2))
        );
        assert_eq!(
            exchange_item_name("Andor Soroni".into()),
            ("Andor Soroni".into(), None)
        );
    }
}
