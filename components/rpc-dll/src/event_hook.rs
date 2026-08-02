use darpc_game_client::{EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA};
use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use darpc_win32::pipe::sender_tick_ms;
use std::{
    io, panic,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory, LibraryLoader::GetModuleHandleW,
    Threading::GetCurrentProcess,
};

use crate::{packet, state_events};

pub(crate) const NAME: &str = "event_dispatch";

const DETOUR_RANGE_LEN: usize = 128;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const SERVER_EVENT_TYPE: u8 = 0x13;
const EVENT_TYPE_OFFSET: usize = 0x0C;
const EVENT_BODY_OFFSET: usize = 0x14;
const EVENT_BODY_LENGTH_OFFSET: usize = 0x18;
const EVENT_VIEW_LENGTH: usize = 0x1C - EVENT_TYPE_OFFSET;
const MAX_OBSERVED_BODY_LENGTH: usize = 128;

#[unsafe(no_mangle)]
static EVENT_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static EVENT_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static EVENT_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static EVENT_RELOCATED_BYTES: AtomicU32 = AtomicU32::new(0);
static EVENT_OBSERVATION_COUNT: AtomicU32 = AtomicU32::new(0);
static SERVER_EVENT_COUNT: AtomicU32 = AtomicU32::new(0);
static EVENT_COUNT: AtomicU32 = AtomicU32::new(0);
static EVENT_PARSE_ERROR_COUNT: AtomicU32 = AtomicU32::new(0);
static EVENT_READ_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);
static EVENT_INVALID_BODY_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_OPCODE: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_FIELDS: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_BODY_LENGTH: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_OFFSET: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_NEEDED: AtomicU32 = AtomicU32::new(0);
static LAST_PARSE_REMAINING: AtomicU32 = AtomicU32::new(0);

pub(crate) struct EventHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EventHealth {
    pub(crate) installed: bool,
    pub(crate) relocated_bytes: u8,
    pub(crate) observation_count: u32,
    pub(crate) server_event_count: u32,
    pub(crate) event_count: u32,
    pub(crate) parse_error_count: u32,
    pub(crate) read_failure_count: u32,
    pub(crate) invalid_body_count: u32,
    pub(crate) last_parse_opcode: u8,
    pub(crate) last_parse_fields: u8,
    pub(crate) last_parse_body_length: u32,
    pub(crate) last_parse_offset: u32,
    pub(crate) last_parse_needed: u32,
    pub(crate) last_parse_remaining: u32,
}

impl EventHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = target_address()?;
        validate_entry(target)?;
        let detour = NonNull::new(event_dispatch_detour as *mut u8)
            .ok_or_else(|| io::Error::other("event detour address is null"))?;
        let detour_range = CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)
            .map_err(InstallError::from)?;
        let spec = DetourSpec::new(target, detour, detour_range, &EVENT_HOOK_ACTIVITY)
            .map_err(InstallError::from)?;

        // SAFETY: the supported executable fingerprint and exact target entry
        // bytes were validated. The detour preserves the target's thiscall ABI,
        // invokes the original first, and observes only bounded copied bytes.
        let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
        let relocated_bytes = u8::try_from(prepared.relocated_len())
            .map_err(|_| io::Error::other("relocated event prologue exceeds u8"))?;
        EVENT_OBSERVATION_COUNT.store(0, Ordering::Release);
        SERVER_EVENT_COUNT.store(0, Ordering::Release);
        EVENT_COUNT.store(0, Ordering::Release);
        EVENT_PARSE_ERROR_COUNT.store(0, Ordering::Release);
        EVENT_READ_FAILURE_COUNT.store(0, Ordering::Release);
        EVENT_INVALID_BODY_COUNT.store(0, Ordering::Release);
        LAST_PARSE_OPCODE.store(0, Ordering::Release);
        LAST_PARSE_FIELDS.store(0, Ordering::Release);
        LAST_PARSE_BODY_LENGTH.store(0, Ordering::Release);
        LAST_PARSE_OFFSET.store(0, Ordering::Release);
        LAST_PARSE_NEEDED.store(0, Ordering::Release);
        LAST_PARSE_REMAINING.store(0, Ordering::Release);
        EVENT_RELOCATED_BYTES.store(u32::from(relocated_bytes), Ordering::Release);
        EVENT_TRAMPOLINE.store(
            prepared.trampoline_address().map_err(InstallError::from)?,
            Ordering::Release,
        );

        let deadline = Instant::now() + INSTALL_TIMEOUT;
        let mut detour = loop {
            match prepared.install() {
                Ok(detour) => break detour,
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::sleep(COMMIT_RETRY_INTERVAL);
                }
                Err(error) => {
                    EVENT_TRAMPOLINE.store(0, Ordering::Release);
                    EVENT_RELOCATED_BYTES.store(0, Ordering::Release);
                    return Err(InstallError::from(error));
                }
            }
        };
        let install_warning = detour.take_resume_warning().map(detour_error);
        EVENT_HOOK_INSTALLED.store(true, Ordering::Release);
        Ok(Self {
            detour,
            relocated_bytes,
            install_warning,
        })
    }

    pub(crate) const fn relocated_bytes(&self) -> u8 {
        self.relocated_bytes
    }

    pub(crate) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(crate) fn uninstall(&mut self) -> io::Result<bool> {
        let deadline = Instant::now() + UNINSTALL_TIMEOUT;
        loop {
            match self.detour.uninstall() {
                Ok(changed) => {
                    EVENT_HOOK_INSTALLED.store(false, Ordering::Release);
                    EVENT_TRAMPOLINE.store(0, Ordering::Release);
                    EVENT_RELOCATED_BYTES.store(0, Ordering::Release);
                    return Ok(changed);
                }
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::sleep(COMMIT_RETRY_INTERVAL);
                }
                Err(error) => return Err(detour_error(error)),
            }
        }
    }
}

