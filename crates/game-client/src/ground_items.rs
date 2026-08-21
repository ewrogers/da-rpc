//! Version-specific launch patch contract for revealing ground items with Alt.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroundItemRevealPatch {
    pub capacity: u32,
    pub state_size: usize,
    pub state_entries_offset: usize,
    pub state_pane_offset: usize,
    pub collector_hook_rva: u32,
    pub collector_hook_expected: &'static [u8],
    pub frame_hook_rva: u32,
    pub frame_hook_expected: &'static [u8],
    pub key_down_hook_rva: u32,
    pub key_down_hook_expected: &'static [u8],
    pub key_up_hook_rva: u32,
    pub key_up_hook_expected: &'static [u8],
    pub static_render_mode_selector_rva: u32,
    pub static_render_mode_selector_expected: &'static [u8],
    pub input_get_event_manager_rva: u32,
    pub render_world_object_rva: u32,
    pub invalidate_pane_rva: u32,
    pub world_item_vtable_rva: u32,
    pub collector_stub_template: &'static [u8],
    pub frame_stub_template: &'static [u8],
    pub key_down_stub_template: &'static [u8],
    pub key_up_stub_template: &'static [u8],
}

const COLLECTOR_STUB_TEMPLATE: &[u8] = b"\x55\x89\xE5\x83\xEC\x08\x53\x56\x57\x89\xCE\xFF\x75\x18\xFF\x75\x14\xFF\x75\x10\xFF\x75\x0C\xFF\x75\x08\x89\xF1\xE8\x72\x00\x00\x00\x89\x45\xFC\x8B\xBE\xE0\x00\x00\x00\x3B\xBE\xE4\x00\x00\x00\x73\x55\x8B\x17\x85\xD2\x74\x4A\x81\x3A\x78\x56\x34\x12\x75\x42\x83\xBA\xB0\x00\x00\x00\x01\x75\x39\xA1\x00\x10\x11\x11\x3D\xFF\x00\x00\x00\x73\x32\x6B\xD8\x0C\x81\xC3\x00\x11\x11\x11\x8B\x0F\x89\x0B\x8B\x4F\x04\x89\x4B\x04\x8B\x4F\x08\x89\x4B\x08\xFF\x05\x00\x10\x11\x11\x89\x35\x04\x10\x11\x11\x8B\x45\x08\xA3\x08\x10\x11\x11\x83\xC7\x0C\xEB\xA3\x8B\x45\xFC\x5F\x5E\x5B\x89\xEC\x5D\xC2\x14\x00\x55\x89\xE5\x6A\xFF\xE9\x63\xFF\xFF\x11";

const FRAME_STUB_TEMPLATE: &[u8] = b"\x55\x89\xE5\x83\xEC\x10\x53\x56\x57\x89\xCE\x89\x35\x28\x10\x11\x11\xC7\x05\x00\x10\x11\x11\x00\x00\x00\x00\x89\xF1\xE8\x8D\x00\x00\x00\x89\x45\xFC\xE8\xD6\xFF\xFF\xEF\x85\xC0\x74\x77\xF6\x80\x34\x04\x00\x00\x01\x74\x6E\x31\xFF\x3B\x3D\x00\x10\x11\x11\x73\x64\x6B\xC7\x0C\x8D\x98\x00\x11\x11\x11\x8B\x13\x85\xD2\x74\x52\x81\x3A\x78\x56\x34\x12\x75\x4A\x83\xBA\xB0\x00\x00\x00\x01\x75\x41\x89\x55\xF4\x8B\x82\xB0\x00\x00\x00\x89\x45\xF8\xC7\x82\xB0\x00\x00\x00\x03\x00\x00\x00\x8B\x0D\x04\x10\x11\x11\x8B\x81\xBC\x02\x00\x00\x8B\x55\xF4\x03\x42\x2C\x50\x53\xFF\x35\x08\x10\x11\x11\xE8\x6A\xFF\xFF\xF0\x8B\x55\xF4\x8B\x45\xF8\x89\x82\xB0\x00\x00\x00\x47\xEB\x94\x8B\x45\xFC\x5F\x5E\x5B\x89\xEC\x5D\xC3\x55\x89\xE5\x83\xEC\x1C\xE9\x46\xFF\xFF\xF1";

