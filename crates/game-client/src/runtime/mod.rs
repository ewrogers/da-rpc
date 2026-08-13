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

/// Module-relative address of the normal floating game-message append helper.
pub const GAME_MESSAGE_APPEND_RVA: usize = 0x0008_03A0;

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

/// Byte offset of the native queued-route vector in `WorldPane`.
#[cfg(any(windows, test))]
pub const WORLD_PANE_ROUTE_VECTOR_OFFSET: usize = 0x2A8;

/// Byte offset of the number of unconsumed queued-route records.
#[cfg(any(windows, test))]
pub const WORLD_PANE_ROUTE_STEP_COUNT_OFFSET: usize = 0x2B8;

/// Byte offset of the current map identifier in `WorldPane`.
#[cfg(any(windows, test))]
pub const WORLD_PANE_MAP_ID_OFFSET: usize = 0x26C;

/// Byte offset of the native entity-pursuit target identifier in `WorldPane`.
pub const WORLD_PANE_PURSUIT_TARGET_ID_OFFSET: usize = 0x2BC;

/// Module-relative address of the native turn request helper.
pub const TURN_RVA: usize = 0x001F_0900;

/// Module-relative address of the native local-player resolver.
pub const SELF_OBJECT_RVA: usize = 0x001E_EDB0;

/// Module-relative address of the native collision-checked single-step helper.
pub const WALK_RVA: usize = 0x001F_09E0;

/// Module-relative address of the native directional movement validator.
pub const MAP_CAN_MOVE_DIRECTION_RVA: usize = 0x001E_FFE0;

/// Module-relative address of the native movement-state reset helper.
pub const RESET_MOVEMENT_RVA: usize = 0x001F_4900;

/// Module-relative address of the native queued-path advancement helper.
pub const ADVANCE_PATH_RVA: usize = 0x001F_4990;

/// Module-relative address of the native exact-tile route builder.
pub const BUILD_PATH_RVA: usize = 0x001F_4DE0;

/// Module-relative address of the native breadth-first route builder.
pub const BUILD_BREADTH_FIRST_PATH_RVA: usize = 0x001F_4E30;

/// Module-relative address of the native 12-byte route-record append helper.
#[cfg(any(windows, test))]
pub const ROUTE_STEP_PUSH_BACK_RVA: usize = 0x001F_59A0;

/// Complete entry instructions required before observing native route builds.
pub const BUILD_BREADTH_FIRST_PATH_ENTRY: [u8; 9] =
    [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xC0, 0x00, 0x00, 0x00];

/// Module-relative address of the route builder's static-collision call.
pub const ROUTE_COLLISION_CALL_RVA: usize = 0x001F_5068;

/// Complete relative call replaced by the combined live/raw collision hook.
pub const ROUTE_COLLISION_CALL: [u8; 5] = [0xE8, 0x73, 0xAF, 0xFF, 0xFF];

/// Module-relative address of the queued route's local-step call.
pub const QUEUED_STEP_CALL_RVA: usize = 0x001F_4A46;

/// Complete relative call replaced by the failed-step recovery hook.
pub const QUEUED_STEP_CALL: [u8; 5] = [0xE8, 0x95, 0xBF, 0xFF, 0xFF];

/// Module-relative address of the normal living-object interaction producer.
pub const WORLD_ENTITY_INTERACTION_RVA: usize = 0x001F_4730;

/// Module-relative address of the live event dispatcher pointer.
pub const EVENT_DISPATCHER_POINTER_RVA: usize = 0x002D_9220;

/// Complete Object Locator RVAs accepted for exact RTTI `NPCSession`.
pub const NPC_SESSION_COL_RVAS: [usize; 2] = [0x002A_0EF8, 0x002A_0F50];

/// Module-relative address of the null-safe live lower-tray pane accessor.
pub const GUI_BACK_PANE_GET_RVA: usize = 0x001A_9C40;

/// Module-relative address of the normal skill-entry activation routine.
pub const SKILL_ACTIVATE_RVA: usize = 0x0009_92F0;

/// Module-relative address of the normal inventory-slot activation routine.
pub const ITEM_ACTIVATE_RVA: usize = 0x0009_0960;

/// Module-relative address of the complete spell-delay controller pointer.
pub const SPELL_DELAY_CONTROL_PANE_POINTER_RVA: usize = 0x0033_FD78;

