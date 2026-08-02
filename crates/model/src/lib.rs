//! Shared domain model for daRPC.

mod event;
mod snapshot;

pub use event::{
    ApplyEventError, CoreStatus, CurrentVitals, LocationUpdate, MapChange, ProgressionStatus,
    StateEvent, StateUpdate, StatusUpdate,
};

pub use snapshot::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Element, EquipmentItem, EquipmentSlot, Gender, InventoryItem, MapLocation,
    Skill, Spell, SpellTargetType,
};
