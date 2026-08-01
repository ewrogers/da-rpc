mod error;
mod memory;
mod threads;

pub use error::DetourError;

use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Decoder, DecoderOptions, FlowControl, Instruction,
    InstructionBlock,
};
use memory::{CommitFault, ExecutableMemory, replace_code};
use std::{
    collections::BTreeSet,
    mem,
    ptr::NonNull,
    slice,
    sync::{Mutex, OnceLock, atomic::AtomicU32, atomic::Ordering},
};
use threads::SuspendedThreads;

const JUMP_LEN: usize = 5;
const MAX_PROLOGUE_LEN: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRange {
    start: usize,
    end: usize,
}

impl CodeRange {
    pub fn new(start: usize, length: usize) -> Result<Self, DetourError> {
        let end = start
            .checked_add(length)
            .filter(|end| *end > start)
            .ok_or(DetourError::InvalidCodeRange)?;
        Ok(Self { start, end })
    }

    pub fn contains(self, address: usize) -> bool {
        (self.start..self.end).contains(&address)
    }
}

#[repr(transparent)]
pub struct DetourActivity(AtomicU32);

impl DetourActivity {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    pub fn active_calls(&self) -> u32 {
        self.0.load(Ordering::Acquire)
    }
}

impl Default for DetourActivity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct DetourSpec {
    target: NonNull<u8>,
    detour: NonNull<u8>,
    detour_range: CodeRange,
    activity: &'static DetourActivity,
}

impl DetourSpec {
    pub fn new(
        target: NonNull<u8>,
        detour: NonNull<u8>,
        detour_range: CodeRange,
        activity: &'static DetourActivity,
    ) -> Result<Self, DetourError> {
        if !detour_range.contains(detour.as_ptr() as usize) {
            return Err(DetourError::InvalidCodeRange);
        }
        Ok(Self {
            target,
            detour,
            detour_range,
            activity,
        })
    }
}

#[must_use = "a prepared detour owns its executable trampoline"]
pub struct PreparedDetour {
    target: NonNull<u8>,
    detour_range: CodeRange,
    activity: &'static DetourActivity,
    target_range: CodeRange,
    trampoline_range: CodeRange,
    original: [u8; MAX_PROLOGUE_LEN],
    patch: [u8; MAX_PROLOGUE_LEN],
    patch_len: usize,
    trampoline: Option<ExecutableMemory>,
    reservation: Option<Reservation>,
}

impl PreparedDetour {
    /// Prepares a complete executable trampoline without changing target code.
    ///
    /// # Safety
    ///
    /// `spec.target` must identify readable executable x86 code for at least
    /// `MAX_PROLOGUE_LEN` bytes. `spec.detour` must have an ABI compatible with
    /// the target and remain executable while the detour is installed. The
    /// activity counter must be incremented before execution can leave the
    /// supplied detour range and decremented only immediately before return.
    /// Installation and removal must run on a management thread that is not
    /// participating in a target, detour, or trampoline call. The current
    /// thread cannot suspend itself and is therefore outside the instruction
    /// pointer checks applied during commit.
    pub unsafe fn prepare(spec: DetourSpec) -> Result<Self, DetourError> {
        let reservation = Reservation::acquire(spec.target.as_ptr() as usize)?;

        // SAFETY: upheld by the caller's target-code contract above.
        let source = unsafe { slice::from_raw_parts(spec.target.as_ptr(), MAX_PROLOGUE_LEN) };
        let target_address = spec.target.as_ptr() as usize;
        let (instructions, patch_len) = decode_prologue(source, target_address)?;
        let target_range = CodeRange::new(target_address, patch_len)?;

        let mut original = [0_u8; MAX_PROLOGUE_LEN];
        original[..patch_len].copy_from_slice(&source[..patch_len]);

        let mut trampoline = ExecutableMemory::allocate()?;
        let trampoline_address = trampoline.address().as_ptr() as usize;
        let relocated = BlockEncoder::encode(
            32,
            InstructionBlock::new(&instructions, trampoline_address as u64),
            BlockEncoderOptions::NONE,
        )
        .map_err(|error| DetourError::Relocation(error.to_string()))?;
        let mut trampoline_bytes = relocated.code_buffer;
        let return_address = target_address + patch_len;
        trampoline_bytes.extend_from_slice(&relative_jump(
            trampoline_address + trampoline_bytes.len(),
            return_address,
        ));
        trampoline.write(&trampoline_bytes)?;
        trampoline.seal(trampoline_bytes.len())?;
        let trampoline_range = CodeRange::new(trampoline_address, trampoline_bytes.len())?;

        let mut patch = [0x90_u8; MAX_PROLOGUE_LEN];
        patch[..JUMP_LEN].copy_from_slice(&relative_jump(
            target_address,
            spec.detour.as_ptr() as usize,
        ));

        Ok(Self {
            target: spec.target,
            detour_range: spec.detour_range,
            activity: spec.activity,
            target_range,
            trampoline_range,
            original,
            patch,
            patch_len,
            trampoline: Some(trampoline),
            reservation: Some(reservation),
        })
    }

