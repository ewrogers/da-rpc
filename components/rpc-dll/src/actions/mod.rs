mod ability;
mod chant;
pub(crate) mod dialog;
pub(crate) mod exchange;
pub(crate) mod group;
mod interaction;
pub(crate) mod movement;
pub(crate) mod network;
mod skill;
pub(crate) mod spell;

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
        CommandKind::CastSpell(cast) => spell::cast(cast),
        CommandKind::UseItem(slot) => interaction::use_item(slot),
        CommandKind::DropItem(transfer) => interaction::drop_item(transfer),
        CommandKind::DropGold(transfer) => interaction::drop_gold(transfer),
        CommandKind::GiveItem(transfer) => interaction::give_item(transfer),
        CommandKind::GiveGold(transfer) => interaction::give_gold(transfer),
        CommandKind::SwapSlots(swap) => interaction::swap_slots(swap),
        CommandKind::PickupItem(position) => interaction::pickup_item(position),
        CommandKind::Unequip(slot) => interaction::unequip(slot),
        CommandKind::Emote(code) => interaction::emote(code),
        CommandKind::Interact(id) => movement::interact(id),
        CommandKind::Dialog(command) => dialog::submit(command),
        CommandKind::Group(command) => group::submit(command),
        CommandKind::Who => Err(CommandFailure::Internal),
        CommandKind::Exchange(command) => exchange::submit(command),
        CommandKind::Chant(text) => chant::submit(text),
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
