use darpc_game_client::{EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use darpc_protocol::HookTimingStage;
use darpc_win32::pipe::sender_tick_ms;
use std::{
    cell::UnsafeCell,
    io, panic,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};

use super::support;
use crate::{
    diagnostics,
    process_memory::read_exact,
    server_event::{Observation, ServerEventProcessor},
};

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
const MAX_OBSERVED_BODY_LENGTH: usize = u16::MAX as usize;

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
static EVENT_SCRATCH_IN_USE: AtomicBool = AtomicBool::new(false);
static EVENT_SCRATCH: EventScratchCell = EventScratchCell(UnsafeCell::new(EventScratch::new()));

struct EventScratch {
    body: [u8; MAX_OBSERVED_BODY_LENGTH],
    server_events: ServerEventProcessor,
}

impl EventScratch {
    const fn new() -> Self {
        Self {
            body: [0; MAX_OBSERVED_BODY_LENGTH],
            server_events: ServerEventProcessor::new(),
        }
    }
}

struct EventScratchCell(UnsafeCell<EventScratch>);

// SAFETY: every access is guarded by EVENT_SCRATCH_IN_USE. A nested or
// concurrent hook observation is skipped instead of aliasing the scratch data.
unsafe impl Sync for EventScratchCell {}

struct EventScratchGuard;

impl Drop for EventScratchGuard {
    fn drop(&mut self) {
        EVENT_SCRATCH_IN_USE.store(false, Ordering::Release);
    }
}

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
        let target =
            support::target_address(support::module_base()?, EVENT_DISPATCH_RVA, "event target")?;
        support::validate_bytes(target, &EVENT_DISPATCH_ENTRY, "event dispatch entry")?;

        // SAFETY: the supported executable fingerprint and exact target entry
        // bytes were validated. The detour preserves the target's thiscall ABI,
        // suppresses only a correlated daRPC Who response, and otherwise calls
        // the original before observing bounded copied bytes.
        let mut prepared = unsafe {
            support::prepare_detour(
                target,
                event_dispatch_detour as *mut u8,
                DETOUR_RANGE_LEN,
                &EVENT_HOOK_ACTIVITY,
                "event detour",
            )
        }?;
        let relocated_bytes = support::relocated_bytes(&prepared, "event prologue")?;
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

        let mut detour = match support::install_prepared(
            &mut prepared,
            INSTALL_TIMEOUT,
            COMMIT_RETRY_INTERVAL,
        ) {
            Ok(detour) => detour,
            Err(error) => {
                EVENT_TRAMPOLINE.store(0, Ordering::Release);
                EVENT_RELOCATED_BYTES.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        };
        let install_warning = detour.take_resume_warning().map(support::detour_error);
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
        let changed =
            support::uninstall_detour(&mut self.detour, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
                .map_err(support::detour_error)?;
        EVENT_HOOK_INSTALLED.store(false, Ordering::Release);
        EVENT_TRAMPOLINE.store(0, Ordering::Release);
        EVENT_RELOCATED_BYTES.store(0, Ordering::Release);
        Ok(changed)
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

#[unsafe(naked)]
unsafe extern "thiscall" fn event_dispatch_detour(
    _dispatcher: *mut core::ffi::c_void,
    _event: *const core::ffi::c_void,
) -> bool {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "cmp dword ptr [{intercept_command_id}], 0",
        "jne 4f",
        "cmp byte ptr [{exchange_intercept_pending}], 0",
        "jne 4f",
        "cmp byte ptr [{player_intercept_pending}], 0",
        "jne 4f",
        "cmp dword ptr [{look_intercept_command_id}], 0",
        "je 3f",
        "4:",
        "push eax",
        "push edx",
        "mov eax, dword ptr [esp + 16]",
        "test eax, eax",
        "jz 1f",
        "cmp byte ptr [eax + {event_type_offset}], {server_event_type}",
        "jne 1f",
        "mov edx, dword ptr [eax + {event_body_offset}]",
        "test edx, edx",
        "jz 1f",
        "cmp byte ptr [edx], {message_opcode}",
        "je 5f",
        "cmp byte ptr [edx], {who_response_opcode}",
        "je 5f",
        "cmp byte ptr [edx], {exchange_opcode}",
        "je 5f",
        "cmp byte ptr [edx], {player_response_opcode}",
        "je 5f",
        "cmp byte ptr [edx], {self_response_opcode}",
        "jne 1f",
        "5:",
        "push ecx",
        "push dword ptr [esp + 20]",
        "call {intercept}",
        "add esp, 4",
        "mov esi, eax",
        "pop ecx",
        "pop edx",
        "pop eax",
        "test esi, esi",
        "jnz 2f",
        "jmp 3f",
        "1:",
        "pop edx",
        "pop eax",
        "3:",
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
        "2:",
        "mov eax, 1",
        "pop esi",
        "lock dec dword ptr [{activity}]",
        "ret 4",
        activity = sym EVENT_HOOK_ACTIVITY,
        trampoline = sym EVENT_TRAMPOLINE,
        observe = sym observe_event,
        intercept = sym intercept_event,
        intercept_command_id = sym crate::who::INTERCEPT_COMMAND_ID,
        exchange_intercept_pending = sym crate::exchange::INTERCEPT_PENDING,
        player_intercept_pending = sym crate::player::INTERCEPT_PENDING,
        look_intercept_command_id = sym crate::look::INTERCEPT_COMMAND_ID,
        event_type_offset = const EVENT_TYPE_OFFSET,
        server_event_type = const SERVER_EVENT_TYPE,
        event_body_offset = const EVENT_BODY_OFFSET,
        message_opcode = const 0x0a_u8,
        who_response_opcode = const 0x36_u8,
        exchange_opcode = const 0x42_u8,
        player_response_opcode = const 0x34_u8,
        self_response_opcode = const 0x39_u8,
    );
}

extern "C" fn intercept_event(event: *const core::ffi::c_void) -> bool {
    panic::catch_unwind(|| {
        if diagnostics::hook_timing_enabled() {
            diagnostics::measure(HookTimingStage::Event, || intercept_event_inner(event))
        } else {
            intercept_event_inner(event)
        }
    })
    .unwrap_or(false)
}

fn intercept_event_inner(event: *const core::ffi::c_void) -> bool {
    if event.is_null() {
        return false;
    }
    let mut view = [0_u8; EVENT_VIEW_LENGTH];
    let Some(address) = (event as usize).checked_add(EVENT_TYPE_OFFSET) else {
        return false;
    };
    if !read_exact(address, &mut view) || view[0] != SERVER_EVENT_TYPE {
        return false;
    }
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
        return false;
    }
    let mut opcode = [0];
    if !read_exact(body_address as usize, &mut opcode)
        || !matches!(opcode[0], 0x0a | 0x34 | 0x36 | 0x39 | 0x42)
    {
        return false;
    }
    if EVENT_SCRATCH_IN_USE
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }
    let _scratch_guard = EventScratchGuard;
    // SAFETY: the scratch guard owns exclusive access for this bounded copy.
    let scratch = unsafe { &mut *EVENT_SCRATCH.0.get() };
    if !read_exact(body_address as usize, &mut scratch.body[..body_length]) {
        EVENT_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    let body = &scratch.body[..body_length];
    let suppressed = ServerEventProcessor::intercept(body);
    if suppressed {
        EVENT_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
        SERVER_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    suppressed
}

extern "C" fn observe_event(event: *const core::ffi::c_void) {
    let _ = panic::catch_unwind(|| {
        if diagnostics::hook_timing_enabled() {
            diagnostics::measure(HookTimingStage::Event, || observe_event_inner(event));
        } else {
            observe_event_inner(event);
        }
    });
}

fn observe_event_inner(event: *const core::ffi::c_void) {
    if event.is_null() {
        return;
    }
    EVENT_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut view = [0_u8; EVENT_VIEW_LENGTH];
    let Some(address) = (event as usize).checked_add(EVENT_TYPE_OFFSET) else {
        return;
    };
    if !read_exact(address, &mut view) {
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
    if EVENT_SCRATCH_IN_USE
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let _scratch_guard = EventScratchGuard;
    // SAFETY: the successful compare_exchange above gives this invocation
    // exclusive access until _scratch_guard releases the flag.
    let scratch = unsafe { &mut *EVENT_SCRATCH.0.get() };
    if !read_exact(body_address as usize, &mut scratch.body[..body_length]) {
        EVENT_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let EventScratch {
        body,
        server_events,
    } = scratch;
    let body = &body[..body_length];
    let tick_ms = sender_tick_ms();
    match server_events.observe(body, tick_ms) {
        Ok(Observation::Observed) => {
            EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        Ok(Observation::Ignored) => {}
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::support;
    use darpc_game_client::EVENT_DISPATCH_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_event_entry() {
        let mut entry = EVENT_DISPATCH_ENTRY;
        let target = NonNull::new(entry.as_mut_ptr()).unwrap();
        support::validate_bytes(target, &EVENT_DISPATCH_ENTRY, "event entry").unwrap();

        let mut wrong_entry = EVENT_DISPATCH_ENTRY;
        wrong_entry[0] ^= 0xFF;
        let wrong_target = NonNull::new(wrong_entry.as_mut_ptr()).unwrap();
        assert_eq!(
            support::validate_bytes(wrong_target, &EVENT_DISPATCH_ENTRY, "event entry")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
