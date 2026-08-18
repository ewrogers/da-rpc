#![cfg_attr(test, allow(dead_code))]

#[cfg(windows)]
use crate::atomic_sequence::next_nonzero;
use crate::{client_text, transfer_slot::TransferSlot};
use darpc_model::{MessageDialog, MessageDialogsState};
use darpc_protocol::{CommandFailure, MessageDialogCommand};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

pub(crate) const MAX_DIALOGS: usize = darpc_protocol::MAX_MESSAGE_DIALOGS;
const MAX_TEXT_BYTES: usize = darpc_protocol::MAX_MESSAGE_DIALOG_TEXT_LEN;
const EVENT_SLOTS: usize = 4;
const OBSERVATION_INTERVAL_MS: u32 = 100;

#[cfg(windows)]
const ENTRIES_OFFSET: usize = 0x64;
#[cfg(windows)]
const COUNT_OFFSET: usize = 0x68;
#[cfg(windows)]
const CAPACITY_OFFSET: usize = 0x6C;
#[cfg(windows)]
const ENTRY_SIZE: usize = 0x0C;
#[cfg(windows)]
const VISIBLE_OFFSET: usize = 0x130;
#[cfg(windows)]
const REGISTRATION_OFFSET: usize = 0x188;
#[cfg(windows)]
const MESSAGE_DIALOG_VTABLE_RVA: usize = 0x0027_2A84;
#[cfg(windows)]
const CLOSE_RVA: usize = 0x0004_8E10;
#[cfg(windows)]
const MAX_PANES: i32 = 1_024;

static CURRENT: Current = Current(UnsafeCell::new(CurrentState::empty()));
static EVENTS: Events = Events::new();
static REVISION: AtomicU32 = AtomicU32::new(0);
static NEXT_ID: AtomicU32 = AtomicU32::new(0);
static OBSERVATION_SCHEDULED: AtomicBool = AtomicBool::new(false);
static NEXT_OBSERVATION_TICK_MS: AtomicU32 = AtomicU32::new(0);

struct Current(UnsafeCell<CurrentState>);

// SAFETY: the client main thread exclusively reads and writes CURRENT. Copies
// cross to the IPC thread through snapshot publication or transfer slots.
unsafe impl Sync for Current {}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct RawMessageDialogs {
    revision: u32,
    count: u8,
    dialogs: [RawMessageDialog; MAX_DIALOGS],
}

