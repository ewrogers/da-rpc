use super::{module_base, network, read};
use darpc_game_client::{EVENT_DISPATCHER_POINTER_RVA, FIELD_MAP_PANE_COL_RVA};
use darpc_protocol::{CommandFailure, FieldMapSelectionCommand};

const ENTRIES_OFFSET: usize = 0x64;
const COUNT_OFFSET: usize = 0x68;
const CAPACITY_OFFSET: usize = 0x6C;
const ENTRY_SIZE: usize = 0x0C;
const VISIBLE_OFFSET: usize = 0x130;
const REGISTRATION_OFFSET: usize = 0x188;
const REGISTERED: u8 = 0x02;
const MAX_PANES: i32 = 1_024;

pub(super) fn submit(command: FieldMapSelectionCommand) -> Result<(), CommandFailure> {
    let packet = crate::field_map::selection_packet(command)?;
    network::submit(&packet)
}

pub(crate) fn is_open() -> bool {
    find().is_ok()
}

fn find() -> Result<(), CommandFailure> {
    let module_base = module_base()?;
    let dispatcher = read::<u32>(address(module_base, EVENT_DISPATCHER_POINTER_RVA)?)
        .filter(|value| *value != 0)
        .ok_or(CommandFailure::InvalidState)? as usize;
    let entries = read::<u32>(address(dispatcher, ENTRIES_OFFSET)?)
        .ok_or(CommandFailure::InvalidState)? as usize;
    let count =
        read::<i32>(address(dispatcher, COUNT_OFFSET)?).ok_or(CommandFailure::InvalidState)?;
    let capacity =
        read::<i32>(address(dispatcher, CAPACITY_OFFSET)?).ok_or(CommandFailure::InvalidState)?;
    if count < 0 || count > capacity || capacity > MAX_PANES || (count != 0 && entries == 0) {
        return Err(CommandFailure::InvalidState);
    }
    for index in 0..count as usize {
        let entry_offset = index
            .checked_mul(ENTRY_SIZE)
            .ok_or(CommandFailure::InvalidState)?;
        let pane = read::<u32>(address(entries, entry_offset)?)
            .filter(|value| *value != 0)
            .map(|value| value as usize);
        let Some(pane) = pane else { continue };
        let Some(vtable) = read::<u32>(pane).map(|value| value as usize) else {
            continue;
        };
        let Some(locator) = vtable
            .checked_sub(4)
            .and_then(read::<u32>)
            .map(|value| value as usize)
        else {
            continue;
        };
        if locator != address(module_base, FIELD_MAP_PANE_COL_RVA)? {
            continue;
        }
        if read::<u8>(address(pane, VISIBLE_OFFSET)?) == Some(1)
            && read::<u8>(address(pane, REGISTRATION_OFFSET)?).unwrap_or(0) & REGISTERED != 0
        {
            return Ok(());
        }
    }
    Err(CommandFailure::InvalidState)
}

fn address(base: usize, offset: usize) -> Result<usize, CommandFailure> {
    base.checked_add(offset).ok_or(CommandFailure::InvalidState)
}