const KEY_DOWN_STUB_TEMPLATE: &[u8] = b"\x55\x89\xE5\x83\xEC\x08\x56\x89\xCE\xFF\x75\x14\xFF\x75\x10\xFF\x75\x0C\xFF\x75\x08\x89\xF1\xE8\x2E\x00\x00\x00\x89\x45\xFC\x0F\xB6\x45\x08\x83\xF8\x38\x74\x07\x3D\xB8\x00\x00\x00\x75\x11\x8B\x0D\x28\x10\x11\x11\x85\xC9\x74\x07\x6A\x00\xE8\xC0\xFF\xFF\xE2\x8B\x45\xFC\x5E\x89\xEC\x5D\xC2\x10\x00\x55\x89\xE5\x6A\xFF\xE9\xAC\xFF\xFF\xE3";

const KEY_UP_STUB_TEMPLATE: &[u8] = b"\x55\x89\xE5\x83\xEC\x08\x56\x89\xCE\xFF\x75\x14\xFF\x75\x10\xFF\x75\x0C\xFF\x75\x08\x89\xF1\xE8\x2E\x00\x00\x00\x89\x45\xFC\x0F\xB6\x45\x08\x83\xF8\x38\x74\x07\x3D\xB8\x00\x00\x00\x75\x11\x8B\x0D\x28\x10\x11\x11\x85\xC9\x74\x07\x6A\x00\xE8\xC0\xFF\xFF\xE1\x8B\x45\xFC\x5E\x89\xEC\x5D\xC2\x10\x00\x55\x89\xE5\x6A\xFF\xE9\xAC\xFF\xFF\xE3";

pub const GROUND_ITEM_REVEAL_PATCH: GroundItemRevealPatch = GroundItemRevealPatch {
    capacity: 255,
    state_size: 0x100 + 255 * 12,
    state_entries_offset: 0x100,
    state_pane_offset: 0x28,
    collector_hook_rva: 0x001D_3740,
    collector_hook_expected: &[0x55, 0x8B, 0xEC, 0x6A, 0xFF],
    frame_hook_rva: 0x001C_E280,
    frame_hook_expected: &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x1C],
    key_down_hook_rva: 0x0006_7C10,
    key_down_hook_expected: &[0x55, 0x8B, 0xEC, 0x6A, 0xFF],
    key_up_hook_rva: 0x0006_7E30,
    key_up_hook_expected: &[0x55, 0x8B, 0xEC, 0x6A, 0xFF],
    static_render_mode_selector_rva: 0x001E_487D,
    static_render_mode_selector_expected: &[
        0x8B, 0x55, 0xD0, 0x0F, 0xB6, 0x82, 0xB9, 0x00, 0x00, 0x00, 0x25, 0x80, 0x00, 0x00, 0x00,
        0x74, 0x09, 0xC7, 0x45, 0xE8, 0x6D, 0x00, 0x00, 0x00, 0xEB, 0x16, 0x8B, 0x4D, 0xD0, 0x0F,
        0xB6, 0x91, 0xB9, 0x00, 0x00, 0x00, 0x83, 0xE2, 0x40, 0x74, 0x07, 0xC7, 0x45, 0xE8, 0x03,
        0x00, 0x00, 0x00,
    ],
    input_get_event_manager_rva: 0x0002_7380,
    render_world_object_rva: 0x001D_3190,
    invalidate_pane_rva: 0x0014_9F60,
    world_item_vtable_rva: 0x0028_B1AC,
    collector_stub_template: COLLECTOR_STUB_TEMPLATE,
    frame_stub_template: FRAME_STUB_TEMPLATE,
    key_down_stub_template: KEY_DOWN_STUB_TEMPLATE,
    key_up_stub_template: KEY_UP_STUB_TEMPLATE,
};

#[cfg(test)]
mod tests {
    use super::GROUND_ITEM_REVEAL_PATCH;

    #[test]
    fn ground_item_reveal_contract_is_complete() {
        let patch = GROUND_ITEM_REVEAL_PATCH;
        assert_eq!(patch.capacity, 255);
        assert_eq!(patch.state_size, 3_316);
        assert_eq!(patch.collector_stub_template.len(), 157);
        assert_eq!(patch.frame_stub_template.len(), 186);
        assert_eq!(patch.key_down_stub_template.len(), 84);
        assert_eq!(patch.key_up_stub_template.len(), 84);
        assert_eq!(patch.collector_hook_expected.len(), 5);
        assert_eq!(patch.frame_hook_expected.len(), 6);
        assert_eq!(patch.key_down_hook_expected.len(), 5);
        assert_eq!(patch.key_up_hook_expected.len(), 5);
        assert_eq!(patch.static_render_mode_selector_expected.len(), 48);
    }
}
