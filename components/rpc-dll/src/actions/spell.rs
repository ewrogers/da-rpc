use super::{
    ability::{self, AbilityEntry, AbilityKind},
    module_base, read,
};
use darpc_game_client::{
    SPELL_DELAY_ACTIVE_OFFSET, SPELL_DELAY_CONTROL_PANE_GET_RVA, SPELL_DENIED_RVA,
    SPELL_NO_ARGS_RVA, SPELL_START_RVA, SPELL_TARGET_RVA,
};
use darpc_protocol::{CommandFailure, SpellArguments, SpellCast, SpellTarget};
use std::{ffi::c_void, mem, ptr::NonNull};

const ICON_OFFSET: usize = 0x192;
const ARGUMENT_TYPE_OFFSET: usize = 0x194;
const NAME_OFFSET: usize = 0x195;
const CAST_LINES_OFFSET: usize = 0x295;
const ACTION_DELAY_OFFSET: usize = 0x297;
const TOTAL_LINES_OFFSET: usize = 0x190;
const CURRENT_LINE_OFFSET: usize = 0x191;
const QUEUED_BODY_OFFSET: usize = 0xB92;
const QUEUED_BODY_LENGTH_OFFSET: usize = 0x8C92;
const USE_SPELL_OPCODE: u8 = 0x0F;

