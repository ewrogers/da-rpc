mod refresh;

use darpc_game_client::{
    CLIENT_MAIN_THREAD_ID_RVA, CLIENT_PACKET_SUBMIT_ENTRY, CLIENT_PACKET_SUBMIT_RVA,
};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use darpc_win32::pipe::sender_tick_ms;
use std::{
    io, panic, ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::WriteProcessMemory,
    LibraryLoader::GetModuleHandleW,
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

use super::{
    heartbeat_priority::{self, HeartbeatPriorityHook},
    support,
};
use crate::process_memory::read_exact;
use refresh::PhysicalRefreshHook;

pub(crate) const NAME: &str = "client_packet_submit";

const DETOUR_RANGE_LEN: usize = 128;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const MESSAGE_OPCODE: u8 = 0x0E;
const SAY_MODE: u8 = 0;
const MAX_OUTGOING_BODY: usize = u8::MAX as usize + 3;
const MAX_BULLETIN_OBSERVATION_BODY: usize = 7;
const MAX_BOARD_POST_BODY: usize = 2 + 2 + 1 + 60 + 2 + 3_000;
const MAX_PLAYER_MAIL_BODY: usize = 2 + 2 + 1 + 15 + 1 + 60 + 2 + 3_000;

#[unsafe(no_mangle)]
static OUTGOING_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static OUTGOING_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static OUTGOING_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static OUTGOING_RELOCATED_BYTES: AtomicU32 = AtomicU32::new(0);
static OUTGOING_OBSERVATION_COUNT: AtomicU32 = AtomicU32::new(0);
static OUTGOING_READ_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) struct OutgoingHook {
    detour: InstalledDetour,
    physical_refresh: PhysicalRefreshHook,
    heartbeat_priority: HeartbeatPriorityHook,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OutgoingHealth {
    pub(crate) observation_count: u32,
    pub(crate) read_failure_count: u32,
    pub(crate) physical_refresh_deferred_count: u32,
    pub(crate) prioritized_heartbeat_count: u32,
    pub(crate) delivered_heartbeat_count: u32,
    pub(crate) heartbeat_fallback_count: u32,
    pub(crate) pending_heartbeat_count: usize,
}

impl OutgoingHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = support::target_address(
            support::module_base()?,
            CLIENT_PACKET_SUBMIT_RVA,
            "outgoing packet target",
        )?;
        support::validate_bytes(target, &CLIENT_PACKET_SUBMIT_ENTRY, "outgoing packet entry")?;

        // SAFETY: exact executable identity and entry bytes are validated. The
        // detour preserves the two-argument thiscall ABI and copies bounded
        // packet bytes only after the original submission routine returns.
        let mut prepared = unsafe {
            support::prepare_detour(
                target,
                client_packet_submit_detour as *mut u8,
                DETOUR_RANGE_LEN,
                &OUTGOING_HOOK_ACTIVITY,
                "outgoing packet detour",
            )
        }?;
        let relocated_bytes = support::relocated_bytes(&prepared, "outgoing prologue")?;
        OUTGOING_OBSERVATION_COUNT.store(0, Ordering::Release);
        OUTGOING_READ_FAILURE_COUNT.store(0, Ordering::Release);
        OUTGOING_RELOCATED_BYTES.store(u32::from(relocated_bytes), Ordering::Release);
        OUTGOING_TRAMPOLINE.store(
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
                OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
                OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        };
        let install_warning = detour.take_resume_warning().map(support::detour_error);
        let mut physical_refresh = match PhysicalRefreshHook::install() {
            Ok(hook) => hook,
            Err(error) => {
                support::uninstall_detour(&mut detour, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
                    .map_err(InstallError::from)?;
                OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
                OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
                return Err(error);
            }
        };
        let mut heartbeat_priority = match HeartbeatPriorityHook::install() {
            Ok(hook) => hook,
            Err(error) => {
                let refresh_error = physical_refresh.uninstall().err();
                let outgoing_error = support::uninstall_detour(
                    &mut detour,
                    UNINSTALL_TIMEOUT,
                    COMMIT_RETRY_INTERVAL,
                )
                .err()
                .map(support::detour_error);
                if outgoing_error.is_none() {
                    OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
                    OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
                }
                if let Some(cleanup_error) = refresh_error.or(outgoing_error) {
                    return Err(InstallError::from(cleanup_error));
                }
                return Err(error);
            }
        };
        let install_warning = install_warning
            .or(physical_refresh.take_install_warning())
            .or(heartbeat_priority.take_install_warning());
        OUTGOING_HOOK_INSTALLED.store(true, Ordering::Release);
        Ok(Self {
            detour,
            physical_refresh,
            heartbeat_priority,
            relocated_bytes,
            install_warning,
        })
    }

    pub(crate) const fn relocated_bytes(&self) -> u8 {
        self.relocated_bytes
    }

    pub(crate) const fn physical_refresh_relocated_bytes(&self) -> u8 {
        self.physical_refresh.relocated_bytes()
    }

    pub(crate) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(crate) fn uninstall(&mut self) -> io::Result<bool> {
        let physical_refresh_changed = self.physical_refresh.uninstall()?;
        let heartbeat_changed = self.heartbeat_priority.uninstall()?;
        let changed =
            support::uninstall_detour(&mut self.detour, UNINSTALL_TIMEOUT, COMMIT_RETRY_INTERVAL)
                .map_err(support::detour_error)?;
        OUTGOING_HOOK_INSTALLED.store(false, Ordering::Release);
        OUTGOING_TRAMPOLINE.store(0, Ordering::Release);
        OUTGOING_RELOCATED_BYTES.store(0, Ordering::Release);
        Ok(physical_refresh_changed || heartbeat_changed || changed)
    }
}

