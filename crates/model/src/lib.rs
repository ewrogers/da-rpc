//! Shared domain model for daRPC.

mod event;
mod message;
mod object;
mod snapshot;

pub use event::{
    ApplyEventError, CollectionBatch, CollectionChange, CollectionKind, CoreStatus, CurrentVitals,
    EffectUpdate, InventoryUpdate, LocationUpdate, MapChange, MovementUpdate, ProgressionStatus,
    SkillbookUpdate, SlotUpdate, SpellbookUpdate, StateEvent, StateUpdate, StatusUpdate,
    TilePosition,
};

pub use message::{ClientMessage, MessageKind};

pub use object::{CreatureKind, Direction, ObjectUpdate, WorldObject};

pub use snapshot::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Effect, EffectDuration, Element, EquipmentItem, EquipmentSlot, Gender,
    InventoryItem, MapLocation, Skill, Spell, SpellTargetType,
};
