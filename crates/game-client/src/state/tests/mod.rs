use super::panes::{EVENT_DISPATCHER_RVA, RECONNECT_DIALOG_VTABLE_RVA};
use super::{
    CHARACTER_NAME_RVA, EQUIPMENT_PANE_RVA, GUI_BACK_PANE_ADJUSTMENT, GUI_BACK_PANE_RVA,
    MAIN_MENU_PANE_RVA, MAIN_THREAD_ID_RVA, MAP_LOADING_PANE_RVA, MAX_OBJECT_TREE_NODES,
    MAX_TREE_DEPTH, MAX_WORLD_OBJECTS, MemoryReader, PLAYER_TRANSLUCENT_STATE_OFFSET,
    RawHumanVisual, RawLifecycle, RawObjects, RawPlayerVisual, RawStateSnapshot, RawWorldObject,
    StateReadError, StateWalker,
};
use crate::{WORLD_PANE_ADJUSTMENT, WORLD_PANE_POINTER_RVA};

#[test]
fn raw_snapshot_size_stays_bounded() {
    let size = std::mem::size_of::<RawStateSnapshot>();
    assert!(size <= 60 * 1024, "raw snapshot is {size} bytes");
}

const BASE: u32 = 0x0040_0000;
const THREAD_ID: u32 = 77;
const WORLD: u32 = 0x0090_0000;
const WORLD_USER: u32 = 0x0091_0000;
const GUI_BACK: u32 = 0x0092_0000;
const STATUS: u32 = 0x0093_0000;
const EXTRA_STATUS: u32 = 0x0094_0000;
const OBJECT_LIST: u32 = 0x0095_0000;
const HEAD: u32 = 0x0095_1000;
const NODE: u32 = 0x0095_2000;
const OBJECT: u32 = 0x0096_0000;
const MAP_CELLS: u32 = 0x0097_0000;
const MAP_NAME_TEXT: u32 = 0x0098_0000;
const INVENTORY_PANE: u32 = 0x0098_1000;
const INVENTORY_ENTRY: u32 = 0x0098_2000;
const INVENTORY_GOLD_ENTRY: u32 = 0x0098_3000;
const EQUIPMENT_PANE: u32 = 0x0099_0000;
const ABILITY_INVENTORY: u32 = 0x009A_0000;
const SKILL_PANE: u32 = 0x009A_1000;
const SPELL_PANE: u32 = 0x009A_2000;
const SKILL_POINTERS: u32 = 0x009B_0000;
const SPELL_POINTERS: u32 = 0x009B_1000;
const SKILL_ENTRY: u32 = 0x009C_0000;
const SPELL_ENTRY: u32 = 0x009D_0000;
const EVENT_DISPATCHER: u32 = 0x009E_0000;
const EVENT_ENTRIES: u32 = 0x009E_1000;
const RECONNECT_DIALOG: u32 = 0x009E_2000;
const EFFECT_PANE: u32 = 0x009E_3000;
const BOTTOM_BUTTONS: u32 = 0x009E_4000;
const TREE_NODES: u32 = 0x00A0_0000;
const TREE_OBJECTS: u32 = 0x00A4_0000;
const TREE_NODE_STRIDE: u32 = 0x20;
const TREE_OBJECT_STRIDE: u32 = 0x200;

struct FakeMemory {
    bytes: Vec<u8>,
}

