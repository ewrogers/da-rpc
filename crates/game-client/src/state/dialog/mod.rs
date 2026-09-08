//! NPCServerItemMenuDialog selection layout verified in the supported client.

use super::{MemoryReader, StateReadError, StateWalker, add, indexed};

const MAX_DIALOG_ITEMS: usize = 512;
const ITEM_DIALOG_VTABLE_RVA: u32 = 0x0028_149C;
const ITEM_STRIDE: u32 = 0x218;

impl<M: MemoryReader> StateWalker<'_, M> {
    /// Recheck row and quantity immediately before the native item producer.
    /// Ordinary menus send names and have no quantity field. Pursuit 0x004B
    /// sends the selected record ID and an explicit quantity byte.
    pub fn dialog_item_selection_is_valid(
        &self,
        answer: u32,
        model: u32,
        index: u16,
        quantity: u8,
    ) -> Result<bool, StateReadError> {
        let count = self.dialog_item_count(answer, model)?;
        if index >= count || quantity == 0 {
            return Ok(false);
        }
        let pursuit = self.read_u16(add(model, 0x10)?)?;
        let items = self.read_u32(add(model, 0x14)?)?;
        if items == 0 || !items.is_multiple_of(4) {
            return Err(StateReadError::InvalidCollection);
        }
        let mut item = [0; ITEM_STRIDE as usize];
        self.read_bytes(
            indexed(items, 0, ITEM_STRIDE, usize::from(index))?,
            &mut item,
        )?;
        if pursuit == 0x004B {
            Ok(quantity <= item[0x0C])
        } else {
            Ok(quantity == 1 && item[0x0D..0x10D].contains(&0))
        }
    }

    fn dialog_item_count(&self, answer: u32, model: u32) -> Result<u16, StateReadError> {
        if answer == 0
            || model == 0
            || !answer.is_multiple_of(4)
            || !model.is_multiple_of(4)
            || self.read_u32(answer)? != self.module_address(ITEM_DIALOG_VTABLE_RVA)?
            || self.read_u32(add(answer, 0x638)?)? != model
        {
            return Err(StateReadError::InvalidCollection);
        }
        let count = self.read_u16(add(model, 0x12)?)?;
        if usize::from(count) > MAX_DIALOG_ITEMS {
            return Err(StateReadError::InvalidCollection);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