pub(crate) fn health() -> EventHealth {
    EventHealth {
        installed: EVENT_HOOK_INSTALLED.load(Ordering::Acquire),
        relocated_bytes: EVENT_RELOCATED_BYTES.load(Ordering::Acquire) as u8,
        observation_count: EVENT_OBSERVATION_COUNT.load(Ordering::Acquire),
        server_event_count: SERVER_EVENT_COUNT.load(Ordering::Acquire),
        event_count: EVENT_COUNT.load(Ordering::Acquire),
        parse_error_count: EVENT_PARSE_ERROR_COUNT.load(Ordering::Acquire),
        read_failure_count: EVENT_READ_FAILURE_COUNT.load(Ordering::Acquire),
        invalid_body_count: EVENT_INVALID_BODY_COUNT.load(Ordering::Acquire),
        last_parse_opcode: LAST_PARSE_OPCODE.load(Ordering::Acquire) as u8,
        last_parse_fields: LAST_PARSE_FIELDS.load(Ordering::Acquire) as u8,
        last_parse_body_length: LAST_PARSE_BODY_LENGTH.load(Ordering::Acquire),
        last_parse_offset: LAST_PARSE_OFFSET.load(Ordering::Acquire),
        last_parse_needed: LAST_PARSE_NEEDED.load(Ordering::Acquire),
        last_parse_remaining: LAST_PARSE_REMAINING.load(Ordering::Acquire),
    }
}

fn target_address() -> io::Result<NonNull<u8>> {
    // SAFETY: a null module name requests the executable module for the current
    // process and has no lifetime transfer.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(EVENT_DISPATCH_RVA)
        .ok_or_else(|| io::Error::other("event target address overflow"))?;
    NonNull::new(address as *mut u8).ok_or_else(|| io::Error::other("event target address is null"))
}

fn validate_entry(target: NonNull<u8>) -> io::Result<()> {
    // SAFETY: the supported executable fingerprint establishes that the target
    // RVA names readable executable memory spanning this fixed entry contract.
    let actual = unsafe { slice::from_raw_parts(target.as_ptr(), EVENT_DISPATCH_ENTRY.len()) };
    if actual != EVENT_DISPATCH_ENTRY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{NAME} entry mismatch at RVA 0x{EVENT_DISPATCH_RVA:08X}: expected={:02X?} actual={actual:02X?}",
                EVENT_DISPATCH_ENTRY
            ),
        ));
    }
    Ok(())
}

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn event_dispatch_detour(
    _dispatcher: *mut core::ffi::c_void,
    _event: *const core::ffi::c_void,
) -> bool {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "push dword ptr [esp + 8]",
        "call dword ptr [{trampoline}]",
        "mov esi, eax",
        "push dword ptr [esp + 8]",
        "call {observe}",
        "add esp, 4",
        "mov eax, esi",
        "pop esi",
        "lock dec dword ptr [{activity}]",
        "ret 4",
        activity = sym EVENT_HOOK_ACTIVITY,
        trampoline = sym EVENT_TRAMPOLINE,
        observe = sym observe_event,
    );
}

