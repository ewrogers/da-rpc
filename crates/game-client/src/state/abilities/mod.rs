use super::{MemoryReader, RawClientText, StateReadError, StateWalker, add, indexed};

pub const ABILITY_SLOT_COUNT: usize = 90;

const ABILITY_PANE_OFFSET: u32 = 0x190;
const SKILL_PANE_SIZE: usize = 0x1B8;
const SPELL_PANE_SIZE: usize = 0x12C;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSkillbook {
    pub skills: [Option<RawSkill>; ABILITY_SLOT_COUNT],
}

impl RawSkillbook {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            skills: [None; ABILITY_SLOT_COUNT],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSkill {
    pub slot: u8,
    pub icon: u16,
    pub name: RawClientText<128>,
    pub cooldown_started_at: u32,
    pub cooldown_ends_at: u32,
    pub cooldown_visual_active: bool,
    pub action_delay_active: bool,
    pub action_delay_duration_ms: u32,
    pub action_delay_timing_available: bool,
    pub name_suffix_left: i32,
    pub base_name_length: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSpellbook {
    pub spells: [Option<RawSpell>; ABILITY_SLOT_COUNT],
}

impl RawSpellbook {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            spells: [None; ABILITY_SLOT_COUNT],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSpell {
    pub slot: u8,
    pub icon: u16,
    pub name: RawClientText<128>,
    pub argument_type: u8,
    pub prompt: Option<RawClientText<128>>,
    pub cast_lines: u8,
    pub action_delay_active: bool,
    pub action_delay_started_at: u32,
    pub action_delay_ends_at: u32,
    pub action_delay_timing_available: bool,
    pub name_suffix_left: i32,
    pub base_name_length: i32,
}

impl<M: MemoryReader> StateWalker<'_, M> {
    pub(super) fn capture_abilities(
        &self,
        gui_back: u32,
        skillbook: &mut RawSkillbook,
        spellbook: &mut RawSpellbook,
    ) -> Result<(bool, bool), StateReadError> {
        skillbook.skills.fill(None);
        spellbook.spells.fill(None);
        if gui_back == 0 {
            return Ok((false, false));
        }
        let inventory = self.read_u32(add(gui_back, 0x4F8C)?)?;
        if inventory == 0 {
            return Ok((false, false));
        }
        let skills = self.read_u32(add(inventory, 0x224)?)?;
        let spells = self.read_u32(add(inventory, 0x228)?)?;
        let skillbook_available = self.capture_skillbook(skills, skillbook)?;
        let spellbook_available = self.capture_spellbook(spells, spellbook)?;
        if self.read_u32(add(gui_back, 0x4F8C)?)? != inventory
            || self.read_u32(add(inventory, 0x224)?)? != skills
            || self.read_u32(add(inventory, 0x228)?)? != spells
        {
            return Err(StateReadError::PointersChanged);
        }
        Ok((skillbook_available, spellbook_available))
    }

    fn capture_skillbook(
        &self,
        pane: u32,
        output: &mut RawSkillbook,
    ) -> Result<bool, StateReadError> {
        let Some(table) = self.pane_table(pane)? else {
            return Ok(false);
        };
        for index in 0..table.capacity {
            let pointer = self.read_u32(indexed(table.items, 0, 4, index)?)?;
            if pointer == 0 {
                continue;
            }
            let mut bytes = [0_u8; SKILL_PANE_SIZE];
            self.read_bytes(add(pointer, ABILITY_PANE_OFFSET)?, &mut bytes)?;
            let slot = bytes[0x182];
            let destination = ability_slot(&mut output.skills, slot)?;
            *destination = Some(RawSkill {
                slot,
                icon: u16::from_le_bytes([bytes[0], bytes[1]]),
                name: raw_text(&bytes[0x02..0x82])?,
                cooldown_started_at: u32_at(&bytes, 0x188),
                cooldown_ends_at: u32_at(&bytes, 0x18C),
                cooldown_visual_active: bytes[0x190] != 0,
                action_delay_active: bytes[0x192] != 0,
                action_delay_duration_ms: 0,
                action_delay_timing_available: false,
                name_suffix_left: i32_at(&bytes, 0x1AC),
                base_name_length: i32_at(&bytes, 0x1B4),
            });
        }
        self.validate_pane_table(pane, table)?;
        Ok(true)
    }

