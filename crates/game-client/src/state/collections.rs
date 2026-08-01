use super::{MemoryReader, RawClientText, StateReadError, StateWalker, add, indexed};

pub const INVENTORY_SLOT_COUNT: usize = 60;
pub const EQUIPMENT_SLOT_COUNT: usize = 18;

const INVENTORY_POINTERS_OFFSET: u32 = 0x1A0;
const INVENTORY_ENTRY_OFFSET: u32 = 0x190;
const INVENTORY_ENTRY_SIZE: usize = 0xB8;
const INVENTORY_GOLD_SLOT: usize = 60;
const EQUIPMENT_SNAPSHOT_OFFSET: u32 = 0x111C;
const EQUIPMENT_SNAPSHOT_SIZE: usize = 0x9C8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawInventory {
    pub items: [Option<RawInventoryItem>; INVENTORY_SLOT_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawInventoryItem {
    pub slot: u8,
    pub sprite: u16,
    pub dye_color: u8,
    pub name: RawClientText<128>,
    pub quantity: u32,
    pub can_stack: bool,
    pub durability: u32,
    pub max_durability: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawEquipment {
    pub items: [Option<RawEquipmentItem>; EQUIPMENT_SLOT_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawEquipmentItem {
    pub slot: u8,
    pub sprite: u16,
    pub dye_color: u8,
    pub name: RawClientText<128>,
    pub durability: u32,
    pub max_durability: u32,
}

impl<M: MemoryReader> StateWalker<'_, M> {
    pub(super) fn capture_inventory(
        &self,
        gui_back: u32,
    ) -> Result<Option<RawInventory>, StateReadError> {
        if gui_back == 0 {
            return Ok(None);
        }
        let pane = self.read_u32(add(gui_back, 0x4F88)?)?;
        if pane == 0 {
            return Ok(None);
        }

        let mut items = [None; INVENTORY_SLOT_COUNT];
        for (index, item) in items.iter_mut().enumerate() {
            if index + 1 == INVENTORY_GOLD_SLOT {
                continue;
            }
            let pointer = self.read_u32(indexed(
                pane,
                INVENTORY_POINTERS_OFFSET,
                size_of::<u32>() as u32,
                index,
            )?)?;
            if pointer == 0 {
                continue;
            }
            let mut bytes = [0_u8; INVENTORY_ENTRY_SIZE];
            self.read_bytes(add(pointer, INVENTORY_ENTRY_OFFSET)?, &mut bytes)?;
            let expected_slot = u8::try_from(index + 1).expect("inventory slot fits u8");
            let slot = bytes[0x84];
            let sprite = u16::from_le_bytes([bytes[0], bytes[1]]);
            if slot != expected_slot || sprite == 0 {
                return Err(StateReadError::InvalidCollection);
            }
            *item = Some(RawInventoryItem {
                slot,
                sprite,
                dye_color: bytes[0x82],
                name: raw_text(&bytes[0x02..0x82])?,
                quantity: u32_at(&bytes, 0xB0),
                can_stack: bytes[0xB4] != 0,
                durability: u32_at(&bytes, 0xA8),
                max_durability: u32_at(&bytes, 0xAC),
            });
        }
        Ok(Some(RawInventory { items }))
    }

    pub(super) fn capture_equipment(
        &self,
        pane: u32,
    ) -> Result<Option<RawEquipment>, StateReadError> {
        if pane == 0 {
            return Ok(None);
        }
        let mut bytes = [0_u8; EQUIPMENT_SNAPSHOT_SIZE];
        self.read_bytes(add(pane, EQUIPMENT_SNAPSHOT_OFFSET)?, &mut bytes)?;
        let mut items = [None; EQUIPMENT_SLOT_COUNT];
        for (index, item) in items.iter_mut().enumerate() {
            let sprite_offset = index * size_of::<u16>();
            let sprite = u16::from_le_bytes([bytes[sprite_offset], bytes[sprite_offset + 1]]);
            if sprite == 0 {
                continue;
            }
            let name_offset = 0x36 + index * 128;
            let durability_offset = 0x938 + index * 8;
            *item = Some(RawEquipmentItem {
                slot: u8::try_from(index + 1).expect("equipment slot fits u8"),
                sprite,
                dye_color: bytes[0x24 + index],
                name: raw_text(&bytes[name_offset..name_offset + 128])?,
                durability: u32_at(&bytes, durability_offset),
                max_durability: u32_at(&bytes, durability_offset + 4),
            });
        }
        Ok(Some(RawEquipment { items }))
    }
}

fn raw_text<const N: usize>(bytes: &[u8]) -> Result<RawClientText<N>, StateReadError> {
    let bytes: [u8; N] = bytes
        .try_into()
        .map_err(|_| StateReadError::InvalidCollection)?;
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(StateReadError::InvalidCollection)?;
    if length == 0 {
        return Err(StateReadError::InvalidCollection);
    }
    Ok(RawClientText {
        bytes,
        length: u8::try_from(length).map_err(|_| StateReadError::InvalidCollection)?,
    })
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed collection field fits snapshot"),
    )
}
