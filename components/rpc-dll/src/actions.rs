pub(crate) mod movement;
mod skill;

use darpc_protocol::{CommandFailure, CommandKind, WalkTarget};
use std::{ffi::c_void, mem, ptr};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory, LibraryLoader::GetModuleHandleW,
    Threading::GetCurrentProcess,
};

pub(crate) fn execute(command: CommandKind) -> Result<(), CommandFailure> {
    match command {
        CommandKind::Diagnostic => Ok(()),
        CommandKind::Turn(direction) => movement::turn(direction),
        CommandKind::Walk(WalkTarget::Direction(direction)) => movement::walk(direction),
        CommandKind::Walk(WalkTarget::Destination { x, y }) => movement::walk_to(x, y),
        CommandKind::UseSkill(slot) => skill::use_skill(slot),
    }
}

fn module_base() -> Result<usize, CommandFailure> {
    // SAFETY: a null module name requests the executable module for the
    // current process and does not transfer ownership.
    let module = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    (module != 0)
        .then_some(module)
        .ok_or(CommandFailure::InvalidState)
}

fn read<T: Copy>(address: usize) -> Option<T> {
    let mut value = mem::MaybeUninit::<T>::uninit();
    let mut read = 0_usize;
    // SAFETY: the destination is valid for one T. ReadProcessMemory validates
    // the source range and reports failure rather than dereferencing it here.
    let succeeded = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            value.as_mut_ptr().cast(),
            mem::size_of::<T>(),
            &mut read,
        )
    };
    (succeeded != 0 && read == mem::size_of::<T>()).then(|| {
        // SAFETY: ReadProcessMemory initialized every byte of T on this branch,
        // and every T used here is an integer or pointer-sized plain value.
        unsafe { value.assume_init() }
    })
}