type SpellDelayGetFn = unsafe extern "C" fn() -> *mut c_void;
type SpellDeniedFn = unsafe extern "thiscall" fn(*mut c_void) -> u8;
type SpellNoArgsFn = unsafe extern "thiscall" fn(*mut c_void);
type SpellTargetFn = unsafe extern "thiscall" fn(*mut c_void, u32, u16, u16);
type SpellStartFn = unsafe extern "thiscall" fn(*mut c_void, *const u8, i16, u16, u8, *const u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CastingState {
    pub(crate) active: bool,
    pub(crate) slot: Option<u8>,
    pub(crate) total_lines: u8,
    pub(crate) current_line: u8,
}

pub(super) fn cast(cast: SpellCast) -> Result<(), CommandFailure> {
    let (module, entry) = ability::resolve(cast.slot.get(), AbilityKind::Spell)?;
    if entry.read::<u8>(ACTION_DELAY_OFFSET)? != 0 {
        return Err(CommandFailure::Rejected);
    }
    let control = control(module)?;
    // SAFETY: executable validation fixes the x86 thiscall ABI and the entry
    // is a live SpellInvItemPane resolved on the client main thread.
    if unsafe { denied_fn(module)(entry.as_ptr()) } != 0 {
        return Err(CommandFailure::Rejected);
    }

    let actual_type = entry.read::<u8>(ARGUMENT_TYPE_OFFSET)?;
    match cast.arguments {
        SpellArguments::None if actual_type == 5 => {
            // SAFETY: the live entry, controller state, action delay, denied
            // list, executable RVA, and main-thread execution were validated.
            unsafe { no_args_fn(module)(entry.as_ptr()) };
        }
        SpellArguments::Target(target) if actual_type == 2 => {
            let (id, x, y) = resolve_target(target)?;
            // SAFETY: the target coordinates were validated and resolved on
            // the main thread. The unusual Y-before-X ABI matches the client.
            unsafe { target_fn(module)(entry.as_ptr(), id, y, x) };
        }
        SpellArguments::None if actual_type == 2 => {
            let (id, position) =
                crate::state::self_target().ok_or(CommandFailure::InvalidTarget)?;
            let x = u16::try_from(position.x).map_err(|_| CommandFailure::InvalidTarget)?;
            let y = u16::try_from(position.y).map_err(|_| CommandFailure::InvalidTarget)?;
            // SAFETY: the cached self ID and coordinates came from the current
            // main-thread state snapshot. The unusual Y-before-X ABI matches
            // the client.
            unsafe { target_fn(module)(entry.as_ptr(), id, y, x) };
        }
        SpellArguments::Input(input) if actual_type == 1 => {
            let mut body = [0_u8; 2 + darpc_protocol::MAX_SPELL_INPUT_LEN];
            body[0] = USE_SPELL_OPCODE;
            body[1] = cast.slot.get();
            let input = input.as_bytes();
            body[2..2 + input.len()].copy_from_slice(input);
            start_body(module, control, entry, &body[..2 + input.len()])?;
        }
        _ => return Err(CommandFailure::InvalidArguments),
    }
    Ok(())
}

pub(crate) fn casting_state() -> Option<CastingState> {
    let module = module_base().ok()?;
    let control = control(module).ok()?;
    let length = read_field::<u16>(control, QUEUED_BODY_LENGTH_OFFSET).ok()?;
    let slot =
        if length >= 2 && read_field::<u8>(control, QUEUED_BODY_OFFSET).ok()? == USE_SPELL_OPCODE {
            Some(read_field::<u8>(control, QUEUED_BODY_OFFSET + 1).ok()?)
        } else {
            None
        };
    Some(CastingState {
        active: read_field::<u8>(control, SPELL_DELAY_ACTIVE_OFFSET).ok()? != 0,
        slot,
        total_lines: read_field::<u8>(control, TOTAL_LINES_OFFSET).ok()?,
        current_line: read_field::<u8>(control, CURRENT_LINE_OFFSET).ok()?,
    })
}

pub(crate) fn argument_type(slot: u8) -> Option<u8> {
    let (_, entry) = ability::resolve(slot, AbilityKind::Spell).ok()?;
    entry.read(ARGUMENT_TYPE_OFFSET).ok()
}

fn resolve_target(target: SpellTarget) -> Result<(u32, u16, u16), CommandFailure> {
    let (id, x, y) = match target {
        SpellTarget::Object(id) => {
            let position =
                crate::state::target_position(id.get()).ok_or(CommandFailure::InvalidTarget)?;
            (id.get(), position.x, position.y)
        }
        SpellTarget::Tile { x, y } => {
            if !crate::state::valid_tile(x, y) {
                return Err(CommandFailure::InvalidTarget);
            }
            (0, x, y)
        }
    };
    Ok((
        id,
        u16::try_from(x).map_err(|_| CommandFailure::InvalidTarget)?,
        u16::try_from(y).map_err(|_| CommandFailure::InvalidTarget)?,
    ))
}

fn start_body(
    module: usize,
    control: NonNull<c_void>,
    entry: AbilityEntry,
    body: &[u8],
) -> Result<(), CommandFailure> {
    let length = i16::try_from(body.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let icon = entry.read::<u16>(ICON_OFFSET)?;
    let cast_lines = entry.read::<u8>(CAST_LINES_OFFSET)?;
    let name = entry.address(NAME_OFFSET)? as *const u8;
    // SAFETY: body and the live entry name remain valid for this synchronous
    // call. The controller and exact x86 thiscall ABI were validated above.
    unsafe {
        start_fn(module)(
            control.as_ptr(),
            body.as_ptr(),
            length,
            icon,
            cast_lines,
            name,
        )
    };
    Ok(())
}

fn control(module: usize) -> Result<NonNull<c_void>, CommandFailure> {
    // SAFETY: executable validation fixes the RVA and cdecl ABI.
    NonNull::new(unsafe { get_control_fn(module)() }).ok_or(CommandFailure::InvalidState)
}

fn read_field<T: Copy>(base: NonNull<c_void>, offset: usize) -> Result<T, CommandFailure> {
    let address = (base.as_ptr() as usize)
        .checked_add(offset)
        .ok_or(CommandFailure::Internal)?;
    read(address).ok_or(CommandFailure::InvalidState)
}

fn get_control_fn(module: usize) -> SpellDelayGetFn {
    // SAFETY: the supported executable fixes this x86 function address.
    unsafe { mem::transmute(module + SPELL_DELAY_CONTROL_PANE_GET_RVA) }
}

fn denied_fn(module: usize) -> SpellDeniedFn {
    // SAFETY: the supported executable fixes this x86 function address.
    unsafe { mem::transmute(module + SPELL_DENIED_RVA) }
}

fn no_args_fn(module: usize) -> SpellNoArgsFn {
    // SAFETY: the supported executable fixes this x86 function address.
    unsafe { mem::transmute(module + SPELL_NO_ARGS_RVA) }
}

fn target_fn(module: usize) -> SpellTargetFn {
    // SAFETY: the supported executable fixes this x86 function address.
    unsafe { mem::transmute(module + SPELL_TARGET_RVA) }
}

fn start_fn(module: usize) -> SpellStartFn {
    // SAFETY: the supported executable fixes this x86 function address.
    unsafe { mem::transmute(module + SPELL_START_RVA) }
}
