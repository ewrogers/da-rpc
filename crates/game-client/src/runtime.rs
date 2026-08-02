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

#[cfg(test)]
mod tests {
    use super::{
        EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA, EVENT_DISPATCHER_TICK_ENTRY,
        EVENT_DISPATCHER_TICK_RVA, MAP_SIZE_HANDLER_ENTRY, MAP_SIZE_HANDLER_RVA,
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
}
