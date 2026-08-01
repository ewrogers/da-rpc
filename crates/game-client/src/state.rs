mod abilities;
mod collections;
mod types;

pub use abilities::{ABILITY_SLOT_COUNT, RawSkill, RawSkillbook, RawSpell, RawSpellbook};
pub use collections::{
    EQUIPMENT_SLOT_COUNT, INVENTORY_SLOT_COUNT, RawEquipment, RawEquipmentItem, RawInventory,
    RawInventoryItem,
};
pub use types::{
    MemoryReader, RawCharacter, RawClientText, RawLifecycle, RawLocation, RawMapName, RawModifiers,
    RawPaneProgression, RawStateSnapshot, StateReadError,
};

use types::MAX_MAP_NAME_BYTES;

const CHARACTER_NAME_RVA: u32 = 0x0033_D910;
const EQUIPMENT_PANE_RVA: u32 = 0x002F_C914;
const WORLD_IMPLEMENTATION_RVA: u32 = 0x0033_D964;
const MAIN_MENU_PANE_RVA: u32 = 0x0033_D968;
const MAIN_THREAD_ID_RVA: u32 = 0x0034_0400;
const GUI_BACK_PANE_RVA: u32 = 0x0042_B768;
const MAP_LOADING_PANE_RVA: u32 = 0x0045_1598;

const WORLD_IMPLEMENTATION_ADJUSTMENT: u32 = 0x2EC;
const GUI_BACK_PANE_ADJUSTMENT: u32 = 0x190;
const MAX_MAP_DIMENSION: i32 = 255;
const MAX_TREE_DEPTH: usize = 64;

pub struct StateWalker<'a, M> {
    memory: &'a M,
    module_base: u32,
}

impl<'a, M: MemoryReader> StateWalker<'a, M> {
    #[must_use]
    pub const fn new(memory: &'a M, module_base: u32) -> Self {
        Self {
            memory,
            module_base,
        }
    }

