use super::{render_human, render_json};
use darpc_model::{
    ClientLifecycle, ClientSnapshot, DialogChoice, DialogInteraction, DialogKind, DialogNavigation,
    DialogSpeaker, DialogSpriteType, DialogState, DialogTarget, ExchangeItem, ExchangeOffer,
    ExchangeState,
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
    }
}

#[test]
fn snapshot_output_keeps_dialog_without_character_state() {
    let snapshot = snapshot();
    let human = render_human(42, 1, 2, &snapshot);
    assert!(human.contains("event_sequence=12"));
    assert!(human.contains("character: unavailable"));
    assert!(human.contains("dialog: revision=7"));
    assert!(human.contains("exchange: id=9 partner=ZiLo"));
    assert!(human.contains("exchange_item: party=local index=0 name=Red Potion quantity=2"));
    assert!(human.contains("0\t\"Ask\""));

    let json: serde_json::Value = serde_json::from_str(&render_json(42, 1, 2, &snapshot)).unwrap();
    assert_eq!(json["snapshot"]["event_sequence"], 12);
    assert_eq!(json["snapshot"]["updated_tick_ms"], 125);
    assert_eq!(json["snapshot"]["dialog"]["revision"], 7);
    assert_eq!(json["snapshot"]["exchange"]["partner"], "ZiLo");
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
