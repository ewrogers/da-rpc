use super::*;

#[test]
fn message_types_have_distinct_public_event_names() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    for (sequence, kind, expected) in [
        (1, MessageKind::Say, "message.say"),
        (2, MessageKind::Shout, "message.shout"),
        (8, MessageKind::Chant, "message.chant"),
        (3, MessageKind::Whisper, "message.whisper"),
        (4, MessageKind::Guild, "message.guild"),
        (5, MessageKind::Group, "message.group"),
        (6, MessageKind::System, "message.system"),
        (7, MessageKind::World, "message.world"),
    ] {
        let events = expand(
            42,
            identity,
            StateEvent {
                sequence,
                revision: sequence,
                tick_ms: sequence,
                update: StateUpdate::Message(ClientMessage {
                    kind,
                    sender: None,
                    recipient: None,
                    text: "hello".into(),
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected);
    }
}

#[test]
fn lifecycle_transitions_have_semantic_public_event_names() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    for (sequence, previous, current, expected) in [
        (
            1,
            ClientLifecycle::Title,
            ClientLifecycle::InGame,
            "client.logged_in",
        ),
        (
            2,
            ClientLifecycle::InGame,
            ClientLifecycle::Disconnected,
            "client.disconnected",
        ),
    ] {
        let events = expand(
            42,
            identity,
            StateEvent {
                sequence,
                revision: sequence,
                tick_ms: sequence,
                update: StateUpdate::Lifecycle(LifecycleUpdate { previous, current }),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected);
        let json = serde_json::to_value(&events[0]).unwrap();
        assert!(!json["data"]["previous"].is_null());
        assert!(!json["data"]["current"].is_null());
    }
}

#[test]
fn audio_updates_have_distinct_public_event_names() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    for (sequence, update, expected) in [
        (1, AudioUpdate::SoundPlayed { effect: 12 }, "sound.played"),
        (2, AudioUpdate::MusicStarted { track: 4 }, "music.started"),
        (3, AudioUpdate::MusicStopped, "music.stopped"),
    ] {
        let events = expand(
            42,
            identity,
            StateEvent {
                sequence,
                revision: sequence,
                tick_ms: sequence,
                update: StateUpdate::Audio(update),
            },
            None,
            None,
            observed_at(),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name(), expected);
        let json = serde_json::to_value(&events[0]).unwrap();
        match update {
            AudioUpdate::SoundPlayed { effect } => assert_eq!(json["data"]["effect"], effect),
            AudioUpdate::MusicStarted { track } => assert_eq!(json["data"]["track"], track),
            AudioUpdate::MusicStopped => {
                assert!(json["data"].get("effect").is_none());
                assert!(json["data"].get("track").is_none());
            }
        }
    }
}

#[test]
fn empty_messages_do_not_become_public_events() {
    let identity = ClientIdentity {
        pid: 42,
        process_creation_time: 100,
        dll_instance_id: [1; 16],
    };
    for text in ["", "   "] {
        let events = expand(
            42,
            identity,
            StateEvent {
                sequence: 1,
                revision: 1,
                tick_ms: 1,
                update: StateUpdate::Message(ClientMessage {
                    kind: MessageKind::System,
                    sender: None,
                    recipient: None,
                    text: text.into(),
                }),
            },
            None,
            None,
            observed_at(),
        );
        assert!(events.is_empty());
    }
}

#[test]
fn spell_cast_retains_resolved_name_and_target_context() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 8,
            revision: 9,
            tick_ms: 500,
            update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                slot: 4,
                arguments: ModelSpellCastArguments::Target {
                    id: Some(77),
                    x: 10,
                    y: 12,
                },
            }),
        },
        Some("Ao Puinsein".into()),
        Some("Eidolon".into()),
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "spell.cast");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["name"], "Ao Puinsein");
    assert_eq!(event["data"]["arguments"]["type"], "target");
    assert_eq!(event["data"]["arguments"]["id"], 77);
    assert_eq!(event["data"]["arguments"]["name"], "Eidolon");
    assert_eq!(event["data"]["arguments"]["x"], 10);
    assert_eq!(event["data"]["arguments"]["y"], 12);
}

#[test]
fn interrupted_spell_reports_replacement_as_the_cancellation_source() {
    let events = expand(
        42,
        ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        },
        StateEvent {
            sequence: 8,
            revision: 9,
            tick_ms: 500,
            update: StateUpdate::Ability(AbilityUpdate::SpellCancelled {
                slot: 4,
                source: ModelSpellCancellationSource::Replaced,
            }),
        },
        Some("Inner Fire".into()),
        None,
        observed_at(),
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name(), "spell.cancelled");
    let event = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(event["data"]["name"], "Inner Fire");
    assert_eq!(event["data"]["source"], "replaced");
}