impl FakeMemory {
    fn gameplay() -> Self {
        let mut memory = Self {
            bytes: vec![0; 0x0070_0000],
        };
        memory.u32(BASE + MAIN_THREAD_ID_RVA, THREAD_ID);
        memory.u32(
            BASE + WORLD_PANE_POINTER_RVA as u32,
            WORLD + WORLD_PANE_ADJUSTMENT as u32,
        );
        memory.u32(
            BASE + GUI_BACK_PANE_RVA,
            GUI_BACK + GUI_BACK_PANE_ADJUSTMENT,
        );
        memory.u32(BASE + MAIN_MENU_PANE_RVA, 0);
        memory.u32(BASE + MAP_LOADING_PANE_RVA, 0);
        memory.bytes(BASE + CHARACTER_NAME_RVA, b"SiLo\0");
        memory.bytes(GUI_BACK + GUI_BACK_PANE_ADJUSTMENT + 0x4CAC, b"Mileth\0");

        memory.u32(WORLD + 0x194, OBJECT_LIST);
        memory.i32(WORLD + 0x1C4, 100);
        memory.i32(WORLD + 0x1C8, 80);
        memory.u32(WORLD + 0x26C, 3001);
        memory.u8(WORLD + 0x275, 0);
        memory.u32(WORLD + 0x27C, MAP_CELLS);
        memory.u32(WORLD + 0x2CC, WORLD_USER);

        memory.u32(WORLD_USER + 0x1050, 0x1122_3344);
        memory.u8(WORLD_USER + 0x1058, 99);
        memory.u8(WORLD_USER + 0x1059, 7);
        memory.u32(WORLD_USER + 0x105C, 123_456);
        memory.u32(WORLD_USER + 0x1060, 8_000_000);
        memory.u16(WORLD_USER + 0x1064, 30);
        memory.u16(WORLD_USER + 0x1068, 31);
        memory.u16(WORLD_USER + 0x106A, 32);
        memory.u16(WORLD_USER + 0x106C, 33);
        memory.u16(WORLD_USER + 0x106E, 34);
        memory.u32(WORLD_USER + 0x1078, 1_000);
        memory.u32(WORLD_USER + 0x107C, 1_100);
        memory.u32(WORLD_USER + 0x1080, 900);
        memory.u32(WORLD_USER + 0x1084, 950);
        memory.u8(WORLD_USER + 0x1089, 3);
        memory.u8(WORLD_USER + 0x108D, 0x08);
        memory.u32(WORLD_USER + 0x15C80, 120);
        memory.u32(WORLD_USER + 0x15C84, 88);
        memory.u8(WORLD_USER + 0x15C88, 1);

        memory.u32(GUI_BACK + 0x4FA0, STATUS);
        memory.u32(GUI_BACK + 0x4FA4, EXTRA_STATUS);
        memory.u32(STATUS + 0x1D0, 44_000);
        memory.u32(STATUS + 0x1D8, 55_000);
        memory.u32(STATUS + 0x1E0, 66_000);
        memory.u8(EXTRA_STATUS + 0x4F8, -7_i8 as u8);
        memory.u8(EXTRA_STATUS + 0x4F9, 8);
        memory.u8(EXTRA_STATUS + 0x4FA, 9);
        memory.u16(EXTRA_STATUS + 0x4FC, 1);
        memory.u16(EXTRA_STATUS + 0x4FE, 2);
        memory.u16(EXTRA_STATUS + 0x500, 3);

        memory.u32(OBJECT_LIST + 0x20, HEAD);
        memory.u32(HEAD + 0x04, NODE);
        memory.u32(NODE + 0x0C, 0x1122_3344);
        memory.u32(NODE + 0x10, OBJECT);
        memory.u32(OBJECT + 0x24, 0x1122_3344);
        memory.u32(OBJECT + 0x2C, 1);
        memory.i32(OBJECT + 0x40, 22);
        memory.i32(OBJECT + 0x44, 11);
        memory.u8(OBJECT + 0x48, 1);
        memory.bytes(OBJECT + 0x112, b"SiLo\0");
        memory.u8(OBJECT + 0x192, 2);
        memory.u8(OBJECT + 0x98, 1);
        memory.u8(OBJECT + 0xA4, 0);
        memory.u16(OBJECT + 0xA6, 17);
        memory.u8(OBJECT + 0xA8, 6);
        memory.u16(OBJECT + 0xAA, 1);
        memory.u8(OBJECT + 0xAC, 4);
        memory.u16(OBJECT + 0xAE, 102);
        memory.u16(OBJECT + 0xB0, 103);
        memory.u8(OBJECT + 0xB2, 5);
        memory.u16(OBJECT + 0xB4, 104);
        memory.u8(OBJECT + 0xB6, 6);
        memory.u16(OBJECT + 0xB8, 105);
        memory.u16(OBJECT + 0xBE, 106);
        memory.u16(OBJECT + 0xC0, 109);
        memory.u8(OBJECT + 0xC2, 8);
        memory.u16(OBJECT + 0xC4, 107);
        memory.u16(OBJECT + 0xC6, 108);
        memory.u8(OBJECT + 0xC8, 7);
        memory.u16(OBJECT + 0xCA, 110);
        memory.u8(OBJECT + 0xCC, 9);
        memory.u16(OBJECT + 0xCE, 111);
        memory.u8(OBJECT + 0xD0, 10);
        memory.u8(OBJECT + 0xD1, 11);
        memory.u8(OBJECT + 0xD2, 0);
        memory.u8(OBJECT + 0xD3, 12);
        memory.u8(OBJECT + 0x104, 1);
        memory.i32(OBJECT + 0x40, 22);
        memory.i32(OBJECT + 0x44, 11);
        memory
    }