extern "C" fn observe_event(event: *const core::ffi::c_void) {
    let _ = panic::catch_unwind(|| {
        if event.is_null() {
            return;
        }
        EVENT_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
        let mut view = [0_u8; EVENT_VIEW_LENGTH];
        let Some(address) = (event as usize).checked_add(EVENT_TYPE_OFFSET) else {
            return;
        };
        if !read_memory(address, &mut view) {
            EVENT_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if view[0] != SERVER_EVENT_TYPE {
            return;
        }
        SERVER_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
        let body_address = u32::from_le_bytes(
            view[EVENT_BODY_OFFSET - EVENT_TYPE_OFFSET..EVENT_BODY_OFFSET - EVENT_TYPE_OFFSET + 4]
                .try_into()
                .expect("event body pointer is four bytes"),
        );
        let body_length = u32::from_le_bytes(
            view[EVENT_BODY_LENGTH_OFFSET - EVENT_TYPE_OFFSET
                ..EVENT_BODY_LENGTH_OFFSET - EVENT_TYPE_OFFSET + 4]
                .try_into()
                .expect("event body length is four bytes"),
        ) as usize;
        if body_address == 0 || body_length == 0 || body_length > MAX_OBSERVED_BODY_LENGTH {
            EVENT_INVALID_BODY_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut body = [0_u8; MAX_OBSERVED_BODY_LENGTH];
        if !read_memory(body_address as usize, &mut body[..body_length]) {
            EVENT_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let update = match packet::update(&body[..body_length]) {
            Ok(Some(update)) => update,
            Ok(None) => return,
            Err(error) => {
                EVENT_PARSE_ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
                LAST_PARSE_OPCODE.store(u32::from(body[0]), Ordering::Relaxed);
                LAST_PARSE_FIELDS.store(
                    u32::from(body.get(1).copied().unwrap_or_default()),
                    Ordering::Relaxed,
                );
                LAST_PARSE_BODY_LENGTH.store(body_length as u32, Ordering::Relaxed);
                LAST_PARSE_OFFSET.store(error.offset() as u32, Ordering::Relaxed);
                LAST_PARSE_NEEDED.store(error.needed() as u32, Ordering::Relaxed);
                LAST_PARSE_REMAINING.store(error.remaining() as u32, Ordering::Relaxed);
                return;
            }
        };
        EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
        let tick_ms = sender_tick_ms();
        match update {
            packet::ServerUpdate::Status(update) => {
                state_events::observe_status(update, tick_ms);
            }
            packet::ServerUpdate::UserPosition(position) => {
                state_events::observe_user_position(position.x, position.y, tick_ms);
            }
            packet::ServerUpdate::Move(position) => {
                state_events::observe_move(position.x, position.y, tick_ms);
            }
            packet::ServerUpdate::Effect(effect) => {
                state_events::observe_effect(effect.icon, effect.duration, tick_ms);
            }
        }
    });
}

fn read_memory(address: usize, output: &mut [u8]) -> bool {
    let mut read = 0_usize;
    // SAFETY: output is valid for its length. ReadProcessMemory validates the
    // current-process source range and reports failure without dereferencing it
    // through a Rust reference.
    let succeeded = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const core::ffi::c_void,
            output.as_mut_ptr().cast(),
            output.len(),
            &mut read,
        )
    };
    succeeded != 0 && read == output.len()
}

#[cfg(test)]
mod tests {
    use super::validate_entry;
    use darpc_game_client::EVENT_DISPATCH_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_event_entry() {
        let mut entry = EVENT_DISPATCH_ENTRY;
        let target = NonNull::new(entry.as_mut_ptr()).unwrap();
        validate_entry(target).unwrap();

        let mut wrong_entry = EVENT_DISPATCH_ENTRY;
        wrong_entry[0] ^= 0xFF;
        let wrong_target = NonNull::new(wrong_entry.as_mut_ptr()).unwrap();
        assert_eq!(
            validate_entry(wrong_target).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
