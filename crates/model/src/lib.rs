//! Shared domain model for daRPC.

mod dialog;
mod emote;
mod entity;
mod event;
mod group;
mod message;
mod object;
mod snapshot;

pub use event::{
    AbilityUpdate, ActionUpdate, ApplyEventError, CollectionBatch, CollectionChange,
    CollectionKind, CoreStatus, CurrentVitals, EffectUpdate, InventoryUpdate, LocationUpdate,
    MapChange, MovementUpdate, ProgressionStatus, SkillbookUpdate, SlotUpdate,
    SpellCancellationSource, SpellCastArguments, SpellbookUpdate, StateEvent, StateUpdate,
    StatusUpdate, TilePosition,
};

pub use group::{
    GroupInvitation, GroupInvitationCloseReason, GroupMember, GroupState, GroupUpdate,
};

pub use dialog::{
    DialogChoice, DialogCloseReason, DialogInput, DialogInteraction, DialogItem, DialogKind,
    DialogNavigation, DialogSlot, DialogSpeaker, DialogSpriteType, DialogState, DialogSubmission,
    DialogTarget, DialogUpdate,
};

pub use emote::{NAMED_EMOTES, NamedEmote, emote_code, is_client_emote_code};
pub use entity::EntityUpdate;

pub use message::{ClientMessage, MessageKind};

pub use object::{CreatureKind, Direction, ObjectUpdate, WorldObject};

pub use snapshot::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Effect, EffectDuration, Element, EquipmentItem, EquipmentSlot, Gender,
    InventoryItem, MapLocation, Skill, Spell, SpellTargetType,
};
