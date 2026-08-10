mod convert;
mod publication;

#[cfg(not(test))]
use darpc_game_client::RawLifecycle;
use darpc_game_client::{
    MemoryReader, RawGroupState, RawInventory, RawObjects, RawSkillbook, RawSpellbook,
    RawStateSnapshot, StateReadError, StateWalker,
};
use darpc_model::ClientSnapshot;
use std::{
    fmt, ptr,
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    LibraryLoader::GetModuleHandleW,
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

pub(crate) use self::publication::CaptureFailure;
use crate::map_name;

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(1);

static REQUEST_GENERATION: AtomicU32 = AtomicU32::new(0);
static PROCESSED_GENERATION: AtomicU32 = AtomicU32::new(0);

pub(crate) fn reset() {
    REQUEST_GENERATION.store(0, Ordering::Release);
    PROCESSED_GENERATION.store(0, Ordering::Release);
    publication::reset();
    crate::state::reset();
    crate::dialog::reset();
    crate::group::reset();
    crate::exchange::reset();
    #[cfg(all(windows, not(test)))]
    crate::actions::group::reset();
    map_name::reset();
}

#[must_use]
pub(crate) fn request() -> u32 {
    let previous = REQUEST_GENERATION
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            let next = value.wrapping_add(1);
            Some(if next == 0 { 1 } else { next })
        })
        .expect("snapshot request update cannot fail");
    let next = previous.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

pub(crate) fn wait(
    request_generation: u32,
    timeout: Duration,
) -> Result<ClientSnapshot, WaitError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(publication) = publication::read()
            && publication.request_generation == request_generation
        {
            return publication.result.map_err(WaitError::Capture);
        }
        if Instant::now() >= deadline {
            return Err(WaitError::TimedOut);
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
}

pub(crate) fn observe_tick() {
    let request_generation = REQUEST_GENERATION.load(Ordering::Acquire);
    if request_generation == PROCESSED_GENERATION.load(Ordering::Acquire) {
        return;
    }
    if crate::state::map_transition_pending() {
        return;
    }

    let Some(mut publication) = publication::begin() else {
        return;
    };
    let started = Instant::now();
    let capture = {
        let (raw, objects) = publication.buffers();
        capture(raw, objects)
    };
    let duration_us = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
    match capture {
        Ok(()) => publication.publish_ready(request_generation, duration_us),
        Err(error) => {
            publication.publish_failed(request_generation, CaptureFailure::from(error));
        }
    }
    PROCESSED_GENERATION.store(request_generation, Ordering::Release);
}

fn capture(raw: &mut RawStateSnapshot, objects: &mut RawObjects) -> Result<(), StateReadError> {
    let (walker, thread_id) = process_walker()?;
    walker.capture_into(thread_id, raw)?;
    crate::group::merge_snapshot(&mut raw.group, raw.group_available);
    let center = raw
        .character_available
        .then_some(&raw.character)
        .and_then(|character| character.location)
        .and_then(|location| location.x.zip(location.y));
    walker.capture_objects(thread_id, center, objects)?;
    if raw.character_available
        && let Some(id) = raw.character.id
    {
        objects.name_player(
            id,
            &raw.character.name[..usize::from(raw.character.name_len)],
        );
    }
    Ok(())
}

pub(crate) fn capture_group(output: &mut RawGroupState) -> Result<(), StateReadError> {
    let (walker, thread_id) = process_walker()?;
    walker.capture_group_state(thread_id, output)
}

pub(crate) fn capture_inventory(output: &mut RawInventory) -> Result<bool, StateReadError> {
    let (walker, thread_id) = process_walker()?;
    walker.capture_inventory_state(thread_id, output)
}

pub(crate) fn capture_abilities(
    skillbook: &mut RawSkillbook,
    spellbook: &mut RawSpellbook,
) -> Result<(bool, bool), StateReadError> {
    let (walker, thread_id) = process_walker()?;
    walker.capture_ability_state(thread_id, skillbook, spellbook)
}

#[cfg(not(test))]
pub(crate) fn capture_lifecycle() -> Result<RawLifecycle, StateReadError> {
    let (walker, thread_id) = process_walker()?;
    walker.capture_lifecycle(thread_id)
}

fn process_walker() -> Result<(StateWalker<'static, ProcessMemory>, u32), StateReadError> {
    // SAFETY: a null module name requests the executable module for the current
    // process and has no lifetime transfer.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module_base =
        u32::try_from(module as usize).map_err(|_| StateReadError::AddressOverflow)?;
    // SAFETY: this function has no preconditions and returns the current thread ID.
    let thread_id = unsafe { GetCurrentThreadId() };
    static MEMORY: ProcessMemory = ProcessMemory;
    Ok((StateWalker::new(&MEMORY, module_base), thread_id))
}

struct ProcessMemory;

impl MemoryReader for ProcessMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> bool {
        let mut read = 0_usize;
        // SAFETY: the destination is valid for `output.len()` bytes. The source
        // belongs to the current process; ReadProcessMemory validates it and
        // reports failure instead of dereferencing an unreadable pointer here.
        let succeeded = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as usize as *const core::ffi::c_void,
                output.as_mut_ptr().cast(),
                output.len(),
                &mut read,
            )
        };
        succeeded != 0 && read == output.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitError {
    TimedOut,
    Capture(CaptureFailure),
}

impl fmt::Display for WaitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => {
                formatter.write_str("client did not capture a snapshot before timeout")
            }
            Self::Capture(error) => write!(formatter, "snapshot capture failed: {error}"),
        }
    }
}
