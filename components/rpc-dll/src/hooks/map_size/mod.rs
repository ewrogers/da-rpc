use darpc_game_client::{MAP_SIZE_HANDLER_ENTRY, MAP_SIZE_HANDLER_RVA};
use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use darpc_win32::pipe::sender_tick_ms;
use std::{
    io, panic,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::{map_name, state};

pub(crate) const NAME: &str = "map_size_handler";

const DETOUR_RANGE_LEN: usize = 96;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const COMMIT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const PACKET_MAP_ID_OFFSET: usize = 0x10;
const PACKET_WIDTH_OFFSET: usize = 0x12;
const PACKET_HEIGHT_OFFSET: usize = 0x13;
const PACKET_NAME_OFFSET: usize = 0x1C;
const MAX_MAP_NAME_BYTES: usize = u8::MAX as usize;

#[unsafe(no_mangle)]
static MAP_SIZE_HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
static MAP_SIZE_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct MapSizeHook {
    detour: InstalledDetour,
    relocated_bytes: u8,
    install_warning: Option<io::Error>,
}

impl MapSizeHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        let target = target_address()?;
        validate_entry(target)?;
        let detour = NonNull::new(map_size_handler_detour as *mut u8)
            .ok_or_else(|| io::Error::other("map-size detour address is null"))?;
        let detour_range = CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)
            .map_err(InstallError::from)?;
        let spec = DetourSpec::new(target, detour, detour_range, &MAP_SIZE_HOOK_ACTIVITY)
            .map_err(InstallError::from)?;

        // SAFETY: client fingerprint and exact target entry bytes were
        // validated. The detour preserves the handler's thiscall ABI and owns
        // no client pointers after the callback returns.
        let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
        let relocated_bytes = u8::try_from(prepared.relocated_len())
            .map_err(|_| io::Error::other("map-size relocated length does not fit in u8"))?;
        MAP_SIZE_TRAMPOLINE.store(
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
                    MAP_SIZE_TRAMPOLINE.store(0, Ordering::Release);
                    return Err(InstallError::from(error));
                }
            }
        };
        let install_warning = detour.take_resume_warning().map(detour_error);

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
                    MAP_SIZE_TRAMPOLINE.store(0, Ordering::Release);
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

fn target_address() -> io::Result<NonNull<u8>> {
    // SAFETY: a null module name requests the executable module for the current
    // process and has no lifetime transfer.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(MAP_SIZE_HANDLER_RVA)
        .ok_or_else(|| io::Error::other("map-size target address overflow"))?;
    NonNull::new(address as *mut u8)
        .ok_or_else(|| io::Error::other("map-size target address is null"))
}

fn validate_entry(target: NonNull<u8>) -> io::Result<()> {
    // SAFETY: the supported executable fingerprint establishes that this RVA
    // names readable executable memory spanning the fixed entry contract.
    let actual = unsafe { slice::from_raw_parts(target.as_ptr(), MAP_SIZE_HANDLER_ENTRY.len()) };
    if actual == MAP_SIZE_HANDLER_ENTRY {
        Ok(())
    } else {
        Err(io::Error::other(
            "map-size handler entry bytes do not match",
        ))
    }
}

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}

#[unsafe(naked)]
unsafe extern "thiscall" fn map_size_handler_detour(
    _world: *mut core::ffi::c_void,
    _packet: *const core::ffi::c_void,
) -> u8 {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push ecx",
        "push dword ptr [esp + 8]",
        "push ecx",
        "call {observe}",
        "add esp, 8",
        "pop ecx",
        "push dword ptr [esp + 4]",
        "call dword ptr [{trampoline}]",
        "lock dec dword ptr [{activity}]",
        "ret 4",
        activity = sym MAP_SIZE_HOOK_ACTIVITY,
        observe = sym observe_map_size,
        trampoline = sym MAP_SIZE_TRAMPOLINE,
    );
}

extern "C" fn observe_map_size(world: *const core::ffi::c_void, packet: *const u8) {
    let _ = panic::catch_unwind(|| {
        if world.is_null() || packet.is_null() {
            return;
        }
        // SAFETY: this callback runs synchronously at the validated map-size
        // handler entry. Its packet argument is a live SMapSize object with a
        // fixed inline name buffer for the duration of the call.
        let (map_id, width, height, name) = unsafe {
            let map_id = u16::from_le_bytes([
                *packet.add(PACKET_MAP_ID_OFFSET),
                *packet.add(PACKET_MAP_ID_OFFSET + 1),
            ]);
            let name = slice::from_raw_parts(packet.add(PACKET_NAME_OFFSET), MAX_MAP_NAME_BYTES);
            let length = name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len());
            (
                u32::from(map_id),
                i32::from(*packet.add(PACKET_WIDTH_OFFSET)),
                i32::from(*packet.add(PACKET_HEIGHT_OFFSET)),
                &name[..length],
            )
        };
        map_name::publish(world as usize as u32, map_id, name);
        state::stage_map_transition(map_id, width, height, name, sender_tick_ms());
    });
}
