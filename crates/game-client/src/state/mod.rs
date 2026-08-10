mod abilities;
mod collections;
mod effects;
mod groups;
mod objects;
mod panes;
mod types;

pub use abilities::{ABILITY_SLOT_COUNT, RawSkill, RawSkillbook, RawSpell, RawSpellbook};
pub use collections::{
    EQUIPMENT_SLOT_COUNT, INVENTORY_SLOT_COUNT, RawEquipment, RawEquipmentItem, RawInventory,
    RawInventoryItem,
};
pub use effects::{EFFECT_SLOT_COUNT, RawEffect, RawEffects};
pub use groups::{
    GROUP_INVITATION_CAPACITY, GROUP_MEMBER_CAPACITY, GROUP_NAME_BYTES, RawGroupInvitation,
    RawGroupMember, RawGroupState,
};
pub use objects::{MAX_OBJECT_NAME_BYTES, MAX_WORLD_OBJECTS, RawObjects, RawWorldObject};
pub use types::{
    MemoryReader, RawAppearance, RawCharacter, RawClientText, RawLifecycle, RawLocation,
    RawMapName, RawModifiers, RawPaneProgression, RawStateSnapshot, StateReadError,
};

use crate::{
    SPELL_DELAY_ACTIVE_OFFSET, SPELL_DELAY_CONTROL_PANE_POINTER_RVA, WORLD_PANE_ADJUSTMENT,
    WORLD_PANE_POINTER_RVA, WORLD_PANE_ROUTE_ACTIVE_OFFSET,
};
use types::MAX_MAP_NAME_BYTES;

const CHARACTER_NAME_RVA: u32 = 0x0033_D910;
const EQUIPMENT_PANE_RVA: u32 = 0x002F_C914;
const MAIN_MENU_PANE_RVA: u32 = 0x0033_D968;
const MAIN_THREAD_ID_RVA: u32 = 0x0034_0400;
const GUI_BACK_PANE_RVA: u32 = 0x0042_B768;
const MAP_LOADING_PANE_RVA: u32 = 0x0045_1598;
const BOTTOM_BUTTONS_PANE_RVA: u32 = 0x002D_9230;

