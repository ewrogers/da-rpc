use darpc_game_client::{
    CLIENT_MAIN_THREAD_ID_RVA, CLIENT_PACKET_SUBMIT_ENTRY, CLIENT_PACKET_SUBMIT_RVA,
};
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
    Diagnostics::Debug::ReadProcessMemory,
    LibraryLoader::GetModuleHandleW,
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

pub(crate) const NAME: &str = "client_packet_submit";

const DETOUR_RANGE_LEN: usize = 128;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const MAX_USE_SPELL_BODY: usize = 102;

#[unsafe(no_mangle)]
static OUTGOING_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static OUTGOING_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static OUTGOING_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static OUTGOING_RELOCATED_BYTES: AtomicU32 = AtomicU32::new(0);
static OUTGOING_OBSERVATION_COUNT: AtomicU32 = AtomicU32::new(0);
static OUTGOING_READ_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) struct OutgoingHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OutgoingHealth {
    pub(crate) observation_count: u32,
    pub(crate) read_failure_count: u32,
}

impl OutgoingHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = target_address()?;
        validate_entry(target)?;
        let detour = NonNull::new(client_packet_submit_detour as *mut u8)
            .ok_or_else(|| io::Error::other("outgoing packet detour address is null"))?;
        let detour_range = CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)
            .map_err(InstallError::from)?;
        let spec = DetourSpec::new(target, detour, detour_range, &OUTGOING_HOOK_ACTIVITY)
            .map_err(InstallError::from)?;

        // SAFETY: exact executable identity and entry bytes are validated. The
        // detour preserves the two-argument thiscall ABI and copies bounded
        // packet bytes only after the original submission routine returns.
        let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
        let relocated_bytes = u8::try_from(prepared.relocated_len())
            .map_err(|_| io::Error::other("relocated outgoing prologue exceeds u8"))?;
        OUTGOING_OBSERVATION_COUNT.store(0, Ordering::Release);
        OUTGOING_READ_FAILURE_COUNT.store(0, Ordering::Release);
        OUTGOING_RELOCATED_BYTES.store(u32::from(relocated_bytes), Ordering::Release);
        OUTGOING_TRAMPOLINE.store(
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
                    OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
                    OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
                    return Err(InstallError::from(error));
                }
            }
        };
        let install_warning = detour.take_resume_warning().map(detour_error);
        OUTGOING_HOOK_INSTALLED.store(true, Ordering::Release);
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
                    OUTGOING_HOOK_INSTALLED.store(false, Ordering::Release);
                    OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
                    OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
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

pub(crate) fn health() -> OutgoingHealth {
    OutgoingHealth {
        observation_count: OUTGOING_OBSERVATION_COUNT.load(Ordering::Acquire),
        read_failure_count: OUTGOING_READ_FAILURE_COUNT.load(Ordering::Acquire),
    }
}

fn target_address() -> io::Result<NonNull<u8>> {
    // SAFETY: a null module name requests the current executable module.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(CLIENT_PACKET_SUBMIT_RVA)
        .ok_or_else(|| io::Error::other("outgoing packet target address overflow"))?;
    NonNull::new(address as *mut u8)
        .ok_or_else(|| io::Error::other("outgoing packet target address is null"))
}

fn validate_entry(target: NonNull<u8>) -> io::Result<()> {
    // SAFETY: supported executable validation establishes a readable target.
    let actual =
        unsafe { slice::from_raw_parts(target.as_ptr(), CLIENT_PACKET_SUBMIT_ENTRY.len()) };
    if actual != CLIENT_PACKET_SUBMIT_ENTRY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{NAME} entry mismatch at RVA 0x{CLIENT_PACKET_SUBMIT_RVA:08X}: expected={:02X?} actual={actual:02X?}",
                CLIENT_PACKET_SUBMIT_ENTRY
            ),
        ));
    }
    Ok(())
}

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn client_packet_submit_detour(
    _network: *mut core::ffi::c_void,
    _body: *const u8,
    _length: i16,
) -> u32 {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "push dword ptr [esp + 12]",
        "push dword ptr [esp + 12]",
        "call dword ptr [{trampoline}]",
        "mov esi, eax",
        "push dword ptr [esp + 12]",
        "push dword ptr [esp + 12]",
        "call {observe}",
        "add esp, 8",
        "mov eax, esi",
        "pop esi",
        "lock dec dword ptr [{activity}]",
        "ret 8",
        activity = sym OUTGOING_HOOK_ACTIVITY,
        trampoline = sym OUTGOING_TRAMPOLINE,
        observe = sym observe_packet,
    );
}

extern "C" fn observe_packet(body: *const u8, length: i16) {
    let _ = panic::catch_unwind(|| {
        let Ok(length) = usize::try_from(length) else {
            return;
        };
        if body.is_null() || length < 2 {
            return;
        }
        if !is_client_main_thread() {
            return;
        }
        let mut prefix = [0; 2];
        if !read_memory(body as usize, &mut prefix) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if !matches!(prefix[0], 0x0F | 0x3E | 0x4D | 0x4E) {
            return;
        }
        OUTGOING_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
        if prefix[0] != 0x0F {
            crate::state_events::observe_outgoing(&prefix, sender_tick_ms());
            return;
        }
        if length > MAX_USE_SPELL_BODY {
            return;
        }
        let mut packet = [0; MAX_USE_SPELL_BODY];
        if !read_memory(body as usize, &mut packet[..length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        crate::state_events::observe_outgoing(&packet[..length], sender_tick_ms());
    });
}

fn is_client_main_thread() -> bool {
    // SAFETY: a null module name requests the current executable module.
    let module = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    let Some(address) = module.checked_add(CLIENT_MAIN_THREAD_ID_RVA) else {
        return false;
    };
    let mut bytes = [0; 4];
    if module == 0 || !read_memory(address, &mut bytes) {
        return false;
    }
    let expected = u32::from_le_bytes(bytes);
    // SAFETY: GetCurrentThreadId has no preconditions.
    expected != 0 && expected == unsafe { GetCurrentThreadId() }
}

fn read_memory(address: usize, output: &mut [u8]) -> bool {
    let mut read = 0_usize;
    // SAFETY: output is valid and ReadProcessMemory validates the source range.
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
    use darpc_game_client::CLIENT_PACKET_SUBMIT_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_outgoing_entry() {
        let mut entry = CLIENT_PACKET_SUBMIT_ENTRY;
        validate_entry(NonNull::new(entry.as_mut_ptr()).unwrap()).unwrap();
        entry[0] ^= 0xFF;
        assert_eq!(
            validate_entry(NonNull::new(entry.as_mut_ptr()).unwrap())
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
