mod interaction;
mod messages;
mod state;
mod world;

use super::*;
use darpc_model::{
    AbilityUpdate, ActionUpdate, AudioUpdate, CharacterAppearance as ModelCharacterAppearance,
    CharacterClass, CharacterProgression, CharacterSnapshot, CharacterStats, CharacterVitals,
    ClientCommand as ModelClientCommand, ClientLifecycle, ClientMessage, ClientSnapshot,
    CollectionChange, CooldownStatus, CoreStatus, CurrentVitals, Effect, EffectDuration,
    EffectUpdate, EntityUpdate, ExchangeItem as ModelExchangeItem,
    ExchangeOffer as ModelExchangeOffer, ExchangeParty as ModelExchangeParty,
    ExchangeState as ModelExchangeState, ExchangeUpdate, Gender,
    InventoryItem as ModelInventoryItem, LegendIcon as ModelLegendIcon,
    LegendMark as ModelLegendMark, LegendUpdate, LifecycleUpdate, LocationUpdate, MapChange,
    MessageKind, MovementUpdate, PlannedRoute, Skill as ModelSkill, SlotUpdate,
    Spell as ModelSpell, SpellCancellationSource as ModelSpellCancellationSource,
    SpellCastArguments as ModelSpellCastArguments, SpellTargetType, StateUpdate, StatusUpdate,
    TilePosition as ModelTilePosition,
};

fn observed_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_775_000_000, 0).unwrap()
}

fn exchange_state() -> ModelExchangeState {
    ModelExchangeState {
        id: 77,
        partner: "ZiLo".into(),
        local: ModelExchangeOffer::default(),
        other: ModelExchangeOffer::default(),
    }
}