/// Byte offset of the active delayed-cast flag in `SpellDelayControlPane`.
pub const SPELL_DELAY_ACTIVE_OFFSET: usize = 0x8C94;

/// Module-relative address of the spell-delay controller accessor.
pub const SPELL_DELAY_CONTROL_PANE_GET_RVA: usize = 0x0009_3630;

/// Module-relative address of the targeted spell builder.
pub const SPELL_TARGET_RVA: usize = 0x0009_AB60;

/// Module-relative address of the client-side denied-spell lookup.
pub const SPELL_DENIED_RVA: usize = 0x0009_AC90;

/// Module-relative address of the no-argument spell builder.
pub const SPELL_NO_ARGS_RVA: usize = 0x0009_AD40;

/// Module-relative address of the completed-body spell-cast starter.
pub const SPELL_START_RVA: usize = 0x0009_B900;

/// Module-relative address of the shared outbound client-packet submission path.
pub const CLIENT_PACKET_SUBMIT_RVA: usize = 0x0016_3E00;

/// Module-relative address of the active network connection pointer.
pub const CLIENT_SOCKET_POINTER_RVA: usize = 0x0033_D958;

/// Complete entry instructions required before observing outbound packets.
pub const CLIENT_PACKET_SUBMIT_ENTRY: [u8; 9] =
    [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x38, 0x89, 0x4D, 0xC8];

/// Module-relative address of the transport queue submission routine.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_SUBMIT_RVA: usize = 0x0018_6210;

/// Complete entry instructions required before prioritizing transport packets.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_SUBMIT_ENTRY: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x1C];

/// Module-relative address of the transport queue front-pop routine.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_POP_RVA: usize = 0x0014_EBB0;

/// Complete entry instructions required before prioritizing transport packets.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_POP_ENTRY: [u8; 7] = [0x55, 0x8B, 0xEC, 0x51, 0x89, 0x4D, 0xFC];

/// Module-relative address of the transport queue empty-check routine.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_EMPTY_RVA: usize = 0x0014_EC60;

/// Complete entry instructions required before extending the queue empty check.
#[cfg(any(windows, test))]
pub const CLIENT_TRANSPORT_EMPTY_ENTRY: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08];

/// Module-relative address of the client main thread identifier.
pub const CLIENT_MAIN_THREAD_ID_RVA: usize = 0x0034_0400;