    fn gameplay_with_collections() -> Self {
        let mut memory = Self::gameplay();

        memory.u32(GUI_BACK + 0x4F88, INVENTORY_PANE);
        memory.u32(INVENTORY_PANE + 0x1A0, INVENTORY_ENTRY);
        memory.u16(INVENTORY_ENTRY + 0x190, 0x8123);
        memory.bytes(INVENTORY_ENTRY + 0x192, b"Dark Belt [3]\0");
        memory.u8(INVENTORY_ENTRY + 0x212, 7);
        memory.u8(INVENTORY_ENTRY + 0x214, 1);
        memory.u32(INVENTORY_ENTRY + 0x238, 41);
        memory.u32(INVENTORY_ENTRY + 0x23C, 50);
        memory.u32(INVENTORY_ENTRY + 0x240, 3);
        memory.u8(INVENTORY_ENTRY + 0x244, 1);
        memory.u32(INVENTORY_PANE + 0x1A0 + 59 * 4, INVENTORY_GOLD_ENTRY);
        memory.u16(INVENTORY_GOLD_ENTRY + 0x190, 0x8088);
        memory.bytes(INVENTORY_GOLD_ENTRY + 0x192, b"Gold (123456)\0");
        memory.u8(INVENTORY_GOLD_ENTRY + 0x214, 60);

        memory.u32(BASE + EQUIPMENT_PANE_RVA, EQUIPMENT_PANE);
        memory.u16(EQUIPMENT_PANE + 0x111C, 0x9234);
        memory.u8(EQUIPMENT_PANE + 0x1140, 2);
        memory.bytes(EQUIPMENT_PANE + 0x1152, b"Hy-Brasyl Armor\0");
        memory.u32(EQUIPMENT_PANE + 0x1A54, 900);
        memory.u32(EQUIPMENT_PANE + 0x1A58, 1_000);

        memory.u32(GUI_BACK + 0x4F8C, ABILITY_INVENTORY);
        memory.u32(ABILITY_INVENTORY + 0x224, SKILL_PANE);
        memory.u32(ABILITY_INVENTORY + 0x228, SPELL_PANE);
        memory.i32(SKILL_PANE + 0x190, 1);
        memory.u32(SKILL_PANE + 0x194, SKILL_POINTERS);
        memory.u32(SKILL_POINTERS, SKILL_ENTRY);
        memory.u16(SKILL_ENTRY + 0x190, 0x0123);
        memory.bytes(SKILL_ENTRY + 0x192, b"Assail (Lev:10/100)\0");
        memory.u8(SKILL_ENTRY + 0x312, 4);
        memory.u32(SKILL_ENTRY + 0x318, 1_000);
        memory.u32(SKILL_ENTRY + 0x31C, 2_000);
        memory.u8(SKILL_ENTRY + 0x320, 1);
        memory.i32(SKILL_ENTRY + 0x33C, 10);
        memory.i32(SKILL_ENTRY + 0x344, 6);

        memory.i32(SPELL_PANE + 0x190, 1);
        memory.u32(SPELL_PANE + 0x194, SPELL_POINTERS);
        memory.u32(SPELL_POINTERS, SPELL_ENTRY);
        memory.u8(SPELL_ENTRY + 0x190, 7);
        memory.u16(SPELL_ENTRY + 0x192, 0x0456);
        memory.u8(SPELL_ENTRY + 0x194, 1);
        memory.bytes(SPELL_ENTRY + 0x195, b"Fas Spiorad (Lev:3/5)\0");
        memory.bytes(SPELL_ENTRY + 0x215, b"Target \xFFname?\0");
        memory.u8(SPELL_ENTRY + 0x295, 4);
        memory.u8(SPELL_ENTRY + 0x297, 1);
        memory.i32(SPELL_ENTRY + 0x2B0, 3);
        memory.i32(SPELL_ENTRY + 0x2B8, 11);

        memory.u32(GUI_BACK + 0x4F94, EFFECT_PANE);
        memory.u16(EFFECT_PANE + 0x190, 300);
        memory.u8(EFFECT_PANE + 0x1A4, 6);
        memory.u16(EFFECT_PANE + 0x190 + 3 * 2, 301);
        memory.u8(EFFECT_PANE + 0x1A4 + 3, 2);
        memory
    }

