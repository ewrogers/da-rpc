use darpc_game_client::{MAP_SIZE_HANDLER_ENTRY, MAP_SIZE_HANDLER_RVA};
use darpc_hook::{DetourActivity, InstallError, InstalledDetour};
use darpc_win32::pipe::sender_tick_ms;
use std::{
    io, panic, slice,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use super::support;
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
        let target = support::target_address(
            support::module_base()?,
            MAP_SIZE_HANDLER_RVA,
            "map-size target",
        )?;
        support::validate_bytes(target, &MAP_SIZE_HANDLER_ENTRY, "map-size handler entry")?;

        // SAFETY: client fingerprint and exact target entry bytes were
        // validated. The detour preserves the handler's thiscall ABI and owns
        // no client pointers after the callback returns.
        let mut prepared = unsafe {
            support::prepare_detour(
                target,
                map_size_handler_detour as *mut u8,
                DETOUR_RANGE_LEN,
                &MAP_SIZE_HOOK_ACTIVITY,
                "map-size detour",
            )
        }?;
        let relocated_bytes = support::relocated_bytes(&prepared, "map-size prologue")?;
        MAP_SIZE_TRAMPOLINE.store(
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
                MAP_SIZE_TRAMPOLINE.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        };
        let install_warning = detour.take_resume_warning().map(support::detour_error);

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
        MAP_SIZE_TRAMPOLINE.store(0, Ordering::Release);
        Ok(changed)
    }
}

#[unsafe(naked)]
unsafe extern "thiscall" fn map_size_handler_detour(
    _world: *mut core::ffi::c_void,
    _packet: *const core::ffi::c_void,
) -> u8 {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "push ecx",
        "push dword ptr [esp + 12]",
        "push ecx",
        "call {observe}",
        "add esp, 8",
        "pop ecx",
        "push dword ptr [esp + 8]",
        "call dword ptr [{trampoline}]",
        "mov esi, eax",
        "call {finish}",
        "mov eax, esi",
        "pop esi",
        "lock dec dword ptr [{activity}]",
        "ret 4",
        activity = sym MAP_SIZE_HOOK_ACTIVITY,
        observe = sym observe_map_size,
        finish = sym finish_map_size,
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

extern "C" fn finish_map_size() {
    let _ = panic::catch_unwind(state::finish_map_download_stage);
}
