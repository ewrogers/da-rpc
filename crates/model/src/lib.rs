//! Shared domain model for daRPC.

mod dialog;
mod emote;
mod entity;
mod event;
mod exchange;
mod field_map;
mod group;
mod legend;
mod message;
mod object;
mod player;
mod sequence;
mod snapshot;
mod who;

pub use event::{
    AbilityUpdate, ActionUpdate, ApplyEventError, AudioUpdate, ClientCommand, CollectionBatch,
    CollectionChange, CollectionKind, CoreStatus, CurrentVitals, EffectUpdate, InventoryUpdate,
    LifecycleUpdate, LocationUpdate, MapChange, MapExclusionsUpdate, MovementUpdate, PlannedRoute,
    ProgressionStatus, SkillbookUpdate, SlotUpdate, SpellCancellationSource, SpellCastArguments,
    SpellbookUpdate, StateEvent, StateUpdate, StatusUpdate, TilePosition, WalkMode,
};

pub use group::{
    GroupInvitation, GroupInvitationCloseReason, GroupMember, GroupState, GroupUpdate,
};
pub use legend::{LegendIcon, LegendMark, LegendUpdate};

pub use dialog::{
    DialogChoice, DialogCloseReason, DialogInput, DialogInteraction, DialogItem, DialogKind,
    DialogNavigation, DialogSlot, DialogSpeaker, DialogSpriteType, DialogState, DialogSubmission,
    DialogTarget, DialogUpdate,
};

pub use emote::{NAMED_EMOTES, NamedEmote, emote_code, is_client_emote_code};
pub use entity::EntityUpdate;
pub use exchange::{ExchangeItem, ExchangeOffer, ExchangeParty, ExchangeState, ExchangeUpdate};
pub use field_map::{FieldMapDestination, FieldMapSelection, FieldMapState, FieldMapUpdate};

pub use message::{ClientMessage, MessageKind};

pub use object::{CreatureKind, Direction, HumanVisual, ObjectUpdate, PlayerVisual, WorldObject};
pub use player::{
    CharacterProfileUpdate, Nation, PlayerEquipmentItem, PlayerIdentity, PlayerInspectionChanges,
    PlayerInspectionTrigger, PlayerProfile, PlayerUpdate,
};
pub use sequence::SequenceNumber;

pub use snapshot::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Effect, EffectDuration, Element, EquipmentItem, EquipmentSlot, Gender,
    InventoryItem, MapExclusions, MapLocation, Skill, Spell, SpellTargetType,
};

pub use who::{UserState, WhoList, WhoPlayer};
