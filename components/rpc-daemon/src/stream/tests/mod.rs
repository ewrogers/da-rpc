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
    ExchangeState as ModelExchangeState, ExchangeUpdate,
    FieldMapDestination as ModelFieldMapDestination, FieldMapSelection as ModelFieldMapSelection,
    FieldMapState as ModelFieldMapState, FieldMapUpdate, Gender,
    InventoryItem as ModelInventoryItem, LegendIcon as ModelLegendIcon,
    LegendMark as ModelLegendMark, LegendUpdate, LifecycleUpdate, LocationUpdate, MapChange,
    MessageDialog as ModelMessageDialog, MessageDialogsState as ModelMessageDialogsState,
    MessageKind, MovementUpdate, PlannedRoute, Skill as ModelSkill, SlotUpdate,
    Spell as ModelSpell, SpellCancellationSource as ModelSpellCancellationSource,
    SpellCastArguments as ModelSpellCastArguments, SpellTargetType, StateUpdate, StatusUpdate,
    TilePosition as ModelTilePosition,
};

fn observed_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_775_000_000, 0).unwrap()
}

#[test]
fn rejected_observations_close_streams_with_resync_required() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [0xAB; 16],
    };
    let (sender, receiver) = tokio::sync::broadcast::channel(4);
    let body = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let response = response(42, identity, 3, 2, receiver).into_response();
            sender
                .send(PublishedEvent::ResyncRequired { pid: 42, identity })
                .unwrap();
            drop(sender);
            axum::body::to_bytes(response.into_body(), 16 * 1024).await
        })
        .unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.contains("event: stream.ready"));
    assert!(body.contains("event: stream.resync_required"));
    assert!(body.contains("\"last_event_sequence\":2"));
}

fn exchange_state() -> ModelExchangeState {
    ModelExchangeState {
        id: 77,
        partner: "ZiLo".into(),
        local: ModelExchangeOffer::default(),
        other: ModelExchangeOffer::default(),
    }
}