#[cfg(test)]
mod tests {
    use super::{
        ADVANCE_PATH_RVA, BUILD_BREADTH_FIRST_PATH_ENTRY, BUILD_BREADTH_FIRST_PATH_RVA,
        BUILD_PATH_RVA, CLIENT_MAIN_THREAD_ID_RVA, CLIENT_PACKET_SUBMIT_ENTRY,
        CLIENT_PACKET_SUBMIT_RVA, CLIENT_TRANSPORT_EMPTY_ENTRY, CLIENT_TRANSPORT_EMPTY_RVA,
        CLIENT_TRANSPORT_POP_ENTRY, CLIENT_TRANSPORT_POP_RVA, CLIENT_TRANSPORT_SUBMIT_ENTRY,
        CLIENT_TRANSPORT_SUBMIT_RVA, EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA,
        EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA, GUI_BACK_PANE_GET_RVA,
        MAP_CAN_MOVE_DIRECTION_RVA, MAP_SIZE_HANDLER_ENTRY, MAP_SIZE_HANDLER_RVA, QUEUED_STEP_CALL,
        QUEUED_STEP_CALL_RVA, RESET_MOVEMENT_RVA, ROUTE_COLLISION_CALL, ROUTE_COLLISION_CALL_RVA,
        ROUTE_STEP_PUSH_BACK_RVA, SELF_OBJECT_RVA, SKILL_ACTIVATE_RVA, SPELL_DELAY_ACTIVE_OFFSET,
        SPELL_DELAY_CONTROL_PANE_GET_RVA, SPELL_DELAY_CONTROL_PANE_POINTER_RVA, SPELL_DENIED_RVA,
        SPELL_NO_ARGS_RVA, SPELL_START_RVA, SPELL_TARGET_RVA, TURN_RVA, WALK_RVA,
        WORLD_PANE_ADJUSTMENT, WORLD_PANE_MAP_ID_OFFSET, WORLD_PANE_POINTER_RVA,
        WORLD_PANE_PURSUIT_TARGET_ID_OFFSET, WORLD_PANE_ROUTE_ACTIVE_OFFSET,
        WORLD_PANE_ROUTE_STEP_COUNT_OFFSET, WORLD_PANE_ROUTE_VECTOR_OFFSET,
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
    fn transport_priority_contract_is_stable() {
        assert_eq!(CLIENT_TRANSPORT_SUBMIT_RVA, 0x0018_6210);
        assert_eq!(
            CLIENT_TRANSPORT_SUBMIT_ENTRY,
            [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x1C]
        );
        assert_eq!(CLIENT_TRANSPORT_POP_RVA, 0x0014_EBB0);
        assert_eq!(
            CLIENT_TRANSPORT_POP_ENTRY,
            [0x55, 0x8B, 0xEC, 0x51, 0x89, 0x4D, 0xFC]
        );
        assert_eq!(CLIENT_TRANSPORT_EMPTY_RVA, 0x0014_EC60);
        assert_eq!(
            CLIENT_TRANSPORT_EMPTY_ENTRY,
            [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08]
        );
    }

    #[test]
    fn movement_contract_is_stable() {
        assert_eq!(WORLD_PANE_POINTER_RVA, 0x0033_D964);
        assert_eq!(WORLD_PANE_ADJUSTMENT, 0x2EC);
        assert_eq!(WORLD_PANE_ROUTE_ACTIVE_OFFSET, 0x294);
        assert_eq!(WORLD_PANE_ROUTE_VECTOR_OFFSET, 0x2A8);
        assert_eq!(WORLD_PANE_ROUTE_STEP_COUNT_OFFSET, 0x2B8);
        assert_eq!(WORLD_PANE_MAP_ID_OFFSET, 0x26C);
        assert_eq!(WORLD_PANE_PURSUIT_TARGET_ID_OFFSET, 0x2BC);
        assert_eq!(SELF_OBJECT_RVA, 0x001E_EDB0);
        assert_eq!(TURN_RVA, 0x001F_0900);
        assert_eq!(WALK_RVA, 0x001F_09E0);
        assert_eq!(MAP_CAN_MOVE_DIRECTION_RVA, 0x001E_FFE0);
        assert_eq!(RESET_MOVEMENT_RVA, 0x001F_4900);
        assert_eq!(ADVANCE_PATH_RVA, 0x001F_4990);
        assert_eq!(BUILD_PATH_RVA, 0x001F_4DE0);
        assert_eq!(BUILD_BREADTH_FIRST_PATH_RVA, 0x001F_4E30);
        assert_eq!(ROUTE_STEP_PUSH_BACK_RVA, 0x001F_59A0);
        assert_eq!(ROUTE_COLLISION_CALL_RVA, 0x001F_5068);
        assert_eq!(ROUTE_COLLISION_CALL, [0xE8, 0x73, 0xAF, 0xFF, 0xFF]);
        assert_eq!(QUEUED_STEP_CALL_RVA, 0x001F_4A46);
        assert_eq!(QUEUED_STEP_CALL, [0xE8, 0x95, 0xBF, 0xFF, 0xFF]);
        assert_eq!(
            BUILD_BREADTH_FIRST_PATH_ENTRY,
            [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xC0, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn skill_activation_contract_is_stable() {
        assert_eq!(GUI_BACK_PANE_GET_RVA, 0x001A_9C40);
        assert_eq!(SKILL_ACTIVATE_RVA, 0x0009_92F0);
    }

    #[test]
    fn spell_casting_contract_is_stable() {
        assert_eq!(SPELL_DELAY_CONTROL_PANE_POINTER_RVA, 0x0033_FD78);
        assert_eq!(SPELL_DELAY_ACTIVE_OFFSET, 0x8C94);
        assert_eq!(SPELL_DELAY_CONTROL_PANE_GET_RVA, 0x0009_3630);
        assert_eq!(SPELL_TARGET_RVA, 0x0009_AB60);
        assert_eq!(SPELL_DENIED_RVA, 0x0009_AC90);
        assert_eq!(SPELL_NO_ARGS_RVA, 0x0009_AD40);
        assert_eq!(SPELL_START_RVA, 0x0009_B900);
        assert_eq!(CLIENT_PACKET_SUBMIT_RVA, 0x0016_3E00);
        assert_eq!(CLIENT_MAIN_THREAD_ID_RVA, 0x0034_0400);
        assert_eq!(
            CLIENT_PACKET_SUBMIT_ENTRY,
            [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x38, 0x89, 0x4D, 0xC8]
        );
    }
}
