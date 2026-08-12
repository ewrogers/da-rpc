use super::*;

#[test]
fn exchange_updates_use_stable_public_event_names_and_context() {
    let item = ModelExchangeItem {
        index: 0,
        sprite: 123,
        dye_color: 4,
        quantity: Some(2),
        name: "Red Potion".into(),
    };
    let updates = [
        (ExchangeUpdate::Opened(exchange_state()), "exchange.opened"),
        (
            ExchangeUpdate::ItemAdded {
                state: exchange_state(),
                party: ModelExchangeParty::Other,
                item,
            },
            "exchange.item_added",
        ),
        (
            ExchangeUpdate::GoldChanged {
                state: exchange_state(),
                party: ModelExchangeParty::Local,
                gold: 100,
            },
            "exchange.gold_changed",
        ),
        (
            ExchangeUpdate::Accepted {
                state: exchange_state(),
                party: ModelExchangeParty::Other,
                message: "accepted".into(),
            },
            "exchange.accepted",
        ),
        (
            ExchangeUpdate::Completed {
                state: exchange_state(),
                message: "complete".into(),
            },
            "exchange.completed",
        ),
        (
            ExchangeUpdate::Cancelled {
                state: exchange_state(),
                message: "cancelled".into(),
            },
            "exchange.cancelled",
        ),
    ];

    for (sequence, (update, expected_name)) in updates.into_iter().enumerate() {
        let events = expand(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            StateEvent {
                sequence: sequence as u32,
                revision: sequence as u32,
                tick_ms: sequence as u32,
                update: StateUpdate::Exchange(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["data"]["exchange"]["id"], 77);
        assert_eq!(event["data"]["exchange"]["partner"], "ZiLo");
    }
}
