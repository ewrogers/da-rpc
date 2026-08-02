//! Shared domain model for daRPC.

mod event;
mod object;
mod snapshot;

pub use event::{
    ApplyEventError, CoreStatus, CurrentVitals, EffectUpdate, LocationUpdate, MapChange,
    ProgressionStatus, StateEvent, StateUpdate, StatusUpdate,
};

pub use object::{CreatureKind, Direction, ObjectUpdate, WorldObject};

pub use snapshot::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Effect, EffectDuration, Element, EquipmentItem, EquipmentSlot, Gender,
    InventoryItem, MapLocation, Skill, Spell, SpellTargetType,
};