    pub fn trampoline_address(&self) -> Result<usize, DetourError> {
        self.trampoline
            .as_ref()
            .map(|memory| memory.address().as_ptr() as usize)
            .ok_or(DetourError::InvalidState)
    }

    pub fn relocated_len(&self) -> usize {
        self.patch_len
    }

    pub fn install(&mut self) -> Result<InstalledDetour, DetourError> {
        let trampoline = self.trampoline.take().ok_or(DetourError::InvalidState)?;
        let reservation = self.reservation.take().ok_or(DetourError::InvalidState)?;

        let commit = (|| {
            let threads = SuspendedThreads::capture()?;
            let operation = (|| {
                threads.reject_instruction_pointers(&[self.target_range])?;
                // SAFETY: preparation validated the target range, every other
                // process thread is suspended outside it, and the byte slices
                // have the exact decoded instruction length.
                unsafe {
                    replace_code(
                        self.target,
                        &self.original[..self.patch_len],
                        &self.patch[..self.patch_len],
                        CommitFault::None,
                    )
                }
            })();
            threads.resume();
            operation
        })();

        if let Err(error) = commit {
            self.trampoline = Some(trampoline);
            self.reservation = Some(reservation);
            return Err(error);
        }

        Ok(InstalledDetour {
            target: self.target,
            detour_range: self.detour_range,
            activity: self.activity,
            target_range: self.target_range,
            trampoline_range: self.trampoline_range,
            original: self.original,
            patch: self.patch,
            patch_len: self.patch_len,
            trampoline: Some(trampoline),
            reservation: Some(reservation),
            installed: true,
        })
    }
}

#[must_use = "an installed detour must be explicitly uninstalled before code unload"]
pub struct InstalledDetour {
    target: NonNull<u8>,
    detour_range: CodeRange,
    activity: &'static DetourActivity,
    target_range: CodeRange,
    trampoline_range: CodeRange,
    original: [u8; MAX_PROLOGUE_LEN],
    patch: [u8; MAX_PROLOGUE_LEN],
    patch_len: usize,
    trampoline: Option<ExecutableMemory>,
    reservation: Option<Reservation>,
    installed: bool,
}

impl InstalledDetour {
    pub fn is_installed(&self) -> bool {
        self.installed
    }

    pub fn uninstall(&mut self) -> Result<bool, DetourError> {
        if !self.installed {
            return Ok(false);
        }

        {
            let threads = SuspendedThreads::capture()?;
            let operation = (|| {
                let active_calls = self.activity.active_calls();
                if active_calls != 0 {
                    return Err(DetourError::ActiveDetourCalls {
                        count: active_calls,
                    });
                }
                threads.reject_instruction_pointers(&[
                    self.target_range,
                    self.detour_range,
                    self.trampoline_range,
                ])?;
                // SAFETY: all other threads are suspended outside the target,
                // detour, and trampoline ranges, and no tracked detour call is
                // active. The target therefore cannot execute during
                // restoration.
                unsafe {
                    replace_code(
                        self.target,
                        &self.patch[..self.patch_len],
                        &self.original[..self.patch_len],
                        CommitFault::None,
                    )
                }
            })();
            threads.resume();
            operation?;
        }

        self.installed = false;
        self.reservation.take();
        Ok(true)
    }
}

impl Drop for InstalledDetour {
    fn drop(&mut self) {
        if self.installed {
            // Freeing either resource while target code still references the
            // trampoline would make an accidental drop immediately unsafe.
            // Explicit shutdown is mandatory; leaking preserves process
            // integrity so the owning module can refuse to unload.
            if let Some(trampoline) = self.trampoline.take() {
                mem::forget(trampoline);
            }
            if let Some(reservation) = self.reservation.take() {
                mem::forget(reservation);
            }
        }
    }
}

fn decode_prologue(
    source: &[u8],
    target_address: usize,
) -> Result<(Vec<Instruction>, usize), DetourError> {
    let mut decoder = Decoder::with_ip(32, source, target_address as u64, DecoderOptions::NONE);
    let mut instructions = Vec::with_capacity(4);
    let mut length = 0;

    while length < JUMP_LEN {
        if !decoder.can_decode() {
            return Err(DetourError::PrologueTooLong);
        }
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(DetourError::InvalidInstruction {
                address: target_address + length,
            });
        }
        let next_length = length
            .checked_add(instruction.len())
            .filter(|length| *length <= MAX_PROLOGUE_LEN)
            .ok_or(DetourError::PrologueTooLong)?;
        if next_length < JUMP_LEN
            && matches!(
                instruction.flow_control(),
                FlowControl::UnconditionalBranch
                    | FlowControl::IndirectBranch
                    | FlowControl::Return
                    | FlowControl::Interrupt
                    | FlowControl::XbeginXabortXend
                    | FlowControl::Exception
            )
        {
            return Err(DetourError::EarlyTerminatingInstruction {
                address: target_address + length,
            });
        }
        length = next_length;
        instructions.push(instruction);
    }

    Ok((instructions, length))
}

