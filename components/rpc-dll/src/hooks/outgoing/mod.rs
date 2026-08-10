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
    Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
    LibraryLoader::GetModuleHandleW,
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

pub(crate) const NAME: &str = "client_packet_submit";

const DETOUR_RANGE_LEN: usize = 128;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const MESSAGE_OPCODE: u8 = 0x0E;
const SAY_MODE: u8 = 0;
const MAX_OUTGOING_BODY: usize = u8::MAX as usize + 3;

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
        "push ecx",
        "push dword ptr [esp + 12]",
        "push dword ptr [esp + 12]",
        "call {intercept}",
        "add esp, 8",
        "pop ecx",
        "test eax, eax",
        "jz 1f",
        "mov dword ptr [esp + 8], eax",
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
        "1:",
        "xor eax, eax",
        "lock dec dword ptr [{activity}]",
        "ret 8",
        activity = sym OUTGOING_HOOK_ACTIVITY,
        trampoline = sym OUTGOING_TRAMPOLINE,
        intercept = sym intercept_packet,
        observe = sym observe_packet,
    );
}

extern "C" fn intercept_packet(body: *mut u8, length: i16) -> i32 {
    let original_length = i32::from(length);
    panic::catch_unwind(|| {
        let Ok(length) = usize::try_from(length) else {
            return original_length;
        };
        if body.is_null() || length == 0 {
            return original_length;
        }
        if !is_client_main_thread() {
            return original_length;
        }
        let mut prefix = [0; 3];
        let prefix_length = length.min(prefix.len());
        if !read_memory(body as usize, &mut prefix[..prefix_length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return original_length;
        }
        if prefix_length == 3 && prefix[0] == MESSAGE_OPCODE && prefix[1] == SAY_MODE {
            let expected = usize::from(prefix[2]) + 3;
            if length != expected || length > MAX_OUTGOING_BODY {
                return original_length;
            }
            let mut packet = [0; MAX_OUTGOING_BODY];
            if !read_memory(body as usize, &mut packet[..length]) {
                OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
                return original_length;
            }
            return match say_action(&packet[..length]) {
                SayAction::Pass => original_length,
                SayAction::Command(command) => {
                    crate::state::observe_command(command, sender_tick_ms());
                    0
                }
                SayAction::Escape => {
                    let escaped_length = escape_say(&mut packet[..length])
                        .expect("classified double-slash packet is escapable");
                    if write_memory(body as usize, &packet[..escaped_length]) {
                        i32::try_from(escaped_length).expect("outgoing packet length fits i32")
                    } else {
                        original_length
                    }
                }
            };
        }
        original_length
    })
    .unwrap_or(original_length)
}

extern "C" fn observe_packet(body: *const u8, length: i16) {
    let _ = panic::catch_unwind(|| {
        let Ok(length) = usize::try_from(length) else {
            return;
        };
        if body.is_null() || length == 0 || !is_client_main_thread() {
            return;
        }
        let mut prefix = [0; 2];
        let prefix_length = length.min(prefix.len());
        if !read_memory(body as usize, &mut prefix[..prefix_length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if !matches!(
            prefix[0],
            0x07 | 0x08
                | 0x0F
                | 0x11
                | 0x1C
                | 0x1D
                | 0x24
                | 0x29
                | 0x2A
                | 0x3E
                | 0x44
                | 0x4D
                | 0x4E
                | 0x18
        ) {
            return;
        }
        OUTGOING_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
        let expected = match prefix[0] {
            0x18 => 1,
            0x07 => 6,
            0x08 | 0x29 => 10,
            0x24 | 0x2A => 9,
            0x0F => length,
            _ => 2,
        };
        if length != expected || length > MAX_OUTGOING_BODY {
            return;
        }
        if prefix[0] == 0x18 {
            crate::who::observe_request(sender_tick_ms());
            return;
        }
        if length == 2 {
            crate::state::observe_outgoing(&prefix, sender_tick_ms());
            return;
        }
        let mut packet = [0; MAX_OUTGOING_BODY];
        if !read_memory(body as usize, &mut packet[..length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        crate::state::observe_outgoing(&packet[..length], sender_tick_ms());
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SayAction<'a> {
    Pass,
    Command(&'a [u8]),
    Escape,
}

fn say_action(packet: &[u8]) -> SayAction<'_> {
    let Some(text) = packet.get(3..) else {
        return SayAction::Pass;
    };
    if text.starts_with(b"//") {
        return SayAction::Escape;
    }
    let Some(command) = text.strip_prefix(b"/") else {
        return SayAction::Pass;
    };
    let name_length = command
        .iter()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(command.len());
    if name_length == 0 {
        SayAction::Pass
    } else {
        SayAction::Command(command)
    }
}

fn escape_say(packet: &mut [u8]) -> Option<usize> {
    if !packet.get(3..)?.starts_with(b"//") {
        return None;
    }
    packet[2] = packet[2].checked_sub(1)?;
    let length = packet.len();
    packet.copy_within(4..length, 3);
    Some(length - 1)
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

fn write_memory(address: usize, input: &[u8]) -> bool {
    let mut written = 0_usize;
    // SAFETY: WriteProcessMemory validates the destination range and input is
    // readable for exactly input.len() bytes.
    let succeeded = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *mut core::ffi::c_void,
            input.as_ptr().cast(),
            input.len(),
            &mut written,
        )
    };
    succeeded != 0 && written == input.len()
}

#[cfg(test)]
mod tests {
    use super::{SayAction, escape_say, say_action, validate_entry};
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

    #[test]
    fn classifies_commands_and_double_slash_escapes() {
        assert_eq!(
            say_action(b"\x0E\0\x09/walk x,y"),
            SayAction::Command(b"walk x,y")
        );
        assert_eq!(say_action(b"\x0E\0\x0A//walk x,y"), SayAction::Escape);
        assert_eq!(say_action(b"\x0E\0\x05hello"), SayAction::Pass);
        assert_eq!(say_action(b"\x0E\0\x01/"), SayAction::Pass);
    }

    #[test]
    fn removes_one_slash_and_updates_the_say_packet_lengths() {
        let mut packet = *b"\x0E\0\x0A//walk x,y";
        let length = escape_say(&mut packet).unwrap();
        assert_eq!(&packet[..length], b"\x0E\0\x09/walk x,y");
    }
}