const GUI_BACK_PANE_ADJUSTMENT: u32 = 0x190;
const GROUP_OPEN_OFFSET: u32 = 0x1C6;
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

    pub fn capture_into(
        &self,
        current_thread_id: u32,
        output: &mut RawStateSnapshot,
    ) -> Result<(), StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }

        let roots = self.capture_roots()?;
        let (lifecycle, world_user) = self.lifecycle(&roots)?;

        let character_available =
            matches!(lifecycle, RawLifecycle::InGame | RawLifecycle::Disconnected)
                && roots.world != 0
                && world_user != 0;
        if character_available {
            self.capture_character(&roots, world_user, &mut output.character)?;
            self.capture_group(world_user, &mut output.group)?;
            output.group_available = true;
        } else {
            output.group_available = false;
        }

        let current_roots = self.capture_roots()?;
        if current_roots.world_interface != roots.world_interface
            || current_roots.gui_back_interface != roots.gui_back_interface
            || current_roots.equipment != roots.equipment
            || current_roots.event_dispatcher != roots.event_dispatcher
            || current_roots.spell_delay != roots.spell_delay
        {
            return Err(StateReadError::PointersChanged);
        }

        output.world_token = roots.world_interface;
        output.lifecycle = lifecycle;
        output.character_available = character_available;
        Ok(())
    }

    pub fn capture_lifecycle(
        &self,
        current_thread_id: u32,
    ) -> Result<RawLifecycle, StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }

        let roots = self.capture_roots()?;
        let (lifecycle, _) = self.lifecycle(&roots)?;
        let current = self.capture_roots()?;
        if current.world_interface != roots.world_interface
            || current.event_dispatcher != roots.event_dispatcher
            || current.main_menu != roots.main_menu
        {
            return Err(StateReadError::PointersChanged);
        }
        Ok(lifecycle)
    }

    #[cfg(test)]
    pub fn capture(&self, current_thread_id: u32) -> Result<RawStateSnapshot, StateReadError> {
        let mut output = RawStateSnapshot::empty();
        self.capture_into(current_thread_id, &mut output)?;
        Ok(output)
    }

    pub fn capture_objects(
        &self,
        current_thread_id: u32,
        center: Option<(i32, i32)>,
        output: &mut RawObjects,
    ) -> Result<(), StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }
        let roots = self.capture_roots()?;
        output.clear();
        if roots.world != 0 {
            self.capture_objects_from_world(roots.world, center, output)?;
        }
        let current_roots = self.capture_roots()?;
        if current_roots.world_interface != roots.world_interface {
            return Err(StateReadError::PointersChanged);
        }
        Ok(())
    }

    pub fn capture_inventory_state(
        &self,
        current_thread_id: u32,
        output: &mut RawInventory,
    ) -> Result<bool, StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }
        let roots = self.capture_roots()?;
        let available = self.capture_inventory(roots.gui_back, output)?;
        if self.capture_roots()?.gui_back_interface != roots.gui_back_interface {
            return Err(StateReadError::PointersChanged);
        }
        Ok(available)
    }

    pub fn capture_ability_state(
        &self,
        current_thread_id: u32,
        skillbook: &mut RawSkillbook,
        spellbook: &mut RawSpellbook,
    ) -> Result<(bool, bool), StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }
        let roots = self.capture_roots()?;
        let available = self.capture_abilities(roots.gui_back, skillbook, spellbook)?;
        if self.capture_roots()?.gui_back_interface != roots.gui_back_interface {
            return Err(StateReadError::PointersChanged);
        }
        Ok(available)
    }

    pub fn capture_group_state(
        &self,
        current_thread_id: u32,
        output: &mut RawGroupState,
    ) -> Result<(), StateReadError> {
        let expected_thread_id = self.read_module_u32(MAIN_THREAD_ID_RVA)?;
        if expected_thread_id == 0 || expected_thread_id != current_thread_id {
            return Err(StateReadError::WrongThread {
                expected: expected_thread_id,
                actual: current_thread_id,
            });
        }
        let roots = self.capture_roots()?;
        if roots.world == 0 {
            return Err(StateReadError::InvalidGroupState);
        }
        let world_user = self.read_u32(add(roots.world, 0x2CC)?)?;
        if world_user == 0 {
            return Err(StateReadError::InvalidGroupState);
        }
        self.capture_group(world_user, output)?;
        if self.capture_roots()?.world_interface != roots.world_interface {
            return Err(StateReadError::PointersChanged);
        }
        Ok(())
    }

    fn lifecycle(&self, roots: &Roots) -> Result<(RawLifecycle, u32), StateReadError> {
        let reconnect_dialog_open = self.reconnect_dialog_is_open(roots.event_dispatcher)?;
        let world_user = if roots.world == 0 {
            0
        } else {
            self.read_u32(add(roots.world, 0x2CC)?)?
        };
        let lifecycle = if reconnect_dialog_open {
            RawLifecycle::Disconnected
        } else {
            match (roots.main_menu != 0, roots.world != 0, world_user != 0) {
                (true, false, false) => RawLifecycle::Title,
                (false, true, true) => RawLifecycle::InGame,
                (false, false, false) | (true, true, _) | (_, true, false) => {
                    RawLifecycle::Transition
                }
                _ => RawLifecycle::Unknown,
            }
        };
        Ok((lifecycle, world_user))
    }

    fn capture_objects_from_world(
        &self,
        world: u32,
        center: Option<(i32, i32)>,
        objects: &mut RawObjects,
    ) -> Result<(), StateReadError> {
        let list = self.read_u32(add(world, 0x194)?)?;
        if list == 0 {
            return Ok(());
        }
        let head = self.read_u32(add(list, 0x20)?)?;
        if head == 0 {
            return Ok(());
        }
        let mut stack = [0_u32; MAX_TREE_DEPTH];
        let mut depth = 0_usize;
        let mut node = self.read_u32(add(head, 0x04)?)?;
        let mut visited = 0_usize;

        while (node != 0 && node != head) || depth != 0 {
            while node != 0 && node != head {
                let Some(slot) = stack.get_mut(depth) else {
                    return Err(StateReadError::InvalidObjectTree);
                };
                *slot = node;
                depth += 1;
                node = self.read_u32(node)?;
            }

            depth -= 1;
            node = stack[depth];
            visited += 1;
            if visited > MAX_WORLD_OBJECTS * 2 {
                return Err(StateReadError::InvalidObjectTree);
            }

            if let Some(object) = self.capture_world_object(node, center)?
                && !objects.push(object)
            {
                return Err(StateReadError::InvalidObjectTree);
            }
            node = self.read_u32(add(node, 0x08)?)?;
        }

        Ok(())
    }

    fn capture_world_object(
        &self,
        node: u32,
        center: Option<(i32, i32)>,
    ) -> Result<Option<RawWorldObject>, StateReadError> {
        let id = self.read_u32(add(node, 0x0C)?)?;
        let object = self.read_u32(add(node, 0x10)?)?;
        if object == 0
            || self.read_u32(add(object, 0x24)?)? != id
            || self.read_u8(add(object, 0x48)?)? == 0
        {
            return Ok(None);
        }

        let x = self.read_i32(add(object, 0x44)?)?;
        let y = self.read_i32(add(object, 0x40)?)?;
        if center.is_some_and(|(center_x, center_y)| {
            x.abs_diff(center_x).saturating_add(y.abs_diff(center_y)) > 18
        }) {
            return Ok(None);
        }

        let captured = match self.read_u32(add(object, 0x2C)?)? {
            1 => {
                let direction = self.read_u8(add(object, 0x192)?)?;
                if direction > 3 {
                    return Ok(None);
                }
                let (name, name_len) = self.capture_object_name(add(object, 0x112)?)?;
                RawWorldObject::Player {
                    id,
                    name,
                    name_len,
                    x,
                    y,
                    direction,
                }
            }
            2 => {
                let direction = self.read_u8(add(object, 0x192)?)?;
                if direction > 3 {
                    return Ok(None);
                }
                let is_npc = self.read_u8(add(object, 0x1EC)?)? == 2;
                let (name, name_len) = if is_npc {
                    let pane = self.read_u32(add(object, 0x58)?)?;
                    if pane == 0 {
                        ([0; MAX_OBJECT_NAME_BYTES], 0)
                    } else {
                        self.capture_object_name(add(pane, 0x198)?)?
                    }
                } else {
                    ([0; MAX_OBJECT_NAME_BYTES], 0)
                };
                RawWorldObject::Creature {
                    id,
                    is_npc,
                    sprite: None,
                    name,
                    name_len,
                    x,
                    y,
                    direction,
                }
            }
            8 => RawWorldObject::Item {
                id,
                sprite: self.read_u16(add(object, 0x7C)?)?,
                x,
                y,
                z_index: 0,
            },
            _ => return Ok(None),
        };
        Ok(Some(captured))
    }

    fn capture_object_name(
        &self,
        address: u32,
    ) -> Result<([u8; MAX_OBJECT_NAME_BYTES], u8), StateReadError> {
        let mut name = [0_u8; MAX_OBJECT_NAME_BYTES];
        self.read_bytes(address, &mut name)?;
        let length = name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name.len());
        Ok((
            name,
            u8::try_from(length).expect("object name buffer length fits u8"),
        ))
    }

    fn capture_roots(&self) -> Result<Roots, StateReadError> {
        let world_interface = self.read_module_u32(
            u32::try_from(WORLD_PANE_POINTER_RVA).expect("world pane pointer RVA fits u32"),
        )?;
        let gui_back_interface = self.read_module_u32(GUI_BACK_PANE_RVA)?;
        Ok(Roots {
            world_interface,
            world: adjusted(
                world_interface,
                u32::try_from(WORLD_PANE_ADJUSTMENT).expect("world pane adjustment fits u32"),
            )?,
            gui_back_interface,
            gui_back: adjusted(gui_back_interface, GUI_BACK_PANE_ADJUSTMENT)?,
            equipment: self.read_module_u32(EQUIPMENT_PANE_RVA)?,
            event_dispatcher: self.read_module_u32(panes::EVENT_DISPATCHER_RVA)?,
            main_menu: self.read_module_u32(MAIN_MENU_PANE_RVA)?,
            map_loading: self.read_module_u32(MAP_LOADING_PANE_RVA)?,
            spell_delay: self.read_module_u32(
                u32::try_from(SPELL_DELAY_CONTROL_PANE_POINTER_RVA)
                    .expect("spell delay pointer RVA fits u32"),
            )?,
        })
    }

    fn capture_character(
        &self,
        roots: &Roots,
        world_user: u32,
        output: &mut RawCharacter,
    ) -> Result<(), StateReadError> {
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
        let (appearance, x, y, direction) = if let Some(object) = local_object {
            let direction = self.read_u8(add(object, 0x192)?)?;
            (
                self.capture_appearance(object)?,
                Some(self.read_i32(add(object, 0x44)?)?),
                Some(self.read_i32(add(object, 0x40)?)?),
                (direction <= 3).then_some(direction),
            )
        } else {
            (None, None, None, None)
        };

        let pane_progression = self.capture_pane_progression(roots.gui_back)?;
        let modifiers = self.capture_modifiers(roots.gui_back)?;
        let location = self.capture_location(roots, x, y)?;
        let inventory_available = self.capture_inventory(roots.gui_back, &mut output.inventory)?;
        let equipment_available = self.capture_equipment(roots.equipment, &mut output.equipment)?;
        let (skillbook_available, spellbook_available) =
            self.capture_abilities(roots.gui_back, &mut output.skillbook, &mut output.spellbook)?;
        let effects = self.capture_effects(roots.gui_back)?;

        output.id = (self_id != 0).then_some(self_id);
        output.name = name;
        output.name_len = u8::try_from(name_len).expect("character name buffer length fits u8");
        output.direction = direction;
        output.appearance = appearance;
        output.class = self.read_u8(add(world_user, 0x1089)?)?;
        output.is_action_restricted = self.read_u8(add(world_user, 0x15C88)?)? & 0x01 != 0;
        output.is_blinded = self.read_u8(add(world_user, 0x108D)?)? == 0x08;
        output.is_casting = roots.spell_delay != 0
            && self.read_u8(add(
                roots.spell_delay,
                u32::try_from(SPELL_DELAY_ACTIVE_OFFSET)
                    .expect("spell delay active offset fits u32"),
            )?)? != 0;
        output.is_walking = self.read_u8(add(
            roots.world,
            u32::try_from(WORLD_PANE_ROUTE_ACTIVE_OFFSET)
                .expect("world pane route flag offset fits u32"),
        )?)? != 0;
        output.gold = self.read_u32(add(world_user, 0x105C)?)?;
        output.weight = self.read_u32(add(world_user, 0x15C84)?)?;
        output.max_weight = self.read_u32(add(world_user, 0x15C80)?)?;
        output.level = self.read_u8(add(world_user, 0x1058)?)?;
        output.ability_level = self.read_u8(add(world_user, 0x1059)?)?;
        output.experience = self.read_u32(add(world_user, 0x1060)?)?;
        output.pane_progression = pane_progression;
        output.strength = self.read_u16(add(world_user, 0x1064)?)?;
        output.intelligence = self.read_u16(add(world_user, 0x106E)?)?;
        output.wisdom = self.read_u16(add(world_user, 0x106A)?)?;
        output.constitution = self.read_u16(add(world_user, 0x106C)?)?;
        output.dexterity = self.read_u16(add(world_user, 0x1068)?)?;
        output.health = self.read_u32(add(world_user, 0x1078)?)?;
        output.max_health = self.read_u32(add(world_user, 0x107C)?)?;
        output.mana = self.read_u32(add(world_user, 0x1080)?)?;
        output.max_mana = self.read_u32(add(world_user, 0x1084)?)?;
        output.modifiers = modifiers;
        output.location = location;
        output.inventory_available = inventory_available;
        output.equipment_available = equipment_available;
        output.skillbook_available = skillbook_available;
        output.spellbook_available = spellbook_available;
        output.effects = effects;
        Ok(())
    }

    fn capture_group(
        &self,
        world_user: u32,
        group: &mut RawGroupState,
    ) -> Result<(), StateReadError> {
        const RECORDS_OFFSET: u32 = 0x04;
        const RECORD_SIZE: u32 = 0x41;
        const LEADER_OFFSET: u32 = 0x40;
        const COUNT_OFFSET: u32 = 0x1044;

        let count = self.read_u32(add(world_user, COUNT_OFFSET)?)?;
        let count = usize::try_from(count).map_err(|_| StateReadError::InvalidGroupState)?;
        if count > GROUP_MEMBER_CAPACITY {
            return Err(StateReadError::InvalidGroupState);
        }
        group.member_count = u8::try_from(count).expect("group capacity fits u8");
        group.invitation_count = 0;
        group.is_group_open = None;
        group.auto_accept = None;
        for (index, member) in group.members.iter_mut().take(count).enumerate() {
            let record = indexed(world_user, RECORDS_OFFSET, RECORD_SIZE, index)?;
            self.read_bytes(record, &mut member.name)?;
            member.name_len = u8::try_from(
                member
                    .name
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(member.name.len()),
            )
            .expect("group name buffer fits u8");
            member.is_leader = self.read_u8(add(record, LEADER_OFFSET)?)? != 0;
        }
        let bottom_buttons = self.read_module_u32(BOTTOM_BUTTONS_PANE_RVA)?;
        if bottom_buttons != 0 {
            let is_group_open = self.read_u8(add(bottom_buttons, GROUP_OPEN_OFFSET)?)?;
            if is_group_open > 1 {
                return Err(StateReadError::InvalidGroupState);
            }
            group.is_group_open = Some(is_group_open != 0);
        }
        Ok(())
    }

    fn capture_appearance(&self, object: u32) -> Result<Option<RawAppearance>, StateReadError> {
        if self.read_u8(add(object, 0x104)?)? == 0 {
            return Ok(None);
        }

        Ok(Some(RawAppearance {
            gender: self.read_u8(add(object, 0xA4)?)?,
            hair_style: self.read_u16(add(object, 0xA6)?)?,
            hair_color: self.read_u8(add(object, 0xA8)?)?,
            body_sprite: self.read_u16(add(object, 0xAA)?)?,
        }))
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
    event_dispatcher: u32,
    main_menu: u32,
    map_loading: u32,
    spell_delay: u32,
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
