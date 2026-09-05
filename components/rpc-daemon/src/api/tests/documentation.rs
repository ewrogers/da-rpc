use super::*;

#[test]
fn serves_the_openapi_contract_and_vendored_swagger_ui() {
    let openapi = json("/openapi.json");
    assert_eq!(openapi["openapi"], "3.1.0");
    assert_eq!(openapi["info"]["title"], "daRPC API");
    assert!(openapi["paths"]["/health"].is_object());
    assert!(openapi["paths"]["/clients"].is_object());
    assert!(openapi["paths"]["/maps/{map_id}/download"].is_object());
    assert!(
        openapi["paths"]["/maps/{map_id}/download"]["get"]["responses"]["200"]["content"]
            ["application/octet-stream"]
            .is_object()
    );
    for path in [
        "/clients/{client}/status",
        "/clients/{client}/items",
        "/clients/{client}/equipment",
        "/clients/{client}/spells",
        "/clients/{client}/skills",
        "/clients/{client}/effects",
        "/clients/{client}/objects",
        "/clients/{client}/messages",
        "/messages/send",
        "/clients/{client}/events",
        "/clients/{client}/turn",
        "/clients/{client}/walk",
        "/clients/{client}/skills/use",
        "/clients/{client}/skills/swap",
        "/clients/{client}/spells/cast",
        "/clients/{client}/spells/swap",
        "/clients/{client}/items/use",
        "/clients/{client}/items/drop",
        "/clients/{client}/items/give",
        "/clients/{client}/items/swap",
        "/clients/{client}/items/pickup",
        "/clients/{client}/equipment/unequip",
        "/clients/{client}/gold/drop",
        "/clients/{client}/gold/give",
        "/clients/{client}/exchange",
        "/clients/{client}/exchange/items",
        "/clients/{client}/exchange/gold",
        "/clients/{client}/exchange/accept",
        "/clients/{client}/exchange/cancel",
        "/clients/{client}/emote",
        "/clients/{client}/raw/send",
        "/clients/{client}/assail",
        "/clients/{client}/commands/diagnostic",
        "/clients/{client}/commands/{command_id}",
    ] {
        assert!(openapi["paths"][path].is_object(), "OpenAPI omitted {path}");
    }
    assert!(
        openapi["paths"]["/clients/{client}/raw/send"]["post"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("crash"))
    );
    let object_parameters = openapi["paths"]["/clients/{client}/objects"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert!(object_parameters.iter().any(|parameter| {
        parameter["name"] == "types" && parameter["in"] == "query" && parameter["required"] != true
    }));
    assert!(openapi["paths"]["/clients/{client}/snapshot"].is_null());
    assert!(openapi["paths"]["/clients/launch"].is_object());
    assert!(openapi["paths"]["/clients/{client}/load"].is_object());
    assert!(openapi["paths"]["/clients/{client}/unload"].is_object());
    let schemas = openapi["components"]["schemas"].as_object().unwrap();
    for name in [
        "HealthState",
        "HealthStatus",
        "RawDirection",
        "RawSendOptions",
        "ClientList",
        "ClientState",
        "ClientStatus",
        "ClientIdentity",
        "ConnectionMetadata",
        "ObservationMetadata",
        "GameStatus",
        "ActionSource",
        "PlannedRoute",
        "RouteTile",
        "ClientLifecycle",
        "CharacterStatus",
        "CharacterGender",
        "CharacterClass",
        "CharacterProgression",
        "CharacterStats",
        "CharacterVitals",
        "CharacterModifiers",
        "Element",
        "MapLocation",
        "Inventory",
        "InventoryItem",
        "Equipment",
        "EquipmentItem",
        "EquipmentSlot",
        "Spellbook",
        "Spell",
        "Skillbook",
        "Skill",
        "CooldownStatus",
        "SpellTargetType",
        "Effects",
        "Effect",
        "EffectDuration",
        "WorldObjects",
        "WorldObject",
        "Direction",
        "Messages",
        "Message",
        "MessageChannel",
        "LaunchOptions",
        "LoadResult",
        "LifecycleResult",
        "LifecycleAction",
        "UnloadResult",
        "ErrorState",
        "ErrorDetail",
        "ClientEvent",
        "ClientLifecycleChanged",
        "SoundPlayed",
        "MusicStarted",
        "MusicStopped",
        "StreamReady",
        "EventObservation",
        "EffectAdded",
        "EffectRemoved",
        "EffectChanged",
        "InventorySlotChanged",
        "SpellSlotChanged",
        "SkillSlotChanged",
        "CooldownStarted",
        "AbilityReady",
        "WalkingRouteChanged",
        "StreamResyncRequired",
        "StreamClosed",
        "DiagnosticOptions",
        "SkillSlotOptions",
        "SkillNameOptions",
        "UseSkillOptions",
        "SlotSelector",
        "SwapSlotsOptions",
        "SpellTargetOptions",
        "CastSpellBySlot",
        "CastSpellByName",
        "CastSpellOptions",
        "GiveItemOptions",
        "GiveGoldOptions",
        "SkillUsed",
        "SpellBegin",
        "SpellChant",
        "SpellCast",
        "SpellCastArguments",
        "SpellCancelled",
        "SpellCancellationSource",
        "SpellSucceeded",
        "SpellFailed",
        "SpellFailureReason",
        "SpellReceived",
        "ReceivedSpellKind",
        "ExchangeSnapshot",
        "ExchangeState",
        "ExchangeOffer",
        "ExchangeItem",
        "ExchangeParty",
        "ExchangeOpened",
        "ExchangeItemAdded",
        "ExchangeGoldChanged",
        "ExchangeAccepted",
        "ExchangeCompleted",
        "ExchangeCancelled",
        "AddExchangeItemOptions",
        "SetExchangeGoldOptions",
        "CommandStatus",
        "CommandKind",
        "CommandState",
        "CommandFailure",
        "SendMessageChannel",
        "SendMessageOptions",
        "InternalMessageChannel",
        "InternalMessageOptions",
        "InternalMessageResult",
    ] {
        assert!(schemas.contains_key(name), "OpenAPI omitted {name}");
    }
    assert!(
        schemas["MessageChannel"]["enum"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == "chant"))
    );
    let command_failures = schemas["CommandFailure"]["enum"].as_array().unwrap();
    for failure in [
        "insufficient_mana",
        "resist",
        "invalid_target",
        "not_allowed",
    ] {
        assert!(
            command_failures.iter().any(|value| value == failure),
            "OpenAPI omitted command failure {failure}"
        );
    }
    let message_parameters = openapi["paths"]["/clients/{client}/messages"]["get"]["parameters"]
        .as_array()
        .unwrap();
    let since = message_parameters
        .iter()
        .find(|parameter| parameter["name"] == "since")
        .expect("OpenAPI omitted the since parameter");
    assert_eq!(since["required"], false);
    assert_eq!(since["schema"]["format"], "date-time");
    let event_response = &openapi["paths"]["/clients/{client}/events"]["get"]["responses"]["200"];
    assert_eq!(
        event_response["content"]["text/event-stream"]["schema"]["$ref"],
        "#/components/schemas/ClientEvent"
    );
    let event_variants = schemas["ClientEvent"]["oneOf"].as_array().unwrap();
    for event_type in [
        "stream_ready",
        "client_logged_in",
        "client_disconnected",
        "sound_played",
        "music_started",
        "music_stopped",
        "effect_added",
        "effect_removed",
        "effect_changed",
        "skill_cooldown",
        "skill_ready",
        "spell_cooldown",
        "spell_ready",
        "message",
        "spell_succeeded",
        "spell_failed",
        "spell_received",
        "exchange_opened",
        "exchange_item_added",
        "exchange_gold_changed",
        "exchange_accepted",
        "exchange_completed",
        "exchange_cancelled",
    ] {
        assert!(event_variants.iter().any(|variant| {
            variant["properties"]["type"]["enum"]
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == event_type))
        }));
    }
    assert!(
        schemas["LaunchOptions"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "client_path")
    );
    assert!(schemas["LaunchOptions"]["properties"]["skip_exchange_alerts"].is_object());
    assert!(schemas["LaunchOptions"]["properties"]["show_items_with_alt"].is_object());
    assert!(schemas["LoadResult"]["properties"]["was_loaded"].is_object());
    assert!(schemas["LoadResult"]["properties"]["changed"].is_null());
    assert!(schemas["UnloadResult"]["properties"]["was_unloaded"].is_object());
    assert!(schemas["UnloadResult"]["properties"]["changed"].is_null());
    assert_eq!(
        openapi["paths"]["/clients/{client}/skills/use"]["post"]["requestBody"]["content"]["application/json"]
            ["example"],
        serde_json::json!({"name": "Assail"})
    );
    assert!(
        schemas["CharacterStatus"]["properties"]
            .get("gender_id")
            .is_none()
    );
    assert!(
        schemas["CharacterStatus"]["properties"]
            .get("class_id")
            .is_none()
    );
    for collection in [
        "inventory",
        "equipment",
        "spellbook",
        "skillbook",
        "effects",
    ] {
        assert!(
            schemas["CharacterStatus"]["properties"]
                .get(collection)
                .is_none(),
            "CharacterStatus still exposes {collection}"
        );
    }
    assert!(
        schemas["CharacterModifiers"]["properties"]
            .get("attack_element_id")
            .is_none()
    );
    assert!(
        schemas["CharacterModifiers"]["properties"]
            .get("defense_element_id")
            .is_none()
    );
    assert!(
        schemas["Spell"]["properties"]
            .get("target_type_id")
            .is_none()
    );
    assert!(
        schemas["ClientLifecycle"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "disconnected")
    );

    let docs = response("/docs/");
    assert_eq!(docs.status(), StatusCode::OK);
    assert!(text("/docs/").contains("/docs/ayu.css"));
    let asset = response("/docs/assets/swagger-ui-bundle.js");
    assert_eq!(asset.status(), StatusCode::OK);
    let theme = text("/docs/ayu.css");
    assert!(theme.contains("--ayu-bg: #0b0e14"));
    assert!(theme.contains("--ayu-orange: #ffb454"));
    assert!(theme.contains(".swagger-ui .info .title small pre.version"));
    assert!(theme.contains(".swagger-ui button.model-box-control"));
    assert!(theme.contains(".swagger-ui .json-schema-2020-12-accordion"));
    assert!(theme.contains(".swagger-ui .opblock-summary-control:focus"));
    assert!(theme.contains(".swagger-ui .opblock .opblock-section-header h4"));
}

#[test]
fn message_schema_exposes_nullable_sender_metadata() {
    let openapi = json("/openapi.json");
    let schemas = &openapi["components"]["schemas"];
    assert_eq!(
        schemas["MessageSenderType"]["enum"],
        serde_json::json!(["player", "monster", "mundane"])
    );
    let properties = &schemas["Message"]["properties"];
    assert!(properties["sender_id"].is_object());
    assert!(properties["sender_type"].is_object());
}
