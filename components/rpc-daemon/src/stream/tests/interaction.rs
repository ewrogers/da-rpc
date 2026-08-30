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

fn bulletin_state() -> ModelBulletinState {
    ModelBulletinState {
        revision: 8,
        pending: Some(ModelBulletinOperation::OpenEntry),
        last_operation_result: None,
        can_go_back: true,
        can_go_forward: false,
        view: ModelBulletinView::Sections {
            heading: "Boards".into(),
            sections: vec![ModelBulletinSection {
                id: 4,
                name: "Mileth News".into(),
                kind: ModelBulletinSectionKind::Board,
                source: ModelBulletinSource::Global,
            }],
            selected_section_id: Some(4),
            viewport: ModelBulletinViewport {
                position: 1,
                maximum: 3,
            },
            truncated: false,
        },
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
fn bulletin_updates_use_stable_public_event_names_and_full_state() {
    let updates = [
        (BulletinUpdate::Opened(bulletin_state()), "bulletin.opened"),
        (
            BulletinUpdate::Changed(bulletin_state()),
            "bulletin.changed",
        ),
        (
            BulletinUpdate::ActionSubmitted {
                state: Some(bulletin_state()),
                operation: ModelBulletinOperation::OpenEntry,
            },
            "bulletin.changed",
        ),
        (
            BulletinUpdate::Closed {
                previous: bulletin_state(),
            },
            "bulletin.closed",
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
                update: StateUpdate::Bulletin(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        let state = if expected_name == "bulletin.closed" {
            &event["data"]["previous"]
        } else {
            &event["data"]["bulletin"]
        };
        assert_eq!(state["revision"], 8);
        assert_eq!(state["view"]["sections"][0]["name"], "Mileth News");
    }
}

#[test]
fn bulletin_mutation_events_name_the_confirmed_outcome_and_action() {
    let updates = [
        (
            BulletinUpdate::OperationResult {
                state: bulletin_state(),
                result: ModelBulletinOperationResult {
                    operation: ModelBulletinOperation::PostArticle,
                    raw_status: 1,
                    message: Some("Your letter was sent.".into()),
                },
            },
            "bulletin.submitted",
            "bulletin_submitted",
            "post_article",
        ),
        (
            BulletinUpdate::OperationResult {
                state: bulletin_state(),
                result: ModelBulletinOperationResult {
                    operation: ModelBulletinOperation::DeleteEntry,
                    raw_status: 1,
                    message: Some("The message was destroyed.".into()),
                },
            },
            "bulletin.deleted",
            "bulletin_deleted",
            "delete_entry",
        ),
        (
            BulletinUpdate::OperationResult {
                state: bulletin_state(),
                result: ModelBulletinOperationResult {
                    operation: ModelBulletinOperation::SendMail,
                    raw_status: 1,
                    message: Some("There is no recipient for this message.".into()),
                },
            },
            "bulletin.failed",
            "bulletin_failed",
            "send_mail",
        ),
    ];

    for (sequence, (update, expected_name, expected_type, expected_action)) in
        updates.into_iter().enumerate()
    {
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
                update: StateUpdate::Bulletin(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected_name);
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["type"], expected_type);
        assert_eq!(event["data"]["action"], expected_action);
        assert_eq!(event["data"]["raw_status"], 1);
        assert_eq!(event["data"]["bulletin"]["revision"], 8);
        if expected_name == "bulletin.failed" {
            assert_eq!(
                event["data"]["message"],
                "There is no recipient for this message."
            );
        }
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
