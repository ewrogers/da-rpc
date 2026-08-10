use super::ability::{self, AbilityKind};
use darpc_game_client::SKILL_ACTIVATE_RVA;
use darpc_protocol::{CommandFailure, SkillSlot};
use std::{ffi::c_void, mem};

const ACTION_DELAY_OFFSET: usize = 0x322;
const BLOCKED_OFFSET: usize = 0x323;

type SkillActivateFn = unsafe extern "thiscall" fn(*mut c_void);

pub(super) fn use_skill(slot: SkillSlot) -> Result<(), CommandFailure> {
    let (module, entry) = ability::resolve(slot.get(), AbilityKind::Skill)?;
    if entry.read::<u8>(ACTION_DELAY_OFFSET)? != 0 || entry.read::<u8>(BLOCKED_OFFSET)? != 0 {
        return Err(CommandFailure::Rejected);
    }

    // SAFETY: exact executable validation fixes the RVA and x86 thiscall ABI.
    // The entry was resolved from the live pane tree and execution is on the
    // client main thread. This routine does not select or expose a UI pane.
    unsafe { activate_fn(module)(entry.as_ptr()) };
    Ok(())
}

fn activate_fn(module: usize) -> SkillActivateFn {
    // SAFETY: the supported executable fingerprint fixes the function RVA and
    // its x86 thiscall signature.
    unsafe { mem::transmute(module + SKILL_ACTIVATE_RVA) }
}
