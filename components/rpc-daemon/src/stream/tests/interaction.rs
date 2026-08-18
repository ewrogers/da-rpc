use super::*;

fn field_map_state() -> ModelFieldMapState {
    ModelFieldMapState {
        revision: 4,
        field_name: "field001".into(),
        current_node_index: Some(0),
        destinations: vec![ModelFieldMapDestination {
            index: 0,
            screen_x: 100,
            screen_y: 80,
            name: "Mileth".into(),
            checksum: 0x1234,
            map_id: 100,
            map_x: 10,
            map_y: 20,
        }],
        selection: Some(ModelFieldMapSelection {
            destination_index: 0,
        }),
    }
}

#[test]
fn field_map_updates_use_stable_public_event_names_and_full_state() {
    let updates = [
        (
            FieldMapUpdate::Opened(field_map_state()),
            "field_map.opened",
        ),
        (
            FieldMapUpdate::Changed(field_map_state()),
            "field_map.changed",
        ),
        (
            FieldMapUpdate::SelectionSubmitted(field_map_state()),
            "field_map.selection_submitted",
        ),
        (
            FieldMapUpdate::Closed {
                previous: field_map_state(),
            },
            "field_map.closed",
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
                sequence: sequence as u32 + 1,
                revision: sequence as u32 + 1,
                tick_ms: sequence as u32,
                update: StateUpdate::FieldMap(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        let state = if expected_name == "field_map.closed" {
            &event["data"]["previous"]
        } else {
            &event["data"]["field_map"]
        };
        assert_eq!(state["field_name"], "field001");
        assert_eq!(state["selection"]["destination_index"], 0);
    }
}

#[test]
fn message_dialog_updates_use_stable_public_event_name_and_full_state() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 5,
            revision: 6,
            tick_ms: 7,
            update: StateUpdate::MessageDialogs(ModelMessageDialogsState {
                revision: 3,
                dialogs: vec![ModelMessageDialog {
                    id: 9,
                    text: Some("You sense danger nearby.".into()),
                    truncated: false,
                }],
            }),
        },
        None,
        None,
        observed_at(),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "message_dialogs.changed");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["state"]["revision"], 3);
    assert_eq!(event["data"]["state"]["dialogs"][0]["id"], 9);
    assert_eq!(
        event["data"]["state"]["dialogs"][0]["text"],
        "You sense danger nearby."
    );
}

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
