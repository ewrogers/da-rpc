use super::{StateReadError, StateWalker, add, indexed};
use crate::MemoryReader;

pub const EFFECT_SLOT_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawEffect {
    pub icon: u16,
    pub duration: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawEffects {
    pub effects: [Option<RawEffect>; EFFECT_SLOT_COUNT],
}

impl<M: MemoryReader> StateWalker<'_, M> {
    pub(super) fn capture_effects(
        &self,
        gui_back: u32,
    ) -> Result<Option<RawEffects>, StateReadError> {
        if gui_back == 0 {
            return Ok(None);
        }
        let pane = self.read_u32(add(gui_back, 0x4F94)?)?;
        if pane == 0 {
            return Ok(None);
        }

        let mut effects: [Option<RawEffect>; EFFECT_SLOT_COUNT] = [None; EFFECT_SLOT_COUNT];
        for index in 0..EFFECT_SLOT_COUNT {
            let icon = self.read_u16(indexed(pane, 0x190, 2, index)?)? as i16;
            let duration = self.read_u8(indexed(pane, 0x1A4, 1, index)?)?;
            if icon == -1 || duration == 0 {
                continue;
            }
            if icon < 0
                || duration > 6
                || effects[..index]
                    .iter()
                    .flatten()
                    .any(|effect| effect.icon == icon as u16)
            {
                return Err(StateReadError::InvalidCollection);
            }
            effects[index] = Some(RawEffect {
                icon: icon as u16,
                duration,
            });
        }
        Ok(Some(RawEffects { effects }))
    }
}
