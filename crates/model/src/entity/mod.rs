use crate::WorldObject;

/// A transient visual event involving a living world entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityUpdate {
    Animated {
        entity: WorldObject,
        animation: u8,
        duration_10ms: u16,
    },
    Effect {
        entity: WorldObject,
        effect: u16,
        source: Option<WorldObject>,
        frame_interval_ms: Option<i16>,
    },
    Damaged {
        entity: WorldObject,
        health_percent: u8,
    },
}