    fn right_linked_world_tree(&mut self, node_count: usize) {
        assert!(node_count > 0);
        self.u32(HEAD + 0x04, TREE_NODES);
        for index in 0..node_count {
            let node = TREE_NODES
                + u32::try_from(index).expect("test node index fits u32") * TREE_NODE_STRIDE;
            let right = if index + 1 == node_count {
                HEAD
            } else {
                node + TREE_NODE_STRIDE
            };
            self.u32(node + 0x08, right);
        }
    }

    fn right_linked_item_tree(&mut self, node_count: usize) {
        self.right_linked_world_tree(node_count);
        for index in 0..node_count {
            let index = u32::try_from(index).expect("test item index fits u32");
            let node = TREE_NODES + index * TREE_NODE_STRIDE;
            let object = TREE_OBJECTS + index * TREE_OBJECT_STRIDE;
            let id = index + 1;
            self.u32(node + 0x0C, id);
            self.u32(node + 0x10, object);
            self.u32(object + 0x24, id);
            self.u32(object + 0x2C, 8);
            self.i32(object + 0x40, 22);
            self.i32(object + 0x44, 11);
            self.u8(object + 0x48, 1);
            self.u16(object + 0x7C, 0x8123);
        }
    }

    fn offset(address: u32) -> usize {
        usize::try_from(address - BASE).unwrap()
    }

    fn bytes(&mut self, address: u32, value: &[u8]) {
        let offset = Self::offset(address);
        self.bytes[offset..offset + value.len()].copy_from_slice(value);
    }

    fn u8(&mut self, address: u32, value: u8) {
        self.bytes(address, &[value]);
    }

    fn u16(&mut self, address: u32, value: u16) {
        self.bytes(address, &value.to_le_bytes());
    }

    fn u32(&mut self, address: u32, value: u32) {
        self.bytes(address, &value.to_le_bytes());
    }

    fn i32(&mut self, address: u32, value: i32) {
        self.bytes(address, &value.to_le_bytes());
    }
}

impl MemoryReader for FakeMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> bool {
        let Some(offset) = address.checked_sub(BASE).map(|value| value as usize) else {
            return false;
        };
        let Some(end) = offset.checked_add(output.len()) else {
            return false;
        };
        let Some(bytes) = self.bytes.get(offset..end) else {
            return false;
        };
        output.copy_from_slice(bytes);
        true
    }
}

