#![cfg_attr(test, allow(dead_code))]

use crate::{atomic_sequence::next_nonzero, client_text};
use darpc_model::{
    BulletinCompose, BulletinEntry, BulletinEntrySummary, BulletinOperation,
    BulletinOperationResult, BulletinPagination, BulletinSection, BulletinSectionKind,
    BulletinSource, BulletinState, BulletinUpdate, BulletinView, BulletinViewport,
};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
};

pub(crate) const MAX_BULLETIN_SECTIONS: usize = 64;
pub(crate) const MAX_BULLETIN_ENTRIES: usize = 128;
pub(crate) const MAX_BULLETIN_TEXT_BYTES: usize = 255;
pub(crate) const MAX_BULLETIN_BODY_BYTES: usize = darpc_protocol::MAX_BULLETIN_BODY_LEN;
const EVENT_SLOTS: usize = 4;
const PANE_OBSERVATION_INTERVAL_MS: u32 = 100;
const NO_ID: i16 = -1;
const SLOT_EMPTY: u8 = 0;
const SLOT_WRITING: u8 = 1;
const SLOT_READY: u8 = 2;
const SLOT_READING: u8 = 3;

const VIEW_NONE: u8 = 0;
pub(crate) const VIEW_SECTIONS: u8 = 1;
pub(crate) const VIEW_ENTRIES: u8 = 2;
pub(crate) const VIEW_ENTRY: u8 = 3;
pub(crate) const VIEW_BOARD_COMPOSE: u8 = 4;
pub(crate) const VIEW_MAIL_COMPOSE: u8 = 5;

static CURRENT: CurrentBulletin = CurrentBulletin(UnsafeCell::new(RawBulletin::empty()));
static SCRATCH: CurrentBulletin = CurrentBulletin(UnsafeCell::new(RawBulletin::empty()));
static EMPTY_BULLETIN: RawBulletin = RawBulletin::empty();
static EVENTS: BulletinEvents = BulletinEvents::new();
static REVISION: AtomicU32 = AtomicU32::new(0);
static PANE_OBSERVATION_SCHEDULED: AtomicBool = AtomicBool::new(false);
static NEXT_PANE_OBSERVATION_TICK_MS: AtomicU32 = AtomicU32::new(0);

struct CurrentBulletin(UnsafeCell<RawBulletin>);

// SAFETY: CURRENT and SCRATCH are read and written only by the client main
// thread. Copies cross to the IPC thread through snapshot publication or EVENTS.
unsafe impl Sync for CurrentBulletin {}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct RawText<const N: usize> {
    length: u16,
    bytes: [u8; N],
}

impl<const N: usize> RawText<N> {
    const fn empty() -> Self {
        Self {
            length: 0,
            bytes: [0; N],
        }
    }

