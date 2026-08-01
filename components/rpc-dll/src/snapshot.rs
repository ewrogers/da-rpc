mod publication;

use darpc_game_client::{MemoryReader, RawStateSnapshot, StateReadError, StateWalker};
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

    let started = Instant::now();
    let capture = capture();
    let duration_us = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
    match capture {
        Ok(raw) => publication::publish_ready(request_generation, duration_us, raw),
        Err(error) => {
            publication::publish_failed(request_generation, CaptureFailure::from(error));
        }
    }
    PROCESSED_GENERATION.store(request_generation, Ordering::Release);
}

fn capture() -> Result<RawStateSnapshot, StateReadError> {
    // SAFETY: a null module name requests the executable module for the current
    // process and has no lifetime transfer.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module_base =
        u32::try_from(module as usize).map_err(|_| StateReadError::AddressOverflow)?;
    // SAFETY: this function has no preconditions and returns the current thread ID.
    let thread_id = unsafe { GetCurrentThreadId() };
    StateWalker::new(&ProcessMemory, module_base).capture(thread_id)
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