#[test]
fn captures_the_scalar_gameplay_snapshot() {
    let memory = FakeMemory::gameplay();
    let walker = StateWalker::new(&memory, BASE);
    let snapshot = walker.capture(THREAD_ID).unwrap();
    assert_eq!(
        walker.capture_lifecycle(THREAD_ID).unwrap(),
        RawLifecycle::InGame
    );
    assert!(snapshot.character_available);
    let character = &snapshot.character;
    let location = character.location.unwrap();
    let progression = character.pane_progression.unwrap();
    let modifiers = character.modifiers.unwrap();
    let appearance = character.appearance.unwrap();

    assert_eq!(snapshot.lifecycle, RawLifecycle::InGame);
    assert_eq!(character.id, Some(0x1122_3344));
    assert_eq!(&character.name[..usize::from(character.name_len)], b"SiLo");
    assert_eq!(character.direction, Some(2));
    assert_eq!(appearance.gender, 0);
    assert_eq!(appearance.hair_style, 17);
    assert_eq!(appearance.hair_color, 6);
    assert_eq!(appearance.body_sprite, 1);
    assert_eq!(character.class, 3);
    assert!(!character.is_hidden);
    assert!(character.is_action_restricted);
    assert!(character.is_blinded);
    assert_eq!(character.gold, 123_456);
    assert_eq!(character.weight, 88);
    assert_eq!(character.max_weight, 120);
    assert_eq!(progression.ability_points, 66_000);
    assert_eq!(character.strength, 30);
    assert_eq!(character.intelligence, 34);
    assert_eq!(character.health, 1_000);
    assert_eq!(modifiers.armor_class, -7);
    assert_eq!(modifiers.magic_resistance_units, 3);
    assert_eq!(location.map_id, 3001);
    let map_name = location.name.unwrap();
    assert_eq!(&map_name.bytes[..usize::from(map_name.length)], b"Mileth");
    assert_eq!((location.x, location.y), (Some(11), Some(22)));
    assert_eq!((location.width, location.height), (100, 80));
}

#[test]
fn captures_group_members_and_leader() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(WORLD_USER + 0x1044, 2);
    memory.bytes(WORLD_USER + 0x04, b"Eidolon\0");
    memory.u8(WORLD_USER + 0x44, 1);
    memory.bytes(WORLD_USER + 0x45, b"ZiLo\0");
    memory.u32(BASE + 0x002D_9230, BOTTOM_BUTTONS);
    memory.u8(BOTTOM_BUTTONS + 0x1C6, 1);

    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.group_available);
    let group = &snapshot.group;

    assert_eq!(group.member_count, 2);
    assert_eq!(
        &group.members[0].name[..usize::from(group.members[0].name_len)],
        b"Eidolon"
    );
    assert!(group.members[0].is_leader);
    assert_eq!(
        &group.members[1].name[..usize::from(group.members[1].name_len)],
        b"ZiLo"
    );
    assert!(!group.members[1].is_leader);
    assert_eq!(group.is_group_open, Some(true));
}

#[test]
fn capture_into_uses_caller_storage_on_a_small_stack() {
    let memory = FakeMemory::gameplay_with_collections();
    let output = Box::new(RawStateSnapshot::empty());
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut output = output;
            StateWalker::new(&memory, BASE)
                .capture_into(THREAD_ID, &mut output)
                .unwrap();
            assert!(output.character_available);
            assert!(output.character.inventory_available);
            assert!(output.character.spellbook_available);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn captures_visible_world_objects_into_caller_storage() {
    let memory = FakeMemory::gameplay();
    let mut objects = RawObjects::empty();

    StateWalker::new(&memory, BASE)
        .capture_objects(THREAD_ID, Some((11, 22)), &mut objects)
        .unwrap();

    assert_eq!(objects.count, 1);
    let Some(RawWorldObject::Player {
        id,
        name,
        name_len,
        x,
        y,
        direction,
        is_hidden: _,
        visual,
    }) = objects.entries[0]
    else {
        panic!("expected player object");
    };
    assert_eq!(id, 0x1122_3344);
    assert_eq!(&name[..usize::from(name_len)], b"SiLo");
    assert_eq!((x, y, direction), (11, 22, 2));
    assert!(matches!(
        visual,
        Some(RawPlayerVisual::Human(RawHumanVisual {
            head_sprite: 17,
            skin_color: 4,
            weapon_sprite: 106,
            accessory3_sprite: 111,
            face_shape: 12,
            ..
        }))
    ));
}

#[test]
fn captures_ground_item_dye_color() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(OBJECT + 0x2C, 8);
    memory.u16(OBJECT + 0x7C, 0x8123);
    memory.u8(OBJECT + 0xB4, 7);
    let mut objects = RawObjects::empty();

    StateWalker::new(&memory, BASE)
        .capture_objects(THREAD_ID, Some((11, 22)), &mut objects)
        .unwrap();

    assert_eq!(objects.count, 1);
    assert_eq!(
        objects.entries[0],
        Some(RawWorldObject::Item {
            id: 0x1122_3344,
            sprite: 0x8123,
            dye_color: 7,
            x: 11,
            y: 22,
            z_index: 0,
        })
    );
}

