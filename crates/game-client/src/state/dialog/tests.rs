use super::*;

const BASE: u32 = 0x1000_0000;
const ANSWER: u32 = 0x1000;
const MODEL: u32 = 0x2000;
const ITEMS: u32 = 0x6000;

struct Memory(Vec<u8>);

impl MemoryReader for Memory {
    fn read(&self, address: u32, output: &mut [u8]) -> bool {
        let Some(bytes) = self
            .0
            .get(address as usize..address as usize + output.len())
        else {
            return false;
        };
        output.copy_from_slice(bytes);
        true
    }
}

impl Memory {
    fn word(&mut self, address: u32, value: u32) {
        self.0[address as usize..address as usize + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn short(&mut self, address: u32, value: u16) {
        self.0[address as usize..address as usize + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn new(count: u16) -> Self {
        let mut memory = Self(vec![
            0;
            ITEMS as usize + MAX_DIALOG_ITEMS * ITEM_STRIDE as usize
        ]);
        memory.word(ANSWER, BASE + ITEM_DIALOG_VTABLE_RVA);
        memory.word(ANSWER + 0x638, MODEL);
        memory.word(MODEL + 0x14, ITEMS);
        memory.short(MODEL + 0x12, count);
        memory
    }
}

#[test]
fn native_selection_preserves_u16_rows_and_rechecks_quantities() {
    let mut memory = Memory::new(300);
    // Ordinary rows contain uninitialized extended-only bytes. Ignore them.
    memory.0[(ITEMS + 299 * ITEM_STRIDE + 0x0C) as usize] = 255;
    let walker = StateWalker::new(&memory, BASE);
    assert!(
        walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 1)
            .unwrap()
    );
    assert!(
        !walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 2)
            .unwrap()
    );
    assert!(
        !walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 300, 1)
            .unwrap()
    );
    assert!(
        !walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 0)
            .unwrap()
    );
    memory.short(MODEL + 0x10, 0x004B);
    memory.0[(ITEMS + 299 * ITEM_STRIDE + 0x0C) as usize] = 3;
    let walker = StateWalker::new(&memory, BASE);
    assert!(
        walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 3)
            .unwrap()
    );
    assert!(
        !walker
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 4)
            .unwrap()
    );
    memory.0[(ITEMS + 299 * ITEM_STRIDE + 0x0C) as usize] = 0;
    assert!(
        !StateWalker::new(&memory, BASE)
            .dialog_item_selection_is_valid(ANSWER, MODEL, 299, 1)
            .unwrap()
    );
}

#[test]
fn native_selection_rejects_invalid_models_and_unreadable_rows() {
    for corrupt in 0..6 {
        let mut memory = Memory::new(1);
        match corrupt {
            0 => memory.word(ANSWER, BASE + ITEM_DIALOG_VTABLE_RVA + 4),
            1 => memory.word(ANSWER + 0x638, MODEL + 4),
            2 => memory.short(MODEL + 0x12, 513),
            3 => memory.word(MODEL + 0x14, 0),
            4 => memory.word(MODEL + 0x14, ITEMS + 1),
            5 => memory.word(MODEL + 0x14, memory.0.len() as u32),
            _ => unreachable!(),
        }
        assert!(
            StateWalker::new(&memory, BASE)
                .dialog_item_selection_is_valid(ANSWER, MODEL, 0, 1)
                .is_err()
        );
    }
    let mut memory = Memory::new(1);
    memory.0[ITEMS as usize + 0x0D..ITEMS as usize + 0x10D].fill(b'x');
    assert!(
        !StateWalker::new(&memory, BASE)
            .dialog_item_selection_is_valid(ANSWER, MODEL, 0, 1)
            .unwrap()
    );
}