    fn capture_spellbook(
        &self,
        pane: u32,
        output: &mut RawSpellbook,
    ) -> Result<bool, StateReadError> {
        let Some(table) = self.pane_table(pane)? else {
            return Ok(false);
        };
        for index in 0..table.capacity {
            let pointer = self.read_u32(indexed(table.items, 0, 4, index)?)?;
            if pointer == 0 {
                continue;
            }
            let mut bytes = [0_u8; SPELL_PANE_SIZE];
            self.read_bytes(add(pointer, ABILITY_PANE_OFFSET)?, &mut bytes)?;
            let slot = bytes[0];
            let argument_type = bytes[0x04];
            let destination = ability_slot(&mut output.spells, slot)?;
            *destination = Some(RawSpell {
                slot,
                icon: u16::from_le_bytes([bytes[2], bytes[3]]),
                name: raw_text(&bytes[0x05..0x85])?,
                argument_type,
                prompt: if argument_type == 1 {
                    raw_optional_text(&bytes[0x85..0x105])?
                } else {
                    None
                },
                cast_lines: bytes[0x105],
                action_delay_active: bytes[0x107] != 0,
                action_delay_started_at: 0,
                action_delay_ends_at: 0,
                action_delay_timing_available: false,
                name_suffix_left: i32_at(&bytes, 0x120),
                base_name_length: i32_at(&bytes, 0x128),
            });
        }
        self.validate_pane_table(pane, table)?;
        Ok(true)
    }

    fn pane_table(&self, pane: u32) -> Result<Option<PaneTable>, StateReadError> {
        if pane == 0 {
            return Ok(None);
        }
        let capacity = self.read_i32(add(pane, 0x190)?)?;
        if !(0..=ABILITY_SLOT_COUNT as i32).contains(&capacity) {
            return Err(StateReadError::InvalidCollection);
        }
        let items = self.read_u32(add(pane, 0x194)?)?;
        if capacity > 0 && items == 0 {
            return Err(StateReadError::InvalidCollection);
        }
        Ok(Some(PaneTable {
            capacity: capacity as usize,
            items,
        }))
    }

    fn validate_pane_table(&self, pane: u32, table: PaneTable) -> Result<(), StateReadError> {
        if self.read_i32(add(pane, 0x190)?)? != table.capacity as i32
            || self.read_u32(add(pane, 0x194)?)? != table.items
        {
            return Err(StateReadError::PointersChanged);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct PaneTable {
    capacity: usize,
    items: u32,
}

fn ability_slot<T>(
    slots: &mut [Option<T>; ABILITY_SLOT_COUNT],
    slot: u8,
) -> Result<&mut Option<T>, StateReadError> {
    let index = usize::from(slot)
        .checked_sub(1)
        .filter(|index| *index < ABILITY_SLOT_COUNT)
        .ok_or(StateReadError::InvalidCollection)?;
    if slots[index].is_some() {
        return Err(StateReadError::InvalidCollection);
    }
    Ok(&mut slots[index])
}

fn raw_text<const N: usize>(bytes: &[u8]) -> Result<RawClientText<N>, StateReadError> {
    raw_optional_text(bytes)?.ok_or(StateReadError::InvalidCollection)
}

fn raw_optional_text<const N: usize>(
    bytes: &[u8],
) -> Result<Option<RawClientText<N>>, StateReadError> {
    let bytes: [u8; N] = bytes
        .try_into()
        .map_err(|_| StateReadError::InvalidCollection)?;
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(StateReadError::InvalidCollection)?;
    if length == 0 {
        return Ok(None);
    }
    Ok(Some(RawClientText {
        bytes,
        length: u8::try_from(length).map_err(|_| StateReadError::InvalidCollection)?,
    }))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed field"))
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed field"))
}