#[test]
fn rejects_a_cyclic_world_object_tree() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(NODE, NODE);
    let mut objects = RawObjects::empty();

    assert_eq!(
        StateWalker::new(&memory, BASE).capture_objects(THREAD_ID, Some((11, 22)), &mut objects,),
        Err(StateReadError::ObjectTreeDepthExceeded {
            depth: MAX_TREE_DEPTH + 1,
            limit: MAX_TREE_DEPTH,
        })
    );
}

#[test]
fn captures_world_tree_larger_than_the_legacy_traversal_limit() {
    let mut memory = FakeMemory::gameplay();
    memory.right_linked_world_tree(1_025);
    let mut objects = RawObjects::empty();

    StateWalker::new(&memory, BASE)
        .capture_objects(THREAD_ID, Some((11, 22)), &mut objects)
        .unwrap();

    assert_eq!(objects.count, 0);
}

#[test]
fn reports_world_tree_node_limit() {
    let mut memory = FakeMemory::gameplay();
    memory.right_linked_world_tree(MAX_OBJECT_TREE_NODES + 1);
    let mut objects = RawObjects::empty();

    assert_eq!(
        StateWalker::new(&memory, BASE).capture_objects(THREAD_ID, Some((11, 22)), &mut objects),
        Err(StateReadError::ObjectTreeNodeLimitExceeded {
            visited: MAX_OBJECT_TREE_NODES + 1,
            limit: MAX_OBJECT_TREE_NODES,
        })
    );
}

#[test]
fn reports_world_object_capacity_limit() {
    let mut memory = FakeMemory::gameplay();
    memory.right_linked_item_tree(MAX_WORLD_OBJECTS + 1);
    let mut objects = RawObjects::empty();

    assert_eq!(
        StateWalker::new(&memory, BASE).capture_objects(THREAD_ID, Some((11, 22)), &mut objects),
        Err(StateReadError::WorldObjectCapacityExceeded {
            required: MAX_WORLD_OBJECTS + 1,
            capacity: MAX_WORLD_OBJECTS,
        })
    );
}

#[test]
fn non_human_appearance_is_unavailable() {
    let mut memory = FakeMemory::gameplay();
    memory.u8(OBJECT + 0x104, 0);

    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    assert!(snapshot.character.appearance.is_none());
    assert!(!snapshot.character.is_hidden);
}

#[test]
fn hidden_recapture_retains_the_last_visible_appearance() {
    let mut memory = FakeMemory::gameplay();
    let mut snapshot = RawStateSnapshot::empty();
    StateWalker::new(&memory, BASE)
        .capture_into(THREAD_ID, &mut snapshot)
        .unwrap();
    let visible = snapshot.character.appearance;

    memory.u8(OBJECT + PLAYER_TRANSLUCENT_STATE_OFFSET, 1);
    memory.u8(OBJECT + 0x104, 0);
    StateWalker::new(&memory, BASE)
        .capture_into(THREAD_ID, &mut snapshot)
        .unwrap();

    assert!(snapshot.character.is_hidden);
    assert_eq!(snapshot.character.appearance, visible);
}