    pub fn capture(&self, current_thread_id: u32) -> Result<RawStateSnapshot, StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }

        let roots = self.capture_roots()?;
        let world_user = if roots.world == 0 {
            0
        } else {
            self.read_u32(add(roots.world, 0x2CC)?)?
        };
        let lifecycle = match (roots.main_menu != 0, roots.world != 0, world_user != 0) {
            (true, false, false) => RawLifecycle::Title,
            (false, true, true) => RawLifecycle::InGame,
            (false, false, false) | (true, true, _) | (_, true, false) => RawLifecycle::Transition,
            _ => RawLifecycle::Unknown,
        };

        let character = if lifecycle == RawLifecycle::InGame {
            Some(self.capture_character(&roots, world_user)?)
        } else {
            None
        };

        let current_roots = self.capture_roots()?;
        if current_roots.world_interface != roots.world_interface
            || current_roots.gui_back_interface != roots.gui_back_interface
            || current_roots.equipment != roots.equipment
        {
            return Err(StateReadError::PointersChanged);
        }

        Ok(RawStateSnapshot {
            world_token: roots.world_interface,
            lifecycle,
            character,
        })
    }

    fn capture_roots(&self) -> Result<Roots, StateReadError> {
        let world_interface = self.read_module_u32(WORLD_IMPLEMENTATION_RVA)?;
        let gui_back_interface = self.read_module_u32(GUI_BACK_PANE_RVA)?;
        Ok(Roots {
            world_interface,
            world: adjusted(world_interface, WORLD_IMPLEMENTATION_ADJUSTMENT)?,
            gui_back_interface,
            gui_back: adjusted(gui_back_interface, GUI_BACK_PANE_ADJUSTMENT)?,
            equipment: self.read_module_u32(EQUIPMENT_PANE_RVA)?,
            main_menu: self.read_module_u32(MAIN_MENU_PANE_RVA)?,
            map_loading: self.read_module_u32(MAP_LOADING_PANE_RVA)?,
        })
    }

    fn capture_character(
        &self,
        roots: &Roots,
        world_user: u32,
    ) -> Result<RawCharacter, StateReadError> {
        let mut name = [0_u8; 16];
        self.read_bytes(self.module_address(CHARACTER_NAME_RVA)?, &mut name)?;
        let name_len = name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len());

        let self_id = self.read_u32(add(world_user, 0x1050)?)?;
        let local_object = if self_id == 0 {
            None
        } else {
            self.find_local_object(roots.world, self_id).ok().flatten()
        };
        let (gender, x, y) = if let Some(object) = local_object {
            (
                Some(self.read_u8(add(object, 0xA4)?)?),
                Some(self.read_i32(add(object, 0x44)?)?),
                Some(self.read_i32(add(object, 0x40)?)?),
            )
        } else {
            (None, None, None)
        };

        let pane_progression = self.capture_pane_progression(roots.gui_back)?;
        let modifiers = self.capture_modifiers(roots.gui_back)?;
        let location = self.capture_location(roots, x, y)?;
        let inventory = self.capture_inventory(roots.gui_back)?;
        let equipment = self.capture_equipment(roots.equipment)?;
        let (skillbook, spellbook) = self.capture_abilities(roots.gui_back)?;

        Ok(RawCharacter {
            id: (self_id != 0).then_some(self_id),
            name,
            name_len: u8::try_from(name_len).expect("character name buffer length fits u8"),
            gender,
            class: self.read_u8(add(world_user, 0x1089)?)?,
            gold: self.read_u32(add(world_user, 0x105C)?)?,
            level: self.read_u8(add(world_user, 0x1058)?)?,
            ability_level: self.read_u8(add(world_user, 0x1059)?)?,
            experience: self.read_u32(add(world_user, 0x1060)?)?,
            pane_progression,
            strength: self.read_u16(add(world_user, 0x1064)?)?,
            intelligence: self.read_u16(add(world_user, 0x106E)?)?,
            wisdom: self.read_u16(add(world_user, 0x106A)?)?,
            constitution: self.read_u16(add(world_user, 0x106C)?)?,
            dexterity: self.read_u16(add(world_user, 0x1068)?)?,
            health: self.read_u32(add(world_user, 0x1078)?)?,
            max_health: self.read_u32(add(world_user, 0x107C)?)?,
            mana: self.read_u32(add(world_user, 0x1080)?)?,
            max_mana: self.read_u32(add(world_user, 0x1084)?)?,
            modifiers,
            location,
            inventory,
            equipment,
            spellbook,
            skillbook,
        })
    }

    fn capture_pane_progression(
        &self,
        gui_back: u32,
    ) -> Result<Option<RawPaneProgression>, StateReadError> {
        if gui_back == 0 {
            return Ok(None);
        }
        let pane = self.read_u32(add(gui_back, 0x4FA0)?)?;
        if pane == 0 {
            return Ok(None);
        }
        Ok(Some(RawPaneProgression {
            ability_points: self.read_u32(add(pane, 0x1E0)?)?,
            experience_to_next_level: self.read_u32(add(pane, 0x1D0)?)?,
            ability_to_next_level: self.read_u32(add(pane, 0x1D8)?)?,
        }))
    }

    fn capture_modifiers(&self, gui_back: u32) -> Result<Option<RawModifiers>, StateReadError> {
        if gui_back == 0 {
            return Ok(None);
        }
        let pane = self.read_u32(add(gui_back, 0x4FA4)?)?;
        if pane == 0 {
            return Ok(None);
        }
        Ok(Some(RawModifiers {
            armor_class: self.read_i8(add(pane, 0x4F8)?)?,
            damage: self.read_u8(add(pane, 0x4F9)?)?,
            hit: self.read_u8(add(pane, 0x4FA)?)?,
            attack_element: self.read_u16(add(pane, 0x4FC)?)?,
            defense_element: self.read_u16(add(pane, 0x4FE)?)?,
            magic_resistance_units: self.read_u16(add(pane, 0x500)?)?,
        }))
    }

    fn capture_location(
        &self,
        roots: &Roots,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Result<Option<RawLocation>, StateReadError> {
        let width = self.read_i32(add(roots.world, 0x1C4)?)?;
        let height = self.read_i32(add(roots.world, 0x1C8)?)?;
        let transfer_active = self.read_u8(add(roots.world, 0x275)?)?;
        let cells = self.read_u32(add(roots.world, 0x27C)?)?;
        let ready = (1..=MAX_MAP_DIMENSION).contains(&width)
            && (1..=MAX_MAP_DIMENSION).contains(&height)
            && transfer_active == 0
            && cells != 0
            && roots.map_loading == 0;
        if !ready {
            return Ok(None);
        }
        Ok(Some(RawLocation {
            map_id: self.read_u32(add(roots.world, 0x26C)?)?,
            name: self.capture_map_name(roots.gui_back_interface)?,
            x,
            y,
            width,
            height,
        }))
    }

    fn capture_map_name(
        &self,
        gui_back_interface: u32,
    ) -> Result<Option<RawMapName>, StateReadError> {
        if gui_back_interface == 0 {
            return Ok(None);
        }
        let field = add(gui_back_interface, 0x4CAC)?;
        let mut prefix = [0_u8; 4];
        self.read_bytes(field, &mut prefix)?;
        let value_address = if looks_like_inline_map_name_prefix(&prefix) {
            field
        } else {
            u32::from_le_bytes(prefix)
        };
        if value_address == 0 {
            return Ok(None);
        }
        let mut bytes = [0_u8; MAX_MAP_NAME_BYTES];
        self.read_bytes(value_address, &mut bytes)?;
        let Some(length) = bytes.iter().position(|byte| *byte == 0) else {
            return Ok(None);
        };
        if length == 0
            || !bytes[..length]
                .iter()
                .all(|byte| (0x20..=0x7E).contains(byte))
        {
            return Ok(None);
        }
        Ok(Some(RawMapName {
            bytes,
            length: length as u8,
        }))
    }

    fn find_local_object(&self, world: u32, entity_id: u32) -> Result<Option<u32>, StateReadError> {
        let list = self.read_u32(add(world, 0x194)?)?;
        if list == 0 {
            return Ok(None);
        }
        let head = self.read_u32(add(list, 0x20)?)?;
        if head == 0 {
            return Ok(None);
        }
        let mut node = self.read_u32(add(head, 0x04)?)?;
        let mut previous = 0;
        for _ in 0..MAX_TREE_DEPTH {
            if node == 0 || node == head {
                return Ok(None);
            }
            if node == previous {
                return Err(StateReadError::InvalidObjectTree);
            }
            previous = node;
            let node_id = self.read_u32(add(node, 0x0C)?)?;
            if entity_id < node_id {
                node = self.read_u32(node)?;
            } else if entity_id > node_id {
                node = self.read_u32(add(node, 0x08)?)?;
            } else {
                let object = self.read_u32(add(node, 0x10)?)?;
                if object == 0
                    || self.read_u32(add(object, 0x24)?)? != entity_id
                    || self.read_u8(add(object, 0x48)?)? == 0
                    || self.read_u8(add(object, 0x98)?)? == 0
                {
                    return Ok(None);
                }
                return Ok(Some(object));
            }
        }
        Err(StateReadError::InvalidObjectTree)
    }

    fn module_address(&self, rva: u32) -> Result<u32, StateReadError> {
        add(self.module_base, rva)
    }

    fn read_module_u32(&self, rva: u32) -> Result<u32, StateReadError> {
        self.read_u32(self.module_address(rva)?)
    }

    fn read_u8(&self, address: u32) -> Result<u8, StateReadError> {
        let mut bytes = [0; 1];
        self.read_bytes(address, &mut bytes)?;
        Ok(bytes[0])
    }

    fn read_i8(&self, address: u32) -> Result<i8, StateReadError> {
        Ok(self.read_u8(address)? as i8)
    }

    fn read_u16(&self, address: u32) -> Result<u16, StateReadError> {
        let mut bytes = [0; 2];
        self.read_bytes(address, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn read_u32(&self, address: u32) -> Result<u32, StateReadError> {
        let mut bytes = [0; 4];
        self.read_bytes(address, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_i32(&self, address: u32) -> Result<i32, StateReadError> {
        let mut bytes = [0; 4];
        self.read_bytes(address, &mut bytes)?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn read_bytes(&self, address: u32, output: &mut [u8]) -> Result<(), StateReadError> {
        if self.memory.read(address, output) {
            Ok(())
        } else {
            Err(StateReadError::UnreadableMemory {
                address,
                length: output.len(),
            })
        }
    }
}

#[derive(Clone, Copy)]
struct Roots {
    world_interface: u32,
    world: u32,
    gui_back_interface: u32,
    gui_back: u32,
    equipment: u32,
    main_menu: u32,
    map_loading: u32,
}

fn adjusted(pointer: u32, adjustment: u32) -> Result<u32, StateReadError> {
    if pointer == 0 {
        Ok(0)
    } else {
        pointer
            .checked_sub(adjustment)
            .ok_or(StateReadError::AddressOverflow)
    }
}

fn add(address: u32, offset: u32) -> Result<u32, StateReadError> {
    address
        .checked_add(offset)
        .ok_or(StateReadError::AddressOverflow)
}

fn indexed(address: u32, offset: u32, stride: u32, index: usize) -> Result<u32, StateReadError> {
    let index = u32::try_from(index).map_err(|_| StateReadError::AddressOverflow)?;
    let displacement = stride
        .checked_mul(index)
        .and_then(|value| offset.checked_add(value))
        .ok_or(StateReadError::AddressOverflow)?;
    add(address, displacement)
}

fn looks_like_inline_map_name_prefix(prefix: &[u8; 4]) -> bool {
    let mut has_character = false;
    for byte in prefix {
        if *byte == 0 {
            return has_character;
        }
        let is_letter = byte.is_ascii_alphabetic();
        let is_digit = byte.is_ascii_digit();
        if !is_letter && !is_digit && !matches!(*byte, b' ' | b'-' | b'\'') {
            return false;
        }
        if !has_character && !is_letter && !is_digit {
            return false;
        }
        has_character = true;
    }
    has_character
}

#[cfg(test)]
mod tests;
