use super::{render_human, render_json};
use darpc_model::{
    BulletinEntrySummary, BulletinPagination, BulletinSection, BulletinSectionKind, BulletinSource,
    BulletinState, BulletinView, BulletinViewport, ClientLifecycle, ClientSnapshot, DialogChoice,
    DialogInteraction, DialogKind, DialogNavigation, DialogSpeaker, DialogSpriteType, DialogState,
    DialogTarget, ExchangeItem, ExchangeOffer, ExchangeState, PlannedRoute, TilePosition,
};

fn snapshot() -> ClientSnapshot {
    ClientSnapshot {
        revision: 3,
        event_sequence: 12,
        captured_tick_ms: 100,
        updated_tick_ms: 125,
        capture_duration_us: 40,
        world_generation: 2,
        lifecycle: ClientLifecycle::InGame,
        character: None,
        objects: None,
        dialog: Some(DialogState {
            revision: 7,
            kind: DialogKind::Pursuit,
            target: DialogTarget { id: 77 },
            speaker: DialogSpeaker {
                name: Some("Innkeeper".into()),
                sprite: 12,
                sprite_type: DialogSpriteType::Creature,
                color: 3,
                show_graphic: true,
            },
            content: Some("Welcome".into()),
            response_pending: false,
            navigation: DialogNavigation {
                previous: false,
                next: true,
                close: true,
            },
            interaction: DialogInteraction::Choices(vec![DialogChoice {
                index: 0,
                text: "Ask".into(),
            }]),
        }),
        active_field_map: None,
        message_dialogs: darpc_model::MessageDialogsState {
            revision: 8,
            dialogs: vec![darpc_model::MessageDialog {
                id: 3,
                text: Some("You sense danger nearby.".into()),
                truncated: false,
            }],
        },
        active_bulletin: Some(BulletinState {
            revision: 4,
            pending: None,
            last_operation_result: None,
            can_go_back: true,
            can_go_forward: false,
            view: BulletinView::Entries {
                section: BulletinSection {
                    id: 2,
                    name: "Mileth News".into(),
                    kind: BulletinSectionKind::Board,
                    source: BulletinSource::Global,
                },
                entries: vec![BulletinEntrySummary {
                    id: 12,
                    flags: 1,
                    author: "Town Crier".into(),
                    month: 8,
                    day: 29,
                    subject: "Festival".into(),
                }],
                selected_entry_id: Some(12),
                viewport: BulletinViewport {
                    position: 2,
                    maximum: 9,
                },
                pagination: BulletinPagination::Ready,
                truncated: false,
            },
        }),
        group: None,
        exchange: Some(ExchangeState {
            id: 9,
            partner: "ZiLo".into(),
            local: ExchangeOffer {
                items: vec![ExchangeItem {
                    index: 0,
                    sprite: 44,
                    dye_color: 0,
                    quantity: Some(2),
                    name: "Red Potion".into(),
                }],
                gold: 100,
                accepted: false,
            },
            other: ExchangeOffer::default(),
        }),
        legend: None,
        planned_route: Some(PlannedRoute {
            source: darpc_model::ActionSource::Client,
            generation: 8,
            tiles: vec![TilePosition { x: 2, y: 3 }, TilePosition { x: 3, y: 3 }],
        }),
    }
}

#[test]
fn snapshot_output_keeps_dialog_without_character_state() {
    let snapshot = snapshot();
    let human = render_human(42, 1, 2, &snapshot);
    assert!(human.contains("event_sequence=12"));
    assert!(human.contains("character: unavailable"));
    assert!(human.contains("dialog: revision=7"));
    assert!(human.contains("message_dialogs: revision=8 dialogs=1"));
    assert!(human.contains("message_dialog: id=3 text=\"You sense danger nearby.\""));
    assert!(human.contains("bulletin: revision=4 pending=none"));
    assert!(human.contains("bulletin_entry: id=12 flags=1 author=\"Town Crier\""));
    assert!(human.contains("exchange: id=9 partner=ZiLo"));
    assert!(human.contains("planned_route: source=client generation=8 tiles=2"));
    assert!(human.contains("planned_route_tile: index=1 x=3 y=3"));
    assert!(human.contains("exchange_item: party=local index=0 name=Red Potion quantity=2"));
    assert!(human.contains("0\t\"Ask\""));

    let json: serde_json::Value = serde_json::from_str(&render_json(42, 1, 2, &snapshot)).unwrap();
    assert_eq!(json["snapshot"]["event_sequence"], 12);
    assert_eq!(json["snapshot"]["updated_tick_ms"], 125);
    assert_eq!(json["snapshot"]["dialog"]["revision"], 7);
    assert_eq!(json["snapshot"]["message_dialogs"]["revision"], 8);
    assert_eq!(json["snapshot"]["active_bulletin"]["revision"], 4);
    assert_eq!(
        json["snapshot"]["active_bulletin"]["view"]["entries"][0]["subject"],
        "Festival"
    );
    assert_eq!(
        json["snapshot"]["message_dialogs"]["dialogs"][0]["text"],
        "You sense danger nearby."
    );
    assert_eq!(json["snapshot"]["exchange"]["partner"], "ZiLo");
    assert_eq!(json["snapshot"]["planned_route"]["generation"], 8);
    assert_eq!(
        json["snapshot"]["planned_route"]["source"]["kind"],
        "client"
    );
    assert_eq!(json["snapshot"]["planned_route"]["tiles"][1]["x"], 3);
    assert_eq!(
        json["snapshot"]["exchange"]["local"]["items"][0]["quantity"],
        2
    );
    assert_eq!(
        json["snapshot"]["dialog"]["interaction"],
        serde_json::json!({
            "type": "choices",
            "data": [{ "index": 0, "text": "Ask" }],
        })
    );
}
