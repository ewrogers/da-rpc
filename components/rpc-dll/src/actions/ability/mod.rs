use super::{module_base, read};
use crate::process_memory::ProcessValue;
use darpc_game_client::{ABILITY_SLOT_COUNT, GUI_BACK_PANE_GET_RVA};
use darpc_protocol::CommandFailure;
use std::{ffi::c_void, mem, ptr::NonNull};

const BOOKS_OFFSET: usize = 0x4F8C;
const SKILL_PANE_OFFSET: usize = 0x224;
const SPELL_PANE_OFFSET: usize = 0x228;
const CAPACITY_OFFSET: usize = 0x190;
const ITEMS_OFFSET: usize = 0x194;

type GuiBackPaneGetFn = unsafe extern "C" fn() -> *mut c_void;

#[derive(Clone, Copy)]
pub(super) enum AbilityKind {
    Skill,
    Spell,
}

impl AbilityKind {
    const fn pane_offset(self) -> usize {
        match self {
            Self::Skill => SKILL_PANE_OFFSET,
            Self::Spell => SPELL_PANE_OFFSET,
        }
    }

    const fn slot_offset(self) -> usize {
        match self {
            Self::Skill => 0x312,
            Self::Spell => 0x190,
        }
    }

    const fn invalid(self) -> CommandFailure {
        match self {
            Self::Skill => CommandFailure::InvalidSkill,
            Self::Spell => CommandFailure::InvalidSpell,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct AbilityEntry(NonNull<c_void>);

impl AbilityEntry {
    pub(super) const fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }

    pub(super) fn read<T: ProcessValue>(self, offset: usize) -> Result<T, CommandFailure> {
        read(add(self.0.as_ptr() as usize, offset)?).ok_or(CommandFailure::InvalidState)
    }

    pub(super) fn address(self, offset: usize) -> Result<usize, CommandFailure> {
        add(self.0.as_ptr() as usize, offset)
    }
}

pub(super) fn resolve(
    slot: u8,
    kind: AbilityKind,
) -> Result<(usize, AbilityEntry), CommandFailure> {
    let module = module_base()?;
    // SAFETY: exact executable validation fixes the RVA and cdecl ABI. The
    // null-safe accessor returns the current complete GUIBackPane.
    let gui = NonNull::new(unsafe { gui_get_fn(module)() }).ok_or(CommandFailure::InvalidState)?;
    let books = read_pointer(add(gui.as_ptr() as usize, BOOKS_OFFSET)?)?
        .ok_or(CommandFailure::InvalidState)?;
    let pane = read_pointer(add(books.as_ptr() as usize, kind.pane_offset())?)?
        .ok_or(CommandFailure::InvalidState)?;
    let capacity = read::<i32>(add(pane.as_ptr() as usize, CAPACITY_OFFSET)?)
        .ok_or(CommandFailure::InvalidState)?;
    if !(0..=ABILITY_SLOT_COUNT as i32).contains(&capacity) || i32::from(slot) > capacity {
        return Err(kind.invalid());
    }
    let items = read_pointer(add(pane.as_ptr() as usize, ITEMS_OFFSET)?)?
        .ok_or(CommandFailure::InvalidState)?;
    let index = usize::from(slot.checked_sub(1).ok_or_else(|| kind.invalid())?);
    let entry = read_pointer(add(
        items.as_ptr() as usize,
        index
            .checked_mul(mem::size_of::<u32>())
            .ok_or(CommandFailure::Internal)?,
    )?)?
    .ok_or_else(|| kind.invalid())?;
    let entry = AbilityEntry(entry);
    if entry.read::<u8>(kind.slot_offset())? != slot {
        return Err(kind.invalid());
    }
    Ok((module, entry))
}

fn read_pointer(address: usize) -> Result<Option<NonNull<c_void>>, CommandFailure> {
    read::<u32>(address)
        .map(|value| NonNull::new(value as usize as *mut c_void))
        .ok_or(CommandFailure::InvalidState)
}

fn add(base: usize, offset: usize) -> Result<usize, CommandFailure> {
    base.checked_add(offset).ok_or(CommandFailure::Internal)
}

fn gui_get_fn(module: usize) -> GuiBackPaneGetFn {
    // SAFETY: the supported executable fingerprint fixes the function RVA and
    // its x86 cdecl signature.
    unsafe { mem::transmute(module + GUI_BACK_PANE_GET_RVA) }
}