impl RawMessageDialogs {
    pub(crate) const fn empty() -> Self {
        Self {
            revision: 0,
            count: 0,
            dialogs: [RawMessageDialog::empty(); MAX_DIALOGS],
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawMessageDialog {
    id: u32,
    length: u16,
    text_available: bool,
    truncated: bool,
    bytes: [u8; MAX_TEXT_BYTES],
}

impl RawMessageDialog {
    const fn empty() -> Self {
        Self {
            id: 0,
            length: 0,
            text_available: false,
            truncated: false,
            bytes: [0; MAX_TEXT_BYTES],
        }
    }
}

#[derive(Clone, Copy)]
struct CurrentState {
    raw: RawMessageDialogs,
    panes: [u32; MAX_DIALOGS],
}

impl CurrentState {
    const fn empty() -> Self {
        Self {
            raw: RawMessageDialogs::empty(),
            panes: [0; MAX_DIALOGS],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedMessageDialogs(u8);

struct Events {
    slots: [TransferSlot<RawMessageDialogs>; EVENT_SLOTS],
}

impl Events {
    const fn new() -> Self {
        Self {
            slots: [const { TransferSlot::new() }; EVENT_SLOTS],
        }
    }

    fn push(&self, state: RawMessageDialogs) -> Option<QueuedMessageDialogs> {
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.try_write(state) {
                return Some(QueuedMessageDialogs(index as u8));
            }
        }
        None
    }
}

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe { *CURRENT.0.get() = CurrentState::empty() };
    for slot in &EVENTS.slots {
        slot.reset();
    }
    REVISION.store(0, Ordering::Release);
    NEXT_ID.store(0, Ordering::Release);
    OBSERVATION_SCHEDULED.store(false, Ordering::Relaxed);
    NEXT_OBSERVATION_TICK_MS.store(0, Ordering::Relaxed);
}

pub(crate) fn observe_server(body: &[u8]) -> Option<QueuedMessageDialogs> {
    if body.len() < 2 || body[0] != 0x0A || !matches!(body[1], 0x08..=0x0A) {
        return None;
    }
    refresh()
}

pub(crate) fn synchronize(tick_ms: u32) {
    if let Some(update) = refresh() {
        crate::state::observe_message_dialogs(update, tick_ms);
    }
}

pub(crate) fn observe_tick(tick_ms: u32) -> Option<QueuedMessageDialogs> {
    if current().raw.count == 0 {
        OBSERVATION_SCHEDULED.store(false, Ordering::Relaxed);
        return None;
    }
    if OBSERVATION_SCHEDULED.load(Ordering::Relaxed)
        && !crate::wrapping_time::deadline_reached(
            tick_ms,
            NEXT_OBSERVATION_TICK_MS.load(Ordering::Relaxed),
        )
    {
        return None;
    }
    OBSERVATION_SCHEDULED.store(true, Ordering::Relaxed);
    NEXT_OBSERVATION_TICK_MS.store(
        tick_ms.wrapping_add(OBSERVATION_INTERVAL_MS),
        Ordering::Relaxed,
    );
    refresh()
}

pub(crate) fn dismiss(
    command: MessageDialogCommand,
) -> Result<Option<QueuedMessageDialogs>, CommandFailure> {
    if current().raw.revision != command.revision {
        return Err(CommandFailure::InvalidState);
    }
    let index = current().raw.dialogs[..usize::from(current().raw.count)]
        .iter()
        .position(|dialog| dialog.id == command.id)
        .ok_or(CommandFailure::InvalidArguments)?;
    let pane = current().panes[index];
    #[cfg(windows)]
    {
        let module_base = module_base().ok_or(CommandFailure::InvalidState)?;
        if !is_message_dialog(module_base, pane as usize) {
            return Err(CommandFailure::InvalidState);
        }
        type CloseFn = unsafe extern "thiscall" fn(*mut core::ffi::c_void);
        // SAFETY: the pane was revalidated as a live WindowMessageDialogPane,
        // and this RVA and ABI belong to the supported client executable.
        unsafe {
            let close: CloseFn = std::mem::transmute(module_base + CLOSE_RVA);
            close(pane as *mut core::ffi::c_void);
        }
        Ok(refresh())
    }
    #[cfg(not(windows))]
    {
        let _ = pane;
        Err(CommandFailure::InvalidState)
    }
}

pub(crate) fn take(queued: QueuedMessageDialogs) -> Option<MessageDialogsState> {
    decode(EVENTS.slots.get(usize::from(queued.0))?.try_take()?)
}

pub(crate) fn release(queued: QueuedMessageDialogs) {
    if let Some(slot) = EVENTS.slots.get(usize::from(queued.0)) {
        slot.discard();
    }
}

pub(crate) unsafe fn copy_current(output: &mut RawMessageDialogs) {
    // SAFETY: caller guarantees main-thread ownership and exclusive output.
    *output = unsafe { (*CURRENT.0.get()).raw };
}

pub(crate) fn decode_current(raw: &RawMessageDialogs) -> MessageDialogsState {
    decode(*raw).unwrap_or_default()
}

fn decode(raw: RawMessageDialogs) -> Option<MessageDialogsState> {
    let mut dialogs = Vec::with_capacity(usize::from(raw.count));
    for raw_dialog in &raw.dialogs[..usize::from(raw.count)] {
        let text = raw_dialog
            .text_available
            .then(|| {
                client_text::decode_or_empty(&raw_dialog.bytes[..usize::from(raw_dialog.length)])
            })
            .flatten();
        dialogs.push(MessageDialog {
            id: raw_dialog.id,
            text,
            truncated: raw_dialog.truncated,
        });
    }
    Some(MessageDialogsState {
        revision: raw.revision,
        dialogs,
    })
}

fn current() -> &'static mut CurrentState {
    // SAFETY: all callers run on the client main thread.
    unsafe { &mut *CURRENT.0.get() }
}

#[cfg(windows)]
fn refresh() -> Option<QueuedMessageDialogs> {
    let module_base = module_base()?;
    let (panes, count) = scan_panes(module_base)?;
    let previous = *current();
    let mut next = CurrentState::empty();
    for (index, pane) in panes.into_iter().take(count).enumerate() {
        next.panes[index] = pane;
        next.raw.dialogs[index] = read_dialog(
            pane as usize,
            previous
                .panes
                .iter()
                .position(|old| *old == pane)
                .map(|old| previous.raw.dialogs[old].id)
                .unwrap_or_else(|| next_nonzero(&NEXT_ID)),
        );
        next.raw.count += 1;
    }
    if next.raw.count == previous.raw.count
        && next.raw.dialogs[..usize::from(next.raw.count)]
            == previous.raw.dialogs[..usize::from(previous.raw.count)]
    {
        return None;
    }
    next.raw.revision = next_nonzero(&REVISION);
    *current() = next;
    EVENTS.push(next.raw)
}

#[cfg(not(windows))]
fn refresh() -> Option<QueuedMessageDialogs> {
    None
}

#[cfg(windows)]
fn module_base() -> Option<usize> {
    use std::ptr;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: null requests the current executable module without ownership.
    let base = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    (base != 0).then_some(base)
}

#[cfg(windows)]
fn scan_panes(module_base: usize) -> Option<([u32; MAX_DIALOGS], usize)> {
    use darpc_game_client::EVENT_DISPATCHER_POINTER_RVA;
    let dispatcher = crate::process_memory::read::<u32>(module_base + EVENT_DISPATCHER_POINTER_RVA)
        .filter(|value| *value != 0)? as usize;
    let entries = crate::process_memory::read::<u32>(dispatcher + ENTRIES_OFFSET)? as usize;
    let count = crate::process_memory::read::<i32>(dispatcher + COUNT_OFFSET)?;
    let capacity = crate::process_memory::read::<i32>(dispatcher + CAPACITY_OFFSET)?;
    if count < 0 || count > capacity || capacity > MAX_PANES || (count != 0 && entries == 0) {
        return None;
    }
    let mut panes = [0_u32; MAX_DIALOGS];
    let mut pane_count = 0;
    for index in 0..count as usize {
        let Some(pane) = crate::process_memory::read::<u32>(entries + index * ENTRY_SIZE)
            .filter(|value| *value != 0)
        else {
            continue;
        };
        if is_message_dialog(module_base, pane as usize) {
            panes[pane_count] = pane;
            pane_count += 1;
            if pane_count == MAX_DIALOGS {
                break;
            }
        }
    }
    Some((panes, pane_count))
}

#[cfg(windows)]
fn is_message_dialog(module_base: usize, pane: usize) -> bool {
    crate::process_memory::read::<u32>(pane)
        == Some((module_base + MESSAGE_DIALOG_VTABLE_RVA) as u32)
        && crate::process_memory::read::<u8>(pane + VISIBLE_OFFSET) == Some(1)
        && crate::process_memory::read::<u8>(pane + REGISTRATION_OFFSET).unwrap_or(0) & 0x02 != 0
}

#[cfg(windows)]
fn read_dialog(pane: usize, id: u32) -> RawMessageDialog {
    let mut raw = RawMessageDialog::empty();
    raw.id = id;
    let Some(list) = crate::process_memory::read::<u32>(pane + 0x594) else {
        return raw;
    };
    let Some(controls) = crate::process_memory::read::<u32>(list as usize + 0x18) else {
        return raw;
    };
    let Some(control) = crate::process_memory::read::<u32>(controls as usize + 4) else {
        return raw;
    };
    let Some(editor) = crate::process_memory::read::<u32>(control as usize + 0x19C) else {
        return raw;
    };
    let Some(bytes) = crate::process_memory::read::<u32>(editor as usize + 0x1BC) else {
        return raw;
    };
    let Some(length) = crate::process_memory::read::<u32>(bytes as usize + 0x14) else {
        return raw;
    };
    let Some(data) = crate::process_memory::read::<u32>(bytes as usize + 0x18) else {
        return raw;
    };
    let copied = usize::try_from(length)
        .unwrap_or(usize::MAX)
        .min(MAX_TEXT_BYTES);
    if (copied != 0 && data == 0)
        || !crate::process_memory::read_exact(data as usize, &mut raw.bytes[..copied])
    {
        return raw;
    }
    raw.length = copied as u16;
    raw.text_available = true;
    raw.truncated = usize::try_from(length).unwrap_or(usize::MAX) > MAX_TEXT_BYTES;
    raw
}