fn relative_jump(instruction_address: usize, destination: usize) -> [u8; JUMP_LEN] {
    let next = (instruction_address as u32).wrapping_add(JUMP_LEN as u32);
    let displacement = (destination as u32).wrapping_sub(next);
    let mut jump = [0_u8; JUMP_LEN];
    jump[0] = 0xE9;
    jump[1..].copy_from_slice(&displacement.to_le_bytes());
    jump
}

struct Reservation {
    target: usize,
}

impl Reservation {
    fn acquire(target: usize) -> Result<Self, DetourError> {
        let mut targets = reservations()
            .lock()
            .map_err(|_| DetourError::RegistryPoisoned)?;
        if !targets.insert(target) {
            return Err(DetourError::AlreadyReserved { target });
        }
        Ok(Self { target })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        match reservations().lock() {
            Ok(mut targets) => {
                targets.remove(&self.target);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&self.target);
            }
        }
    }
}

fn reservations() -> &'static Mutex<BTreeSet<usize>> {
    static RESERVATIONS: OnceLock<Mutex<BTreeSet<usize>>> = OnceLock::new();
    RESERVATIONS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

#[cfg(test)]
mod tests {
    use super::{
        CodeRange, CommitFault, DetourActivity, DetourError, DetourSpec, ExecutableMemory,
        PreparedDetour, replace_code,
    };
    use std::{ptr::NonNull, slice};

    static ACTIVITY: DetourActivity = DetourActivity::new();

    #[test]
    fn code_ranges_are_checked_and_half_open() {
        assert!(CodeRange::new(10, 0).is_err());
        assert!(CodeRange::new(usize::MAX, 2).is_err());
        let range = CodeRange::new(10, 3).expect("valid range");
        assert!(range.contains(10));
        assert!(range.contains(12));
        assert!(!range.contains(13));
    }

    #[test]
    fn failed_commit_restores_original_code() {
        let mut memory = ExecutableMemory::allocate().expect("allocate code");
        let original = [0xB8, 0x2A, 0, 0, 0, 0xC3];
        memory.write(&original).expect("write code");
        memory.seal(original.len()).expect("seal code");
        let replacement = [0xB8, 0x63, 0, 0, 0, 0xC3];

        // SAFETY: memory is a live executable allocation containing exactly
        // the expected bytes and no other thread can execute this test region.
        let error = unsafe {
            replace_code(
                memory.address(),
                &original,
                &replacement,
                CommitFault::AfterWrite,
            )
        }
        .expect_err("injected commit unexpectedly succeeded");
        assert!(matches!(error, DetourError::CommitFailed { .. }));

        // SAFETY: the allocation remains readable for original.len() bytes.
        let actual = unsafe { slice::from_raw_parts(memory.address().as_ptr(), original.len()) };
        assert_eq!(actual, original);
    }

    #[test]
    fn one_target_has_one_reservation() {
        let mut memory = ExecutableMemory::allocate().expect("allocate code");
        let mut code = [0xCC_u8; 64];
        code[..6].copy_from_slice(&[0xB8, 0x2A, 0, 0, 0, 0xC3]);
        code[32] = 0xC3;
        memory.write(&code).expect("write code");
        memory.seal(code.len()).expect("seal code");

        let target = memory.address();
        let detour =
            NonNull::new((target.as_ptr() as usize + 32) as *mut u8).expect("non-null detour");
        let spec = DetourSpec::new(
            target,
            detour,
            CodeRange::new(detour.as_ptr() as usize, 1).expect("detour range"),
            &ACTIVITY,
        )
        .expect("valid spec");

        // SAFETY: the owned allocation contains readable executable x86 code
        // at both declared addresses for the lifetime of both preparations.
        let _prepared = unsafe { PreparedDetour::prepare(spec) }.expect("first reservation");
        // SAFETY: same as above; this call is expected to stop at reservation.
        let error = unsafe { PreparedDetour::prepare(spec) }
            .err()
            .expect("second reservation unexpectedly succeeded");
        assert!(matches!(error, DetourError::AlreadyReserved { .. }));
    }

    #[test]
    fn short_function_is_not_extended_into_adjacent_code() {
        let mut memory = ExecutableMemory::allocate().expect("allocate code");
        let mut code = [0xCC_u8; 64];
        code[0] = 0xC3;
        code[32] = 0xC3;
        memory.write(&code).expect("write code");
        memory.seal(code.len()).expect("seal code");

        let target = memory.address();
        let detour =
            NonNull::new((target.as_ptr() as usize + 32) as *mut u8).expect("non-null detour");
        let spec = DetourSpec::new(
            target,
            detour,
            CodeRange::new(detour.as_ptr() as usize, 1).expect("detour range"),
            &ACTIVITY,
        )
        .expect("valid spec");

        // SAFETY: the allocation is readable and executable for the complete
        // decoder window; preparation must reject the one-byte function.
        let error = unsafe { PreparedDetour::prepare(spec) }
            .err()
            .expect("short function unexpectedly prepared");
        assert!(matches!(
            error,
            DetourError::EarlyTerminatingInstruction { .. }
        ));
    }
}