#[test]
fn action_lock_and_blinded_state_use_exact_client_values() {
    let mut memory = FakeMemory::gameplay();
    memory.u8(WORLD_USER + 0x108D, 0x07);
    memory.u8(WORLD_USER + 0x15C88, 0x02);

    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    let character = &snapshot.character;
    assert!(!character.is_action_restricted);
    assert!(!character.is_blinded);
}

#[test]
fn title_state_has_no_character() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(BASE + WORLD_PANE_POINTER_RVA as u32, 0);
    memory.u32(BASE + GUI_BACK_PANE_RVA, 0);
    memory.u32(BASE + MAIN_MENU_PANE_RVA, 0x1234);

    let walker = StateWalker::new(&memory, BASE);
    let snapshot = walker.capture(THREAD_ID).unwrap();
    assert_eq!(
        walker.capture_lifecycle(THREAD_ID).unwrap(),
        RawLifecycle::Title
    );
    assert_eq!(snapshot.lifecycle, RawLifecycle::Title);
    assert!(!snapshot.character_available);
}

#[test]
fn reconnect_dialog_takes_precedence_over_the_stable_world() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(BASE + EVENT_DISPATCHER_RVA, EVENT_DISPATCHER);
    memory.u32(EVENT_DISPATCHER + 0x64, EVENT_ENTRIES);
    memory.i32(EVENT_DISPATCHER + 0x68, 1);
    memory.i32(EVENT_DISPATCHER + 0x6C, 1);
    memory.u32(EVENT_ENTRIES, RECONNECT_DIALOG);
    memory.u32(RECONNECT_DIALOG, BASE + RECONNECT_DIALOG_VTABLE_RVA);
    memory.u8(RECONNECT_DIALOG + 0x130, 1);
    memory.u8(RECONNECT_DIALOG + 0x188, 0x02);

    let walker = StateWalker::new(&memory, BASE);
    let snapshot = walker.capture(THREAD_ID).unwrap();
    assert_eq!(
        walker.capture_lifecycle(THREAD_ID).unwrap(),
        RawLifecycle::Disconnected
    );
    assert_eq!(snapshot.lifecycle, RawLifecycle::Disconnected);
    assert!(snapshot.character_available);
}

#[test]
fn hidden_or_unregistered_reconnect_dialog_is_not_disconnected() {
    for (visible, flags) in [(0, 0x02), (1, 0)] {
        let mut memory = FakeMemory::gameplay();
        memory.u32(BASE + EVENT_DISPATCHER_RVA, EVENT_DISPATCHER);
        memory.u32(EVENT_DISPATCHER + 0x64, EVENT_ENTRIES);
        memory.i32(EVENT_DISPATCHER + 0x68, 1);
        memory.i32(EVENT_DISPATCHER + 0x6C, 1);
        memory.u32(EVENT_ENTRIES, RECONNECT_DIALOG);
        memory.u32(RECONNECT_DIALOG, BASE + RECONNECT_DIALOG_VTABLE_RVA);
        memory.u8(RECONNECT_DIALOG + 0x130, visible);
        memory.u8(RECONNECT_DIALOG + 0x188, flags);

        let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
        assert_eq!(snapshot.lifecycle, RawLifecycle::InGame);
    }
}

#[test]
fn rejects_an_invalid_event_pane_list() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(BASE + EVENT_DISPATCHER_RVA, EVENT_DISPATCHER);
    memory.u32(EVENT_DISPATCHER + 0x64, EVENT_ENTRIES);
    memory.i32(EVENT_DISPATCHER + 0x68, 2);
    memory.i32(EVENT_DISPATCHER + 0x6C, 1);

    assert_eq!(
        StateWalker::new(&memory, BASE).capture(THREAD_ID),
        Err(StateReadError::InvalidPaneList)
    );
}