pub(crate) fn health() -> OutgoingHealth {
    let heartbeat = heartbeat_priority::health();
    OutgoingHealth {
        observation_count: OUTGOING_OBSERVATION_COUNT.load(Ordering::Acquire),
        read_failure_count: OUTGOING_READ_FAILURE_COUNT.load(Ordering::Acquire),
        physical_refresh_deferred_count: refresh::deferred_count(),
        prioritized_heartbeat_count: heartbeat.prioritized_count,
        delivered_heartbeat_count: heartbeat.delivered_count,
        heartbeat_fallback_count: heartbeat.fallback_count,
        pending_heartbeat_count: heartbeat.pending_count,
    }
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
        let mut prefix = [0; 3];
        let prefix_length = length.min(prefix.len());
        if !read_exact(body as usize, &mut prefix[..prefix_length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            crate::look::quarantine();
            return original_length;
        }
        if !is_client_main_thread() {
            if matches!(prefix[0], 0x09 | 0x0a) {
                crate::look::quarantine();
            }
            return original_length;
        }
        if matches!(prefix[0], 0x09 | 0x0a) {
            let mut packet = [0; 5];
            if length > packet.len() || !read_exact(body as usize, &mut packet[..length]) {
                crate::look::quarantine();
                return original_length;
            }
            crate::look::observe_outgoing(&packet[..length], crate::commands::action_source());
        }
        if prefix_length == 3 && prefix[0] == MESSAGE_OPCODE && prefix[1] == SAY_MODE {
            let expected = usize::from(prefix[2]) + 3;
            if length != expected || length > MAX_OUTGOING_BODY {
                return original_length;
            }
            let mut packet = [0; MAX_OUTGOING_BODY];
            if !read_exact(body as usize, &mut packet[..length]) {
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
        if !read_exact(body as usize, &mut prefix[..prefix_length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if !matches!(
            prefix[0],
            0x05 | 0x07
                | 0x08
                | 0x0F
                | 0x11
                | 0x1C
                | 0x1D
                | 0x24
                | 0x29
                | 0x2A
                | 0x2D
                | 0x30
                | 0x38
                | 0x3B
                | 0x3E
                | 0x44
                | 0x4A
                | 0x4D
                | 0x4E
                | 0x18
                | 0x43
                | 0x3F
        ) {
            return;
        }
        OUTGOING_OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
        if prefix[0] == 0x3B {
            if !valid_bulletin_length(prefix[1], length) {
                return;
            }
            let observation_length = length.min(MAX_BULLETIN_OBSERVATION_BODY);
            let mut packet = [0; MAX_BULLETIN_OBSERVATION_BODY];
            if !read_exact(body as usize, &mut packet[..observation_length]) {
                OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
                return;
            }
            crate::state::observe_outgoing(&packet[..observation_length], sender_tick_ms());
            return;
        }
        let Some(expected) = expected_body_length(prefix, length) else {
            return;
        };
        if length != expected || length > MAX_OUTGOING_BODY {
            return;
        }
        if prefix[0] == 0x18 {
            crate::who::observe_request(sender_tick_ms());
            return;
        }
        if prefix[0] == 0x2D {
            crate::player::observe_client_self_look(sender_tick_ms());
            return;
        }
        if length == 2 {
            crate::state::observe_outgoing(&prefix, sender_tick_ms());
            return;
        }
        let mut packet = [0; MAX_OUTGOING_BODY];
        if !read_exact(body as usize, &mut packet[..length]) {
            OUTGOING_READ_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if packet[0] == 0x43 && packet[1] == 1 {
            let id = u32::from_be_bytes(packet[2..6].try_into().expect("object request body"));
            crate::player::observe_request(id, sender_tick_ms());
            return;
        }
        crate::state::observe_outgoing(&packet[..length], sender_tick_ms());
    });
}

fn expected_body_length(prefix: [u8; 2], variable_length: usize) -> Option<usize> {
    Some(match prefix[0] {
        0x18 | 0x2D | 0x38 => 1,
        0x30 => 4,
        0x43 => match prefix[1] {
            0x03 => 7,
            _ => 6,
        },
        0x07 => 6,
        0x05 | 0x08 | 0x29 => 10,
        0x24 | 0x2A | 0x3F => 9,
        0x4A => match prefix[1] {
            0x01 => 7,
            0x02 => 8,
            0x03 => 10,
            0x04 | 0x05 => 6,
            _ => return None,
        },
        0x0F => variable_length,
        _ => 2,
    })
}

fn valid_bulletin_length(subtype: u8, length: usize) -> bool {
    match subtype {
        1 => length == 2,
        2 | 3 => length == 7,
        4 => (7..=MAX_BOARD_POST_BODY).contains(&length),
        5 => matches!(length, 6 | 7),
        6 => (8..=MAX_PLAYER_MAIL_BODY).contains(&length),
        7 => length == 6,
        _ => false,
    }
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
    if module == 0 || !read_exact(address, &mut bytes) {
        return false;
    }
    let expected = u32::from_le_bytes(bytes);
    // SAFETY: GetCurrentThreadId has no preconditions.
    expected != 0 && expected == unsafe { GetCurrentThreadId() }
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
    use super::{
        SayAction, escape_say, expected_body_length, say_action, support, valid_bulletin_length,
    };
    use darpc_game_client::CLIENT_PACKET_SUBMIT_ENTRY;
    use std::ptr::NonNull;

    #[test]
    fn validates_the_exact_outgoing_entry() {
        let mut entry = CLIENT_PACKET_SUBMIT_ENTRY;
        support::validate_bytes(
            NonNull::new(entry.as_mut_ptr()).unwrap(),
            &CLIENT_PACKET_SUBMIT_ENTRY,
            "outgoing entry",
        )
        .unwrap();
        entry[0] ^= 0xFF;
        assert_eq!(
            support::validate_bytes(
                NonNull::new(entry.as_mut_ptr()).unwrap(),
                &CLIENT_PACKET_SUBMIT_ENTRY,
                "outgoing entry",
            )
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

    #[test]
    fn recognizes_exchange_packet_lengths() {
        assert_eq!(expected_body_length([0x4A, 0x01], 7), Some(7));
        assert_eq!(expected_body_length([0x4A, 0x02], 8), Some(8));
        assert_eq!(expected_body_length([0x4A, 0x03], 10), Some(10));
        assert_eq!(expected_body_length([0x4A, 0x04], 6), Some(6));
        assert_eq!(expected_body_length([0x4A, 0x05], 6), Some(6));
        assert_eq!(expected_body_length([0x4A, 0x06], 6), None);
    }

    #[test]
    fn validates_bulletin_lengths_without_copying_compose_bodies() {
        assert!(valid_bulletin_length(1, 2));
        assert!(valid_bulletin_length(2, 7));
        assert!(valid_bulletin_length(3, 7));
        assert!(valid_bulletin_length(4, 3_067));
        assert!(valid_bulletin_length(5, 6));
        assert!(valid_bulletin_length(5, 7));
        assert!(valid_bulletin_length(6, 3_083));
        assert!(valid_bulletin_length(7, 6));
        assert!(!valid_bulletin_length(1, 3));
        assert!(!valid_bulletin_length(4, 3_068));
        assert!(!valid_bulletin_length(6, 3_084));
        assert!(!valid_bulletin_length(8, 2));
    }
}
