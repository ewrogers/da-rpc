/// Module-relative address of the main-thread event dispatcher tick.
pub const EVENT_DISPATCHER_TICK_RVA: usize = 0x0006_4180;

/// Complete entry instructions that must be present before installing the
/// event dispatcher tick detour.
pub const EVENT_DISPATCHER_TICK_ENTRY: [u8; 5] = [0x55, 0x8B, 0xEC, 0x6A, 0xFF];

#[cfg(test)]
mod tests {
    use super::{EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA};

    #[test]
    fn tick_target_contract_is_stable() {
        assert_eq!(EVENT_DISPATCHER_TICK_RVA, 0x0006_4180);
        assert_eq!(EVENT_DISPATCHER_TICK_ENTRY, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
    }
}
