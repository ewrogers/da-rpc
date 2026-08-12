use crate::heartbeat_priority::{self, DESCRIPTOR_WORDS};
use darpc_game_client::{
    CLIENT_TRANSPORT_POP_ENTRY, CLIENT_TRANSPORT_POP_RVA, CLIENT_TRANSPORT_SUBMIT_ENTRY,
    CLIENT_TRANSPORT_SUBMIT_RVA,
};
use darpc_hook::{
    CodeRange, DetourActivity, DetourError, DetourSpec, InstallError, InstalledDetour,
    PreparedDetour,
};
use std::{
    io, panic, ptr,
    ptr::NonNull,
    slice,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Threading::{GetCurrentProcess, ReleaseSemaphore},
    },
};

const DETOUR_RANGE_LEN: usize = 128;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_INTERVAL: Duration = Duration::from_millis(1);
const TRANSPORT_QUEUE_OFFSET: usize = 0x54;
const TRANSPORT_SEMAPHORE_OFFSET: usize = 0x14;

#[unsafe(no_mangle)]
static HEARTBEAT_SUBMIT_ACTIVITY: DetourActivity = DetourActivity::new();
#[unsafe(no_mangle)]
static HEARTBEAT_POP_ACTIVITY: DetourActivity = DetourActivity::new();
static HEARTBEAT_SUBMIT_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static HEARTBEAT_POP_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static PRIORITIZED_COUNT: AtomicU32 = AtomicU32::new(0);
static FALLBACK_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) struct HeartbeatPriorityHook {
    submit: InstalledDetour,
    pop: InstalledDetour,
    install_warning: Option<io::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Health {
    pub(crate) prioritized_count: u32,
    pub(crate) fallback_count: u32,
}

impl HeartbeatPriorityHook {
    pub(crate) fn install() -> Result<Self, InstallError> {
        PRIORITIZED_COUNT.store(0, Ordering::Release);
        FALLBACK_COUNT.store(0, Ordering::Release);

        let (mut pop, pop_warning) = install_detour(
            CLIENT_TRANSPORT_POP_RVA,
            &CLIENT_TRANSPORT_POP_ENTRY,
            transport_pop_detour as *mut u8,
            &HEARTBEAT_POP_ACTIVITY,
            &HEARTBEAT_POP_TRAMPOLINE,
        )?;
        let (submit, submit_warning) = match install_detour(
            CLIENT_TRANSPORT_SUBMIT_RVA,
            &CLIENT_TRANSPORT_SUBMIT_ENTRY,
            transport_submit_detour as *mut u8,
            &HEARTBEAT_SUBMIT_ACTIVITY,
            &HEARTBEAT_SUBMIT_TRAMPOLINE,
        ) {
            Ok(installed) => installed,
            Err(error) => {
                if let Err(rollback) = uninstall_detour(&mut pop, &HEARTBEAT_POP_TRAMPOLINE) {
                    return Err(InstallError::from(rollback));
                }
                return Err(error);
            }
        };

        Ok(Self {
            submit,
            pop,
            install_warning: pop_warning.or(submit_warning),
        })
    }

    pub(crate) fn take_install_warning(&mut self) -> Option<io::Error> {
        self.install_warning.take()
    }

    pub(crate) fn uninstall(&mut self) -> io::Result<bool> {
        let submit_changed = uninstall_detour(&mut self.submit, &HEARTBEAT_SUBMIT_TRAMPOLINE)
            .map_err(detour_error)?;
        let deadline = Instant::now() + UNINSTALL_TIMEOUT;
        while !heartbeat_priority::is_empty() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "heartbeat priority queue did not drain before hook removal",
                ));
            }
            thread::sleep(RETRY_INTERVAL);
        }
        let pop_changed =
            uninstall_detour(&mut self.pop, &HEARTBEAT_POP_TRAMPOLINE).map_err(detour_error)?;
        Ok(submit_changed || pop_changed)
    }
}

pub(crate) fn health() -> Health {
    Health {
        prioritized_count: PRIORITIZED_COUNT.load(Ordering::Acquire),
        fallback_count: FALLBACK_COUNT.load(Ordering::Acquire),
    }
}

fn install_detour(
    rva: usize,
    expected: &[u8],
    detour: *mut u8,
    activity: &'static DetourActivity,
    trampoline: &AtomicUsize,
) -> Result<(InstalledDetour, Option<io::Error>), InstallError> {
    let target = target_address(rva)?;
    validate_entry(target, rva, expected)?;
    let detour = NonNull::new(detour)
        .ok_or_else(|| io::Error::other("heartbeat priority detour address is null"))?;
    let range =
        CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN).map_err(InstallError::from)?;
    let spec = DetourSpec::new(target, detour, range, activity).map_err(InstallError::from)?;
    // SAFETY: the supported executable identity and exact entry bytes are
    // validated, and both detours preserve their native thiscall ABIs.
    let mut prepared = unsafe { PreparedDetour::prepare(spec) }.map_err(InstallError::from)?;
    trampoline.store(
        prepared.trampoline_address().map_err(InstallError::from)?,
        Ordering::Release,
    );
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    let mut installed = loop {
        match prepared.install() {
            Ok(installed) => break installed,
            Err(error) if error.is_transient() && Instant::now() < deadline => {
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => {
                trampoline.store(0, Ordering::Release);
                return Err(InstallError::from(error));
            }
        }
    };
    let warning = installed.take_resume_warning().map(detour_error);
    Ok((installed, warning))
}

