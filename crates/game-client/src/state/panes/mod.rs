use super::{MemoryReader, StateReadError, StateWalker, add, indexed};

pub(super) const EVENT_DISPATCHER_RVA: u32 = 0x002D_9220;

pub(super) const RECONNECT_DIALOG_VTABLE_RVA: u32 = 0x0028_2BF4;
const EVENT_ENTRIES_OFFSET: u32 = 0x64;
const EVENT_COUNT_OFFSET: u32 = 0x68;
const EVENT_CAPACITY_OFFSET: u32 = 0x6C;
const EVENT_ENTRY_SIZE: u32 = 0x0C;
const PANE_VISIBLE_OFFSET: u32 = 0x130;
const PANE_REGISTRATION_FLAGS_OFFSET: u32 = 0x188;
const PANE_REGISTERED_FLAG: u8 = 0x02;
const MAX_EVENT_PANES: usize = 1_024;

impl<M: MemoryReader> StateWalker<'_, M> {
    pub(super) fn reconnect_dialog_is_open(&self, dispatcher: u32) -> Result<bool, StateReadError> {
        if dispatcher == 0 {
            return Ok(false);
        }

        let entries = self.read_u32(add(dispatcher, EVENT_ENTRIES_OFFSET)?)?;
        let count = self.read_i32(add(dispatcher, EVENT_COUNT_OFFSET)?)?;
        let capacity = self.read_i32(add(dispatcher, EVENT_CAPACITY_OFFSET)?)?;
        let count = validated_count(entries, count, capacity)?;
        let wanted_vtable = self.module_address(RECONNECT_DIALOG_VTABLE_RVA)?;

        let mut open = false;
        for index in 0..count {
            let pane = self.read_u32(indexed(entries, 0, EVENT_ENTRY_SIZE, index)?)?;
            if pane == 0 || self.read_u32(pane)? != wanted_vtable {
                continue;
            }
            let visible = self.read_u8(add(pane, PANE_VISIBLE_OFFSET)?)? != 0;
            let registered = self.read_u8(add(pane, PANE_REGISTRATION_FLAGS_OFFSET)?)?
                & PANE_REGISTERED_FLAG
                != 0;
            if visible && registered {
                open = true;
                break;
            }
        }

        if self.read_u32(add(dispatcher, EVENT_ENTRIES_OFFSET)?)? != entries
            || self.read_i32(add(dispatcher, EVENT_COUNT_OFFSET)?)? != count as i32
            || self.read_i32(add(dispatcher, EVENT_CAPACITY_OFFSET)?)? != capacity
        {
            return Err(StateReadError::PointersChanged);
        }
        Ok(open)
    }
}

fn validated_count(entries: u32, count: i32, capacity: i32) -> Result<usize, StateReadError> {
    let count = usize::try_from(count).map_err(|_| StateReadError::InvalidPaneList)?;
    let capacity = usize::try_from(capacity).map_err(|_| StateReadError::InvalidPaneList)?;
    if count > capacity || capacity > MAX_EVENT_PANES || (count != 0 && entries == 0) {
        return Err(StateReadError::InvalidPaneList);
    }
    Ok(count)
}