    pub(crate) fn set(&mut self, value: &[u8]) -> bool {
        if value.len() > N || value.len() > u16::MAX as usize {
            return false;
        }
        let old_length = usize::from(self.length);
        let changed = self.as_bytes() != value;
        if !changed {
            return false;
        }
        self.bytes[..old_length.max(value.len())].fill(0);
        self.bytes[..value.len()].copy_from_slice(value);
        self.length = value.len() as u16;
        true
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length).min(N)]
    }

    fn decode(&self) -> Option<String> {
        client_text::decode_or_empty(self.as_bytes())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct RawBulletinSection {
    id: u16,
    kind: u8,
    source: u8,
    name: RawText<MAX_BULLETIN_TEXT_BYTES>,
}

impl RawBulletinSection {
    const fn empty() -> Self {
        Self {
            id: 0,
            kind: 0,
            source: 0,
            name: RawText::empty(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawBulletinEntrySummary {
    id: i16,
    flags: u8,
    month: u8,
    day: u8,
    author: RawText<MAX_BULLETIN_TEXT_BYTES>,
    subject: RawText<MAX_BULLETIN_TEXT_BYTES>,
}

impl RawBulletinEntrySummary {
    const fn empty() -> Self {
        Self {
            id: NO_ID,
            flags: 0,
            month: 0,
            day: 0,
            author: RawText::empty(),
            subject: RawText::empty(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawBulletinEntry {
    id: i16,
    flags: u8,
    has_flags: bool,
    month: u8,
    day: u8,
    navigation_flags: u8,
    unknown_before_id: u8,
    author: RawText<MAX_BULLETIN_TEXT_BYTES>,
    subject: RawText<MAX_BULLETIN_TEXT_BYTES>,
    body: RawText<MAX_BULLETIN_BODY_BYTES>,
}

impl RawBulletinEntry {
    const fn empty() -> Self {
        Self {
            id: NO_ID,
            flags: 0,
            has_flags: false,
            month: 0,
            day: 0,
            navigation_flags: 0,
            unknown_before_id: 0,
            author: RawText::empty(),
            subject: RawText::empty(),
            body: RawText::empty(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct RawBulletin {
    revision: u32,
    active: bool,
    view: u8,
    pending: u8,
    has_pending: bool,
    can_go_back: bool,
    can_go_forward: bool,
    heading: RawText<MAX_BULLETIN_TEXT_BYTES>,
    section: RawBulletinSection,
    sections: [RawBulletinSection; MAX_BULLETIN_SECTIONS],
    section_count: u8,
    selected_section_id: u16,
    has_selected_section: bool,
    entries: [RawBulletinEntrySummary; MAX_BULLETIN_ENTRIES],
    entry_count: u8,
    selected_entry_id: i16,
    has_selected_entry: bool,
    pagination: u8,
    viewport_position: i32,
    viewport_maximum: i32,
    truncated: bool,
    entry: RawBulletinEntry,
    compose_recipient: RawText<MAX_BULLETIN_TEXT_BYTES>,
    compose_recipient_editable: bool,
    compose_subject: RawText<MAX_BULLETIN_TEXT_BYTES>,
    compose_body: RawText<{ darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN }>,
    has_result: bool,
    result_operation: u8,
    result_status: u8,
    result_has_message: bool,
    result_message: RawText<MAX_BULLETIN_TEXT_BYTES>,
}

impl RawBulletin {
    pub(crate) const fn empty() -> Self {
        Self {
            revision: 0,
            active: false,
            view: VIEW_NONE,
            pending: 0,
            has_pending: false,
            can_go_back: false,
            can_go_forward: false,
            heading: RawText::empty(),
            section: RawBulletinSection::empty(),
            sections: [RawBulletinSection::empty(); MAX_BULLETIN_SECTIONS],
            section_count: 0,
            selected_section_id: 0,
            has_selected_section: false,
            entries: [RawBulletinEntrySummary::empty(); MAX_BULLETIN_ENTRIES],
            entry_count: 0,
            selected_entry_id: NO_ID,
            has_selected_entry: false,
            pagination: 0,
            viewport_position: 0,
            viewport_maximum: 0,
            truncated: false,
            entry: RawBulletinEntry::empty(),
            compose_recipient: RawText::empty(),
            compose_recipient_editable: false,
            compose_subject: RawText::empty(),
            compose_body: RawText::empty(),
            has_result: false,
            result_operation: 0,
            result_status: 0,
            result_has_message: false,
            result_message: RawText::empty(),
        }
    }

    pub(crate) const fn active(&self) -> bool {
        self.active && self.view != VIEW_NONE
    }

    pub(crate) const fn view(&self) -> u8 {
        self.view
    }

    pub(crate) const fn section_id(&self) -> u16 {
        self.section.id
    }

    pub(crate) const fn entry_id(&self) -> i16 {
        self.entry.id
    }

    pub(crate) fn entry_author(&self) -> &[u8] {
        self.entry.author.as_bytes()
    }

    pub(crate) fn compose_recipient(&self) -> &[u8] {
        self.compose_recipient.as_bytes()
    }

    pub(crate) fn compose_subject(&self) -> &[u8] {
        self.compose_subject.as_bytes()
    }

    pub(crate) fn compose_body(&self) -> &[u8] {
        self.compose_body.as_bytes()
    }

    pub(crate) fn oldest_entry_id(&self) -> Option<i16> {
        self.entries
            .iter()
            .take(usize::from(self.entry_count))
            .map(|entry| entry.id)
            .min()
    }

    pub(crate) const fn is_mailbox(&self) -> bool {
        self.section.kind == 2
    }

    pub(crate) fn has_section(&self, id: u16) -> bool {
        self.view == VIEW_SECTIONS
            && self.sections[..usize::from(self.section_count)]
                .iter()
                .any(|section| section.id == id)
    }

    pub(crate) fn has_entry(&self, id: i16) -> bool {
        (self.view == VIEW_ENTRIES
            && self.entries[..usize::from(self.entry_count)]
                .iter()
                .any(|entry| entry.id == id))
            || (self.view == VIEW_ENTRY && self.entry.id == id)
    }

    pub(crate) fn set_compose_author(&mut self, value: &[u8]) -> bool {
        self.entry.author.set(value)
    }

    pub(crate) fn set_ui_navigation(&mut self, can_go_back: bool, can_go_forward: bool) -> bool {
        let changed = self.can_go_back != can_go_back || self.can_go_forward != can_go_forward;
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
        changed
    }

    pub(crate) fn set_ui_view(&mut self, view: u8) -> bool {
        if self.view == view {
            return false;
        }
        self.view = view;
        true
    }

    pub(crate) fn set_selected_section(&mut self, value: Option<u16>) -> bool {
        let changed = self.has_selected_section != value.is_some()
            || value.is_some_and(|value| value != self.selected_section_id);
        self.has_selected_section = value.is_some();
        self.selected_section_id = value.unwrap_or_default();
        changed
    }

    pub(crate) fn set_selected_entry(&mut self, value: Option<i16>) -> bool {
        let changed = self.has_selected_entry != value.is_some()
            || value.is_some_and(|value| value != self.selected_entry_id);
        self.has_selected_entry = value.is_some();
        self.selected_entry_id = value.unwrap_or(NO_ID);
        changed
    }

    pub(crate) fn set_viewport(&mut self, position: i32, maximum: i32) -> bool {
        let changed = self.viewport_position != position || self.viewport_maximum != maximum;
        self.viewport_position = position;
        self.viewport_maximum = maximum;
        changed
    }

    pub(crate) fn set_compose_recipient(&mut self, value: &[u8], editable: bool) -> bool {
        let changed =
            self.compose_recipient.set(value) || self.compose_recipient_editable != editable;
        self.compose_recipient_editable = editable;
        changed
    }

    pub(crate) fn set_compose_subject(&mut self, value: &[u8]) -> bool {
        self.compose_subject.set(value)
    }

    pub(crate) fn set_compose_body(&mut self, value: &[u8]) -> bool {
        self.compose_body.set(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedBulletin(u8);

#[derive(Clone, Copy)]
enum EventKind {
    Opened,
    Changed,
    OperationResult,
    Closed,
}

#[derive(Clone, Copy)]
struct RawBulletinEventMetadata {
    kind: EventKind,
    state_available: bool,
}

struct BulletinEventSlot {
    state: AtomicU8,
    metadata: UnsafeCell<RawBulletinEventMetadata>,
    bulletin: UnsafeCell<RawBulletin>,
}

// SAFETY: the atomic state machine gives the main-thread producer or IPC
// consumer exclusive access before either touches the slot storage.
unsafe impl Sync for BulletinEventSlot {}

impl BulletinEventSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_EMPTY),
            metadata: UnsafeCell::new(RawBulletinEventMetadata {
                kind: EventKind::Changed,
                state_available: false,
            }),
            bulletin: UnsafeCell::new(RawBulletin::empty()),
        }
    }

    fn try_write(&self, metadata: RawBulletinEventMetadata, bulletin: &RawBulletin) -> bool {
        if self
            .state
            .compare_exchange(
                SLOT_EMPTY,
                SLOT_WRITING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }
        // SAFETY: SLOT_WRITING grants this producer exclusive access. Copying
        // directly into the slot avoids constructing a large stack temporary.
        unsafe {
            self.metadata.get().write(metadata);
            std::ptr::copy_nonoverlapping(bulletin, self.bulletin.get(), 1);
        }
        self.state.store(SLOT_READY, Ordering::Release);
        true
    }

    fn try_read<T>(
        &self,
        operation: impl FnOnce(RawBulletinEventMetadata, &RawBulletin) -> T,
    ) -> Option<T> {
        self.state
            .compare_exchange(
                SLOT_READY,
                SLOT_READING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()?;
        // SAFETY: SLOT_READING grants this consumer exclusive access and the
        // producer's release guarantees both values are initialized.
        let result = unsafe { operation(*self.metadata.get(), &*self.bulletin.get()) };
        self.state.store(SLOT_EMPTY, Ordering::Release);
        Some(result)
    }

    fn discard(&self) {
        let _ = self.state.compare_exchange(
            SLOT_READY,
            SLOT_EMPTY,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn reset(&self) {
        self.state.store(SLOT_EMPTY, Ordering::Release);
    }
}

struct BulletinEvents {
    slots: [BulletinEventSlot; EVENT_SLOTS],
}

impl BulletinEvents {
    const fn new() -> Self {
        Self {
            slots: [const { BulletinEventSlot::new() }; EVENT_SLOTS],
        }
    }

    fn push(
        &self,
        metadata: RawBulletinEventMetadata,
        bulletin: &RawBulletin,
    ) -> Option<QueuedBulletin> {
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.try_write(metadata, bulletin) {
                return Some(QueuedBulletin(index as u8));
            }
        }
        None
    }

    fn read<T>(
        &self,
        queued: QueuedBulletin,
        operation: impl FnOnce(RawBulletinEventMetadata, &RawBulletin) -> T,
    ) -> Option<T> {
        self.slots.get(usize::from(queued.0))?.try_read(operation)
    }

    fn release(&self, queued: QueuedBulletin) {
        if let Some(slot) = self.slots.get(usize::from(queued.0)) {
            slot.discard();
        }
    }

    fn reset(&self) {
        for slot in &self.slots {
            slot.reset();
        }
    }
}

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe { *CURRENT.0.get() = RawBulletin::empty() };
    EVENTS.reset();
    REVISION.store(0, Ordering::Release);
    PANE_OBSERVATION_SCHEDULED.store(false, Ordering::Relaxed);
    NEXT_PANE_OBSERVATION_TICK_MS.store(0, Ordering::Relaxed);
}

pub(crate) fn observe_server(body: &[u8]) -> Option<QueuedBulletin> {
    let subtype = body.get(1).copied()?;
    let current = current_mut();
    let scratch = scratch_mut();
    let was_active = current.active();
    copy_raw(if was_active { current } else { &EMPTY_BULLETIN }, scratch);
    apply_server(scratch, body).ok()?;
    scratch.active = true;
    scratch.revision = next_nonzero(&REVISION);
    let kind = if matches!(subtype, 6..=8) {
        EventKind::OperationResult
    } else if was_active {
        EventKind::Changed
    } else {
        EventKind::Opened
    };
    copy_raw(scratch, current);
    EVENTS.push(
        RawBulletinEventMetadata {
            kind,
            state_available: true,
        },
        current,
    )
}

pub(crate) fn observe_outgoing(body: &[u8]) -> Option<QueuedBulletin> {
    let operation = outgoing_operation(body)?;
    let current = current_mut();
    if !current.active() {
        return None;
    }
    current.has_pending = true;
    current.pending = operation_raw(operation);
    if operation == BulletinOperation::LoadOlder {
        current.pagination = pagination_raw(BulletinPagination::Loading);
    }
    current.revision = next_nonzero(&REVISION);
    EVENTS.push(
        RawBulletinEventMetadata {
            kind: EventKind::Changed,
            state_available: true,
        },
        current,
    )
}

pub(crate) fn observe_local_submission(_operation: BulletinOperation) -> Option<QueuedBulletin> {
    let current = current_mut();
    if !current.active() {
        return None;
    }
    current.revision = next_nonzero(&REVISION);
    EVENTS.push(
        RawBulletinEventMetadata {
            kind: EventKind::Changed,
            state_available: true,
        },
        current,
    )
}

pub(crate) fn observe_pane(tick_ms: u32) -> Option<QueuedBulletin> {
    if !current_mut().active() {
        PANE_OBSERVATION_SCHEDULED.store(false, Ordering::Relaxed);
        return None;
    }
    if PANE_OBSERVATION_SCHEDULED.load(Ordering::Relaxed)
        && !crate::wrapping_time::deadline_reached(
            tick_ms,
            NEXT_PANE_OBSERVATION_TICK_MS.load(Ordering::Relaxed),
        )
    {
        return None;
    }
    NEXT_PANE_OBSERVATION_TICK_MS.store(
        tick_ms.wrapping_add(PANE_OBSERVATION_INTERVAL_MS),
        Ordering::Relaxed,
    );
    PANE_OBSERVATION_SCHEDULED.store(true, Ordering::Relaxed);

    #[cfg(all(windows, not(test)))]
    let observation = crate::actions::bulletin::observe_ui(current_mut());
    #[cfg(any(not(windows), test))]
    let observation = Ok(false);

    match observation {
        Ok(false) => None,
        Ok(true) => changed(),
        Err(()) => close(),
    }
}

pub(crate) fn current_revision() -> Option<u32> {
    current_mut().active().then_some(current_mut().revision)
}

pub(crate) fn with_current<T>(operation: impl FnOnce(&RawBulletin) -> T) -> Option<T> {
    let current = current_mut();
    current.active().then(|| operation(current))
}

#[cfg(all(windows, not(test)))]
pub(crate) fn refresh_ui() -> Result<(), ()> {
    crate::actions::bulletin::observe_ui(current_mut()).map(|_| ())
}

pub(crate) fn copy_current(output: &mut RawBulletin) {
    copy_raw(current_mut(), output);
}

pub(crate) fn decode_current(raw: &RawBulletin) -> Option<BulletinState> {
    raw.active().then(|| decode(raw)).transpose().ok().flatten()
}

pub(crate) fn take(queued: QueuedBulletin) -> Option<BulletinUpdate> {
    EVENTS
        .read(queued, |event, bulletin| {
            let state = event
                .state_available
                .then(|| decode(bulletin))
                .transpose()
                .ok()
                .flatten();
            Some(match event.kind {
                EventKind::Opened => BulletinUpdate::Opened(state?),
                EventKind::Changed => BulletinUpdate::Changed(state?),
                EventKind::OperationResult => {
                    let state = state?;
                    let result = state.last_operation_result.clone()?;
                    BulletinUpdate::OperationResult { state, result }
                }
                EventKind::Closed => BulletinUpdate::Closed { previous: state? },
            })
        })
        .flatten()
}

pub(crate) fn release(queued: QueuedBulletin) {
    EVENTS.release(queued);
}

fn changed() -> Option<QueuedBulletin> {
    let current = current_mut();
    current.revision = next_nonzero(&REVISION);
    EVENTS.push(
        RawBulletinEventMetadata {
            kind: EventKind::Changed,
            state_available: true,
        },
        current,
    )
}

fn close() -> Option<QueuedBulletin> {
    let current = current_mut();
    if !current.active() {
        return None;
    }
    let event = EVENTS.push(
        RawBulletinEventMetadata {
            kind: EventKind::Closed,
            state_available: true,
        },
        current,
    );
    current.active = false;
    current.view = VIEW_NONE;
    current.has_pending = false;
    event
}

fn apply_server(raw: &mut RawBulletin, body: &[u8]) -> Result<(), ()> {
    let mut reader = Reader::new(body);
    if reader.u8()? != 0x31 {
        return Err(());
    }
    match reader.u8()? {
        1 => apply_sections(raw, &mut reader),
        2 => apply_entries(raw, &mut reader, false),
        3 => apply_entry(raw, &mut reader, false),
        4 => apply_entries(raw, &mut reader, true),
        5 => apply_entry(raw, &mut reader, true),
        6 | 7 => apply_result(raw, &mut reader, true),
        8 => apply_result(raw, &mut reader, false),
        _ => Err(()),
    }
}

fn apply_sections(raw: &mut RawBulletin, reader: &mut Reader<'_>) -> Result<(), ()> {
    let heading = reader.string8()?;
    let count = usize::from(reader.u8()?);
    raw.view = VIEW_SECTIONS;
    raw.heading.set(heading);
    raw.section_count = count.min(MAX_BULLETIN_SECTIONS) as u8;
    raw.truncated = count > MAX_BULLETIN_SECTIONS;
    raw.has_selected_section = false;
    raw.viewport_position = 0;
    raw.viewport_maximum = 0;
    for section in &mut raw.sections {
        *section = RawBulletinSection::empty();
    }
    for index in 0..count {
        let id = reader.u16()?;
        let name = reader.string8()?;
        if let Some(section) = raw.sections.get_mut(index) {
            section.id = id;
            section.kind = if name.eq_ignore_ascii_case(b"Mail") {
                2
            } else {
                1
            };
            section.source = if section.kind == 2 { 3 } else { 1 };
            section.name.set(name);
        }
    }
    raw.has_pending = false;
    Ok(())
}

fn apply_entries(raw: &mut RawBulletin, reader: &mut Reader<'_>, mail: bool) -> Result<(), ()> {
    let source = reader.u8()?;
    let section_id = reader.u16()?;
    let heading = reader.string8()?;
    let count = usize::from(reader.u8()?);
    let same_section = raw.view == VIEW_ENTRIES && raw.section.id == section_id;
    if !same_section {
        raw.entry_count = 0;
        raw.truncated = false;
        for entry in &mut raw.entries {
            *entry = RawBulletinEntrySummary::empty();
        }
    }
    raw.view = VIEW_ENTRIES;
    raw.section.id = section_id;
    raw.section.kind = if mail { 2 } else { 1 };
    raw.section.source = source;
    raw.section.name.set(heading);
    raw.heading.set(heading);
    raw.has_selected_entry = false;
    for _ in 0..count {
        let flags = reader.u8()?;
        let id = reader.i16()?;
        let author = reader.string8()?;
        let month = reader.u8()?;
        let day = reader.u8()?;
        let subject = reader.string8()?;
        if raw.entries[..usize::from(raw.entry_count)]
            .iter()
            .any(|entry| entry.id == id)
        {
            continue;
        }
        let index = usize::from(raw.entry_count);
        let Some(entry) = raw.entries.get_mut(index) else {
            raw.truncated = true;
            continue;
        };
        entry.id = id;
        entry.flags = flags;
        entry.month = month;
        entry.day = day;
        entry.author.set(author);
        entry.subject.set(subject);
        raw.entry_count = raw.entry_count.saturating_add(1);
    }
    raw.pagination = pagination_raw(if count == 0 {
        BulletinPagination::Exhausted
    } else {
        BulletinPagination::Ready
    });
    raw.has_pending = false;
    Ok(())
}

fn apply_entry(raw: &mut RawBulletin, reader: &mut Reader<'_>, mail: bool) -> Result<(), ()> {
    let navigation_flags = reader.u8()?;
    let unknown_before_id = reader.u8()?;
    let id = reader.i16()?;
    let author = reader.string8()?;
    let month = reader.u8()?;
    let day = reader.u8()?;
    let subject = reader.string8()?;
    let body = reader.string16()?;
    let summary_flags = raw.entries[..usize::from(raw.entry_count)]
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.flags);
    raw.view = VIEW_ENTRY;
    raw.section.kind = if mail { 2 } else { 1 };
    raw.section.source = if mail { 3 } else { raw.section.source };
    raw.entry = RawBulletinEntry::empty();
    raw.entry.id = id;
    if let Some(flags) = summary_flags {
        raw.entry.flags = flags;
        raw.entry.has_flags = true;
    }
    raw.entry.navigation_flags = navigation_flags;
    raw.entry.unknown_before_id = unknown_before_id;
    raw.entry.author.set(author);
    raw.entry.month = month;
    raw.entry.day = day;
    raw.entry.subject.set(subject);
    if !raw.entry.body.set(body) && body.len() > MAX_BULLETIN_BODY_BYTES {
        return Err(());
    }
    raw.has_pending = false;
    Ok(())
}

fn apply_result(
    raw: &mut RawBulletin,
    reader: &mut Reader<'_>,
    has_message: bool,
) -> Result<(), ()> {
    let status = reader.u8()?;
    let message = has_message.then(|| reader.string8()).transpose()?;
    raw.has_result = true;
    raw.result_operation = if raw.has_pending { raw.pending } else { 0 };
    raw.result_status = status;
    raw.result_has_message = message.is_some();
    raw.result_message.set(message.unwrap_or_default());
    raw.has_pending = false;
    Ok(())
}

fn outgoing_operation(body: &[u8]) -> Option<BulletinOperation> {
    match body {
        [0x3B, 1] => Some(BulletinOperation::OpenSections),
        [0x43, 3, _, _, _, _, 1] => Some(BulletinOperation::OpenWorldBoard),
        [0x3B, 2, ..] => {
            let loading_older = with_current(|state| {
                state.view == VIEW_ENTRIES
                    && body.get(4..6).is_some_and(|cursor| {
                        state.oldest_entry_id().is_some_and(|oldest| {
                            i16::from_be_bytes([cursor[0], cursor[1]]) < oldest
                        })
                    })
            })
            .unwrap_or(false);
            Some(if loading_older {
                BulletinOperation::LoadOlder
            } else {
                BulletinOperation::OpenSection
            })
        }
        [0x3B, 3, .., 0xFF] => Some(BulletinOperation::PreviousEntry),
        [0x3B, 3, .., 1] => Some(BulletinOperation::NextEntry),
        [0x3B, 3, ..] => Some(BulletinOperation::OpenEntry),
        [0x3B, 4, ..] => Some(BulletinOperation::PostArticle),
        [0x3B, 5, ..] => Some(BulletinOperation::DeleteEntry),
        [0x3B, 6, ..] => Some(BulletinOperation::SendMail),
        [0x3B, 7, ..] => Some(BulletinOperation::HighlightArticle),
        _ => None,
    }
}

fn decode(raw: &RawBulletin) -> Result<BulletinState, ()> {
    let view = match raw.view {
        VIEW_SECTIONS => BulletinView::Sections {
            heading: raw.heading.decode().ok_or(())?,
            sections: raw.sections[..usize::from(raw.section_count)]
                .iter()
                .map(decode_section)
                .collect::<Result<_, _>>()?,
            selected_section_id: raw.has_selected_section.then_some(raw.selected_section_id),
            viewport: decode_viewport(raw),
            truncated: raw.truncated,
        },
        VIEW_ENTRIES => BulletinView::Entries {
            section: decode_section(&raw.section)?,
            entries: raw.entries[..usize::from(raw.entry_count)]
                .iter()
                .map(|entry| {
                    Ok(BulletinEntrySummary {
                        id: entry.id,
                        flags: entry.flags,
                        author: entry.author.decode().ok_or(())?,
                        month: entry.month,
                        day: entry.day,
                        subject: entry.subject.decode().ok_or(())?,
                    })
                })
                .collect::<Result<_, ()>>()?,
            selected_entry_id: raw.has_selected_entry.then_some(raw.selected_entry_id),
            viewport: decode_viewport(raw),
            pagination: pagination_from_raw(raw.pagination),
            truncated: raw.truncated,
        },
        VIEW_ENTRY => BulletinView::Entry {
            section: decode_section(&raw.section)?,
            entry: BulletinEntry {
                id: raw.entry.id,
                flags: raw.entry.has_flags.then_some(raw.entry.flags),
                author: raw.entry.author.decode().ok_or(())?,
                month: raw.entry.month,
                day: raw.entry.day,
                subject: raw.entry.subject.decode().ok_or(())?,
                body: raw.entry.body.decode().ok_or(())?,
                navigation_flags: raw.entry.navigation_flags,
                unknown_before_id: raw.entry.unknown_before_id,
            },
            viewport: decode_viewport(raw),
        },
        VIEW_BOARD_COMPOSE => BulletinView::Compose(BulletinCompose::BoardPost {
            section: decode_section(&raw.section)?,
            author: raw.entry.author.decode().ok_or(())?,
            subject: raw.compose_subject.decode().ok_or(())?,
            body: raw.compose_body.decode().ok_or(())?,
        }),
        VIEW_MAIL_COMPOSE => BulletinView::Compose(BulletinCompose::PlayerMail {
            mailbox: decode_section(&raw.section)?,
            recipient: raw.compose_recipient.decode().ok_or(())?,
            recipient_editable: raw.compose_recipient_editable,
            subject: raw.compose_subject.decode().ok_or(())?,
            body: raw.compose_body.decode().ok_or(())?,
        }),
        _ => return Err(()),
    };
    let result = raw.has_result.then(|| BulletinOperationResult {
        operation: operation_from_raw(raw.result_operation),
        raw_status: raw.result_status,
        message: raw
            .result_has_message
            .then(|| raw.result_message.decode())
            .flatten(),
    });
    Ok(BulletinState {
        revision: raw.revision,
        pending: raw.has_pending.then(|| operation_from_raw(raw.pending)),
        last_operation_result: result,
        can_go_back: raw.can_go_back,
        can_go_forward: raw.can_go_forward,
        view,
    })
}

fn decode_section(raw: &RawBulletinSection) -> Result<BulletinSection, ()> {
    Ok(BulletinSection {
        id: raw.id,
        name: raw.name.decode().ok_or(())?,
        kind: match raw.kind {
            1 => BulletinSectionKind::Board,
            2 => BulletinSectionKind::Mailbox,
            _ => BulletinSectionKind::Unknown,
        },
        source: BulletinSource::from_raw(raw.source),
    })
}

const fn decode_viewport(raw: &RawBulletin) -> BulletinViewport {
    BulletinViewport {
        position: raw.viewport_position,
        maximum: raw.viewport_maximum,
    }
}

const fn pagination_raw(value: BulletinPagination) -> u8 {
    match value {
        BulletinPagination::Unknown => 0,
        BulletinPagination::Ready => 1,
        BulletinPagination::Loading => 2,
        BulletinPagination::Exhausted => 3,
    }
}

const fn pagination_from_raw(value: u8) -> BulletinPagination {
    match value {
        1 => BulletinPagination::Ready,
        2 => BulletinPagination::Loading,
        3 => BulletinPagination::Exhausted,
        _ => BulletinPagination::Unknown,
    }
}

const fn operation_raw(value: BulletinOperation) -> u8 {
    match value {
        BulletinOperation::OpenSections => 1,
        BulletinOperation::OpenWorldBoard => 2,
        BulletinOperation::OpenSection => 3,
        BulletinOperation::LoadOlder => 4,
        BulletinOperation::OpenEntry => 5,
        BulletinOperation::PreviousEntry => 6,
        BulletinOperation::NextEntry => 7,
        BulletinOperation::PostArticle => 8,
        BulletinOperation::DeleteEntry => 9,
        BulletinOperation::SendMail => 10,
        BulletinOperation::HighlightArticle => 11,
        BulletinOperation::SelectSection => 12,
        BulletinOperation::SelectEntry => 13,
        BulletinOperation::Scroll => 14,
        BulletinOperation::Back => 15,
        BulletinOperation::Forward => 16,
        BulletinOperation::BeginBoardPost => 17,
        BulletinOperation::BeginPlayerMail => 18,
        BulletinOperation::BeginReply => 19,
        BulletinOperation::UpdateCompose => 20,
        BulletinOperation::Close => 21,
        BulletinOperation::Unknown => 0,
    }
}

const fn operation_from_raw(value: u8) -> BulletinOperation {
    match value {
        1 => BulletinOperation::OpenSections,
        2 => BulletinOperation::OpenWorldBoard,
        3 => BulletinOperation::OpenSection,
        4 => BulletinOperation::LoadOlder,
        5 => BulletinOperation::OpenEntry,
        6 => BulletinOperation::PreviousEntry,
        7 => BulletinOperation::NextEntry,
        8 => BulletinOperation::PostArticle,
        9 => BulletinOperation::DeleteEntry,
        10 => BulletinOperation::SendMail,
        11 => BulletinOperation::HighlightArticle,
        12 => BulletinOperation::SelectSection,
        13 => BulletinOperation::SelectEntry,
        14 => BulletinOperation::Scroll,
        15 => BulletinOperation::Back,
        16 => BulletinOperation::Forward,
        17 => BulletinOperation::BeginBoardPost,
        18 => BulletinOperation::BeginPlayerMail,
        19 => BulletinOperation::BeginReply,
        20 => BulletinOperation::UpdateCompose,
        21 => BulletinOperation::Close,
        _ => BulletinOperation::Unknown,
    }
}

fn current_mut() -> &'static mut RawBulletin {
    // SAFETY: all callers run on the client main thread.
    unsafe { &mut *CURRENT.0.get() }
}

fn scratch_mut() -> &'static mut RawBulletin {
    // SAFETY: SCRATCH is used only by the client main-thread packet observer.
    unsafe { &mut *SCRATCH.0.get() }
}

fn copy_raw(source: &RawBulletin, destination: &mut RawBulletin) {
    // SAFETY: callers provide distinct preallocated values, RawBulletin has no
    // pointers, and all copies occur on the client main thread.
    unsafe { std::ptr::copy_nonoverlapping(source, destination, 1) };
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u8(&mut self) -> Result<u8, ()> {
        let value = *self.bytes.get(self.offset).ok_or(())?;
        self.offset += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, ()> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16, ()> {
        let bytes = self.take(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn string8(&mut self) -> Result<&'a [u8], ()> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn string16(&mut self) -> Result<&'a [u8], ()> {
        let length = usize::from(self.u16()?);
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ()> {
        let end = self.offset.checked_add(length).ok_or(())?;
        let value = self.bytes.get(self.offset..end).ok_or(())?;
        self.offset = end;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn list_packet(last_id: i16) -> Vec<u8> {
        let mut body = vec![0x31, 2, 1, 0, 7, 5, b'B', b'o', b'a', b'r', b'd', 2];
        for id in [last_id + 1, last_id] {
            body.extend_from_slice(&[0, (id >> 8) as u8, id as u8, 3, b'B', b'o', b'b', 8, 29]);
            body.extend_from_slice(&[4, b'T', b'e', b's', b't']);
        }
        body
    }

    #[test]
    fn decodes_sections_lists_details_and_raw_results() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(observe_outgoing(&[0x3B, 1]).is_none());
        let sections = [
            0x31, 1, 0, 2, 0, 0, 4, b'M', b'a', b'i', b'l', 0, 7, 5, b'B', b'o', b'a', b'r', b'd',
        ];
        let opened = observe_server(&sections).unwrap();
        let BulletinUpdate::Opened(state) = take(opened).unwrap() else {
            panic!("expected open");
        };
        let BulletinView::Sections {
            sections: displayed_sections,
            ..
        } = state.view
        else {
            panic!("expected sections");
        };
        assert_eq!(displayed_sections[0].kind, BulletinSectionKind::Mailbox);

        let list = observe_server(&list_packet(10)).unwrap();
        let BulletinUpdate::Changed(state) = take(list).unwrap() else {
            panic!("expected list change");
        };
        let BulletinView::Entries { entries, .. } = state.view else {
            panic!("expected entries");
        };
        assert_eq!(
            entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            [11, 10]
        );

        let detail = [
            0x31, 3, 3, 9, 0, 10, 3, b'B', b'o', b'b', 8, 29, 4, b'T', b'e', b's', b't', 0, 4,
            b'B', b'o', b'd', b'y',
        ];
        let changed = observe_server(&detail).unwrap();
        let BulletinUpdate::Changed(state) = take(changed).unwrap() else {
            panic!("expected detail change");
        };
        let BulletinView::Entry { entry, .. } = state.view else {
            panic!("expected entry");
        };
        assert_eq!(entry.body, "Body");
        assert_eq!(entry.flags, Some(0));
        assert_eq!(entry.unknown_before_id, 9);

        let submitted = observe_outgoing(&[0x3B, 5, 0, 7, 0, 10]).unwrap();
        let BulletinUpdate::Changed(state) = take(submitted).unwrap() else {
            panic!("expected pending state change");
        };
        assert_eq!(state.pending, Some(BulletinOperation::DeleteEntry));
        let result = observe_server(&[0x31, 7, 4, 2, b'N', b'o']).unwrap();
        let BulletinUpdate::OperationResult { result, .. } = take(result).unwrap() else {
            panic!("expected result");
        };
        assert_eq!(result.operation, BulletinOperation::DeleteEntry);
        assert_eq!(result.raw_status, 4);
        assert_eq!(result.message.as_deref(), Some("No"));

        let closed = close().unwrap();
        assert!(matches!(take(closed), Some(BulletinUpdate::Closed { .. })));
        let reopened = observe_server(&sections).unwrap();
        let BulletinUpdate::Opened(state) = take(reopened).unwrap() else {
            panic!("expected clean reopen");
        };
        assert!(state.last_operation_result.is_none());
    }

    #[test]
    fn pagination_merges_unique_entries_and_stops_on_empty_page() {
        let _guard = LOCK.lock().unwrap();
        reset();
        release(observe_server(&list_packet(10)).unwrap());
        release(observe_server(&list_packet(9)).unwrap());
        let state = decode_current(&*current_mut()).unwrap();
        let BulletinView::Entries {
            entries,
            pagination,
            ..
        } = state.view
        else {
            panic!("expected entries");
        };
        assert_eq!(
            entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            [11, 10, 9]
        );
        assert_eq!(pagination, BulletinPagination::Ready);

        let empty = [0x31, 2, 1, 0, 7, 5, b'B', b'o', b'a', b'r', b'd', 0];
        release(observe_server(&empty).unwrap());
        let state = decode_current(&*current_mut()).unwrap();
        let BulletinView::Entries { pagination, .. } = state.view else {
            panic!("expected entries");
        };
        assert_eq!(pagination, BulletinPagination::Exhausted);
    }

    #[test]
    fn malformed_packets_do_not_replace_current_state() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(observe_server(&[0x31, 3, 0]).is_none());
        assert!(decode_current(&RawBulletin::empty()).is_none());
    }
}