fn uninstall_detour(
    detour: &mut InstalledDetour,
    trampoline: &AtomicUsize,
) -> Result<bool, DetourError> {
    let deadline = Instant::now() + UNINSTALL_TIMEOUT;
    loop {
        match detour.uninstall() {
            Ok(changed) => {
                trampoline.store(0, Ordering::Release);
                return Ok(changed);
            }
            Err(error) if error.is_transient() && Instant::now() < deadline => {
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn target_address(rva: usize) -> io::Result<NonNull<u8>> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: a null module name requests the current executable module.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let module = NonNull::new(module.cast::<u8>()).ok_or_else(io::Error::last_os_error)?;
    let address = (module.as_ptr() as usize)
        .checked_add(rva)
        .ok_or_else(|| io::Error::other("heartbeat priority target address overflow"))?;
    NonNull::new(address as *mut u8)
        .ok_or_else(|| io::Error::other("heartbeat priority target address is null"))
}

fn validate_entry(target: NonNull<u8>, rva: usize, expected: &[u8]) -> io::Result<()> {
    // SAFETY: supported executable validation establishes a readable target.
    let actual = unsafe { slice::from_raw_parts(target.as_ptr(), expected.len()) };
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "heartbeat priority entry mismatch at RVA 0x{rva:08X}: expected={expected:02X?} actual={actual:02X?}"
            ),
        ));
    }
    Ok(())
}

#[unsafe(naked)]
unsafe extern "thiscall" fn transport_submit_detour(
    _network: *mut core::ffi::c_void,
    _kind: usize,
    _buffer: *mut u8,
    _length: usize,
) {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "mov esi, ecx",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push esi",
        "call {prioritize}",
        "add esp, 16",
        "mov ecx, esi",
        "pop esi",
        "test eax, eax",
        "jz 1f",
        "lock dec dword ptr [{activity}]",
        "ret 12",
        "1:",
        "lock dec dword ptr [{activity}]",
        "jmp dword ptr [{trampoline}]",
        activity = sym HEARTBEAT_SUBMIT_ACTIVITY,
        prioritize = sym prioritize_submit,
        trampoline = sym HEARTBEAT_SUBMIT_TRAMPOLINE,
    );
}

#[unsafe(naked)]
unsafe extern "thiscall" fn transport_pop_detour(
    _queue: *mut core::ffi::c_void,
    _output: *mut usize,
) {
    core::arch::naked_asm!(
        "lock inc dword ptr [{activity}]",
        "push esi",
        "mov esi, ecx",
        "push dword ptr [esp + 8]",
        "push esi",
        "call {pop}",
        "add esp, 8",
        "mov ecx, esi",
        "pop esi",
        "test eax, eax",
        "jz 1f",
        "lock dec dword ptr [{activity}]",
        "ret 4",
        "1:",
        "lock dec dword ptr [{activity}]",
        "jmp dword ptr [{trampoline}]",
        activity = sym HEARTBEAT_POP_ACTIVITY,
        pop = sym pop_priority,
        trampoline = sym HEARTBEAT_POP_TRAMPOLINE,
    );
}

extern "C" fn prioritize_submit(network: usize, kind: usize, buffer: usize, length: usize) -> u32 {
    u32::from(
        panic::catch_unwind(|| {
            let mut prefix = [0_u8; 1];
            if length == 0
                || !read_memory(buffer, &mut prefix)
                || !heartbeat_priority::is_heartbeat(&prefix)
            {
                return false;
            }
            let Some(queue_address) = network.checked_add(TRANSPORT_QUEUE_OFFSET) else {
                FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                return false;
            };
            let Some(queue) = read_usize(queue_address) else {
                FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                return false;
            };
            let Some(semaphore_address) = network.checked_add(TRANSPORT_SEMAPHORE_OFFSET) else {
                FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                return false;
            };
            let Some(semaphore) = read_usize(semaphore_address) else {
                FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                return false;
            };
            let descriptor = [kind, buffer, length, 0, 0, 0];
            let prioritized = heartbeat_priority::push_and_signal(queue, descriptor, || {
                // SAFETY: the handle comes from the validated native network object.
                unsafe { ReleaseSemaphore(semaphore as HANDLE, 1, ptr::null_mut()) != 0 }
            });
            if prioritized {
                PRIORITIZED_COUNT.fetch_add(1, Ordering::Relaxed);
            } else {
                FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            prioritized
        })
        .unwrap_or(false),
    )
}

extern "C" fn pop_priority(queue: usize, output: *mut usize) -> u32 {
    u32::from(
        panic::catch_unwind(|| {
            if output.is_null() {
                return false;
            }
            let Some(descriptor) = heartbeat_priority::pop_for(queue) else {
                return false;
            };
            // SAFETY: the native pop ABI guarantees space for the six-word queue
            // descriptor, and descriptor is a readable array of exactly that size.
            unsafe {
                ptr::copy_nonoverlapping(descriptor.as_ptr(), output, DESCRIPTOR_WORDS);
            }
            true
        })
        .unwrap_or(false),
    )
}

fn read_usize(address: usize) -> Option<usize> {
    let mut value = 0_usize;
    // SAFETY: value is initialized and converted to its exact byte view.
    let bytes = unsafe {
        slice::from_raw_parts_mut((&mut value as *mut usize).cast::<u8>(), size_of::<usize>())
    };
    read_memory(address, bytes).then_some(value)
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

fn detour_error(error: DetourError) -> io::Error {
    io::Error::other(error)
}
