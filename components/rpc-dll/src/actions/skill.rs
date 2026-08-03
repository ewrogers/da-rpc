use super::{module_base, read};
use darpc_game_client::{ABILITY_SLOT_COUNT, GUI_BACK_PANE_GET_RVA, SKILL_ACTIVATE_RVA};
use darpc_protocol::{CommandFailure, SkillSlot};
use std::{ffi::c_void, mem, ptr::NonNull};

const SKILL_BOOK_OFFSET: usize = 0x4F8C;
const SKILL_PANE_OFFSET: usize = 0x224;
const CAPACITY_OFFSET: usize = 0x190;
const ITEMS_OFFSET: usize = 0x194;
const ENTRY_SLOT_OFFSET: usize = 0x312;
const ACTION_DELAY_OFFSET: usize = 0x322;
const BLOCKED_OFFSET: usize = 0x323;

type GuiBackPaneGetFn = unsafe extern "C" fn() -> *mut c_void;
type SkillActivateFn = unsafe extern "thiscall" fn(*mut c_void);

pub(super) fn use_skill(slot: SkillSlot) -> Result<(), CommandFailure> {
    let module = module_base()?;
    let entry = resolve_entry(module, slot)?;
    if read_field::<u8>(entry, ACTION_DELAY_OFFSET)? != 0
        || read_field::<u8>(entry, BLOCKED_OFFSET)? != 0
    {
        return Err(CommandFailure::Rejected);
    }

    // SAFETY: exact executable validation fixes the RVA and x86 thiscall ABI.
    // The entry was resolved from the live pane tree and execution is on the
    // client main thread. This routine does not select or expose a UI pane.
    unsafe { activate_fn(module)(entry.as_ptr()) };
    Ok(())
}

fn resolve_entry(module: usize, slot: SkillSlot) -> Result<NonNull<c_void>, CommandFailure> {
    // SAFETY: exact executable validation fixes the RVA and cdecl ABI. The
    // null-safe accessor returns the current complete GUIBackPane.
    let gui = NonNull::new(unsafe { gui_get_fn(module)() }).ok_or(CommandFailure::InvalidState)?;
    let books = read_pointer(add(gui.as_ptr() as usize, SKILL_BOOK_OFFSET)?)?
        .ok_or(CommandFailure::InvalidState)?;
    let skills = read_pointer(add(books.as_ptr() as usize, SKILL_PANE_OFFSET)?)?
        .ok_or(CommandFailure::InvalidState)?;
    let capacity = read_field::<i32>(skills, CAPACITY_OFFSET)?;
    if !(0..=ABILITY_SLOT_COUNT as i32).contains(&capacity) || i32::from(slot.get()) > capacity {
        return Err(CommandFailure::InvalidSkill);
    }
    let items = read_pointer(add(skills.as_ptr() as usize, ITEMS_OFFSET)?)?
        .ok_or(CommandFailure::InvalidState)?;
    let index = usize::from(slot.get() - 1);
    let entry_address = add(items.as_ptr() as usize, index * mem::size_of::<u32>())?;
    let entry = read_pointer(entry_address)?.ok_or(CommandFailure::InvalidSkill)?;
    if read_field::<u8>(entry, ENTRY_SLOT_OFFSET)? != slot.get() {
        return Err(CommandFailure::InvalidSkill);
    }
    Ok(entry)
}

fn read_pointer(address: usize) -> Result<Option<NonNull<c_void>>, CommandFailure> {
    read::<u32>(address)
        .map(|value| NonNull::new(value as usize as *mut c_void))
        .ok_or(CommandFailure::InvalidState)
}

fn read_field<T: Copy>(base: NonNull<c_void>, offset: usize) -> Result<T, CommandFailure> {
    read(add(base.as_ptr() as usize, offset)?).ok_or(CommandFailure::InvalidState)
}

fn add(base: usize, offset: usize) -> Result<usize, CommandFailure> {
    base.checked_add(offset).ok_or(CommandFailure::Internal)
}

fn gui_get_fn(module: usize) -> GuiBackPaneGetFn {
    // SAFETY: the supported executable fingerprint fixes the function RVA and
    // its x86 cdecl signature.
    unsafe { mem::transmute(module + GUI_BACK_PANE_GET_RVA) }
}

fn activate_fn(module: usize) -> SkillActivateFn {
    // SAFETY: the supported executable fingerprint fixes the function RVA and
    // its x86 thiscall signature.
    unsafe { mem::transmute(module + SKILL_ACTIVATE_RVA) }
}