#[test]
fn captures_a_pointer_backed_map_name() {
    let mut memory = FakeMemory::gameplay();
    memory.u32(GUI_BACK + GUI_BACK_PANE_ADJUSTMENT + 0x4CAC, MAP_NAME_TEXT);
    memory.bytes(MAP_NAME_TEXT, b"Rucesion Inn\0");

    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    let map_name = snapshot.character.location.unwrap().name.unwrap();
    assert_eq!(
        &map_name.bytes[..usize::from(map_name.length)],
        b"Rucesion Inn"
    );
}

#[test]
fn captures_inventory_and_equipment_slots() {
    let memory = FakeMemory::gameplay_with_collections();
    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    let character = &snapshot.character;
    assert!(character.inventory_available);
    assert!(character.equipment_available);
    let inventory_items = character.inventory.items;
    let inventory = inventory_items[0].unwrap();
    let equipment = character.equipment.items[0].unwrap();

    assert_eq!(
        (inventory.slot, inventory.sprite, inventory.dye_color),
        (1, 0x8123, 7)
    );
    assert_eq!(
        &inventory.name.bytes[..usize::from(inventory.name.length)],
        b"Dark Belt [3]"
    );
    assert_eq!(
        (
            inventory.quantity,
            inventory.durability,
            inventory.max_durability
        ),
        (3, 41, 50)
    );
    assert!(inventory.can_stack);
    assert!(inventory_items[59].is_none());
    assert_eq!(
        (equipment.slot, equipment.sprite, equipment.dye_color),
        (1, 0x9234, 2)
    );
    assert_eq!(
        (equipment.durability, equipment.max_durability),
        (900, 1_000)
    );
}

#[test]
fn captures_spellbook_and_skillbook_slots() {
    let memory = FakeMemory::gameplay_with_collections();
    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    let character = &snapshot.character;
    assert!(character.skillbook_available);
    assert!(character.spellbook_available);
    let skill = character.skillbook.skills[3].unwrap();
    let spell = character.spellbook.spells[6].unwrap();

    assert_eq!((skill.slot, skill.icon), (4, 0x0123));
    assert_eq!(
        (skill.cooldown_started_at, skill.cooldown_ends_at),
        (1_000, 2_000)
    );
    assert!(skill.cooldown_visual_active);
    assert_eq!(
        (spell.slot, spell.icon, spell.argument_type),
        (7, 0x0456, 1)
    );
    let prompt = spell.prompt.unwrap();
    assert_eq!(
        &prompt.bytes[..usize::from(prompt.length)],
        b"Target \xFFname?"
    );
    assert_eq!(spell.cast_lines, 4);
    assert!(spell.action_delay_active);
}

#[test]
fn captures_active_spell_effects() {
    let memory = FakeMemory::gameplay_with_collections();
    let snapshot = StateWalker::new(&memory, BASE).capture(THREAD_ID).unwrap();
    assert!(snapshot.character_available);
    let effects = snapshot.character.effects.unwrap().effects;

    assert_eq!(
        (effects[0].unwrap().icon, effects[0].unwrap().duration),
        (300, 6)
    );
    assert!(effects[1].is_none());
    assert_eq!(
        (effects[3].unwrap().icon, effects[3].unwrap().duration),
        (301, 2)
    );
}

#[test]
fn rejects_invalid_spell_effect_duration() {
    let mut memory = FakeMemory::gameplay_with_collections();
    memory.u8(EFFECT_PANE + 0x1A4, 7);

    assert_eq!(
        StateWalker::new(&memory, BASE).capture(THREAD_ID),
        Err(StateReadError::InvalidCollection)
    );
}

#[test]
fn rejects_capture_from_the_wrong_thread() {
    let memory = FakeMemory::gameplay();
    assert_eq!(
        StateWalker::new(&memory, BASE).capture(THREAD_ID + 1),
        Err(StateReadError::WrongThread {
            expected: THREAD_ID,
            actual: THREAD_ID + 1,
        })
    );
}
