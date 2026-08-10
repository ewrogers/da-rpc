use super::*;

#[test]
fn event_messages_require_bounded_utf8_fields() {
    let message = Message::EventPollResponse(EventPollResponse {
        request_id: 1,
        result: EventPollResult::Events(vec![StateEvent {
            sequence: 1,
            revision: 1,
            tick_ms: 1,
            update: StateUpdate::Message(ClientMessage {
                kind: MessageKind::System,
                sender: None,
                recipient: None,
                text: "x".repeat(4 * 1024 + 1),
            }),
        }]),
    });

    assert!(matches!(
        encode_frame(&Frame::new(0, 0, message)),
        Err(EncodeError::EventStringTooLong { .. })
    ));
}

#[test]
fn collection_events_require_valid_batches_and_content() {
    let invalid_batch = Message::EventPollResponse(EventPollResponse {
        request_id: 1,
        result: EventPollResult::Events(vec![StateEvent {
            sequence: 1,
            revision: 1,
            tick_ms: 1,
            update: StateUpdate::Inventory(SlotUpdate {
                batch_index: 0,
                batch_count: 0,
                change: CollectionChange::Changed,
                slot: 1,
                before: None,
                after: None,
            }),
        }]),
    });
    assert!(matches!(
        encode_frame(&Frame::new(0, 0, invalid_batch)),
        Err(EncodeError::InvalidCollectionBatch { index: 0, count: 0 })
    ));

    let empty_change = Message::EventPollResponse(EventPollResponse {
        request_id: 1,
        result: EventPollResult::Events(vec![StateEvent {
            sequence: 1,
            revision: 1,
            tick_ms: 1,
            update: StateUpdate::Inventory(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Changed,
                slot: 1,
                before: None,
                after: None,
            }),
        }]),
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, empty_change)),
        Err(EncodeError::EmptyCollectionUpdate)
    );

    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&1_u16.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&[6, 0, 0]);
    let malformed = frame_for(MessageType::EventPollResponse, &payload);
    assert_eq!(
        decode_frame(&malformed),
        Err(DecodeError::InvalidCollectionBatch { index: 0, count: 0 })
    );
}
