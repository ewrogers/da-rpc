/// Module-relative address of the main-thread event dispatcher tick.
pub const EVENT_DISPATCHER_TICK_RVA: usize = 0x0006_4180;

/// Complete entry instructions that must be present before installing the
/// event dispatcher tick detour.
pub const EVENT_DISPATCHER_TICK_ENTRY: [u8; 5] = [0x55, 0x8B, 0xEC, 0x6A, 0xFF];

/// Module-relative address of the central decoded-event dispatcher.
pub const EVENT_DISPATCH_RVA: usize = 0x0006_47C0;

/// Complete entry instructions that must be present before installing the
/// decoded-event dispatcher detour.
pub const EVENT_DISPATCH_ENTRY: [u8; 5] = [0x55, 0x8B, 0xEC, 0x6A, 0xFF];

/// Module-relative address of the accepted server map-size handler.
pub const MAP_SIZE_HANDLER_RVA: usize = 0x001F_1BF0;

/// Complete entry instructions that must be present before installing the
/// map-size handler detour.
pub const MAP_SIZE_HANDLER_ENTRY: [u8; 5] = [0x55, 0x8B, 0xEC, 0x6A, 0xFF];

/// Module-relative address of the live world-pane interface pointer.
pub const WORLD_PANE_POINTER_RVA: usize = 0x0033_D964;

/// Offset of the stored interface subobject within the complete `WorldPane`.
///
/// Subtract this value from the stored interface pointer before calling native
/// `WorldPane` methods or reading fields on the complete object.
pub const WORLD_PANE_ADJUSTMENT: usize = 0x2EC;

/// Byte offset of the native queued-route active flag in `WorldPane`.
pub const WORLD_PANE_ROUTE_ACTIVE_OFFSET: usize = 0x294;

/// Module-relative address of the native turn request helper.
pub const TURN_RVA: usize = 0x001F_0900;

/// Module-relative address of the native local-player resolver.
pub const SELF_OBJECT_RVA: usize = 0x001E_EDB0;

/// Module-relative address of the native collision-checked single-step helper.
pub const WALK_RVA: usize = 0x001F_09E0;

/// Module-relative address of the native movement-state reset helper.
pub const RESET_MOVEMENT_RVA: usize = 0x001F_4900;

/// Module-relative address of the native queued-path advancement helper.
pub const ADVANCE_PATH_RVA: usize = 0x001F_4990;

/// Module-relative address of the native exact-tile route builder.
pub const BUILD_PATH_RVA: usize = 0x001F_4DE0;

#[cfg(test)]
mod tests {
    use super::{
        ADVANCE_PATH_RVA, BUILD_PATH_RVA, EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA,
        EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA, MAP_SIZE_HANDLER_ENTRY,
        MAP_SIZE_HANDLER_RVA, RESET_MOVEMENT_RVA, SELF_OBJECT_RVA, TURN_RVA, WALK_RVA,
        WORLD_PANE_ADJUSTMENT, WORLD_PANE_POINTER_RVA, WORLD_PANE_ROUTE_ACTIVE_OFFSET,
    };

    #[test]
    fn tick_target_contract_is_stable() {
        assert_eq!(EVENT_DISPATCHER_TICK_RVA, 0x0006_4180);
        assert_eq!(EVENT_DISPATCHER_TICK_ENTRY, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
    }

    #[test]
    fn map_size_target_contract_is_stable() {
        assert_eq!(MAP_SIZE_HANDLER_RVA, 0x001F_1BF0);
        assert_eq!(MAP_SIZE_HANDLER_ENTRY, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
    }

    #[test]
    fn event_dispatch_target_contract_is_stable() {
        assert_eq!(EVENT_DISPATCH_RVA, 0x0006_47C0);
        assert_eq!(EVENT_DISPATCH_ENTRY, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
    }

    #[test]
    fn movement_contract_is_stable() {
        assert_eq!(WORLD_PANE_POINTER_RVA, 0x0033_D964);
        assert_eq!(WORLD_PANE_ADJUSTMENT, 0x2EC);
        assert_eq!(WORLD_PANE_ROUTE_ACTIVE_OFFSET, 0x294);
        assert_eq!(SELF_OBJECT_RVA, 0x001E_EDB0);
        assert_eq!(TURN_RVA, 0x001F_0900);
        assert_eq!(WALK_RVA, 0x001F_09E0);
        assert_eq!(RESET_MOVEMENT_RVA, 0x001F_4900);
        assert_eq!(ADVANCE_PATH_RVA, 0x001F_4990);
        assert_eq!(BUILD_PATH_RVA, 0x001F_4DE0);
    }
}
