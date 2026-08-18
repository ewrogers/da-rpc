use super::*;

#[test]
fn snapshot_decoder_accepts_the_pre_dialog_protocol_1_0_tail() {
    let mut snapshot = snapshot();
    snapshot.dialog = None;
    snapshot.active_field_map = None;
    snapshot.group = None;
    snapshot.exchange = None;
    snapshot.legend = None;
    snapshot.planned_route = None;
    let frame = Frame::new(
        7,
        123,
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 1,
            result: SnapshotResult::Ready(Box::new(snapshot)),
        }),
    );
    let mut bytes = encode_frame(&frame).unwrap();
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    assert_eq!(bytes.pop(), Some(0));
    let payload_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) - 8;
    bytes[16..20].copy_from_slice(&payload_len.to_le_bytes());

    let decoded = decode_frame(&bytes).unwrap();
    let Message::SnapshotResponse(response) = decoded.message else {
        panic!("expected snapshot response");
    };
    let SnapshotResult::Ready(snapshot) = response.result else {
        panic!("expected ready snapshot");
    };
    assert_eq!(snapshot.dialog, None);
}

#[test]
fn snapshot_collections_are_strictly_validated() {
    let mut invalid_slot = snapshot();
    invalid_slot
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap()[0]
        .slot = 0;
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(invalid_slot)),
            }),
        )),
        Err(EncodeError::InvalidSnapshotSlot { slot: 0, max: 60 })
    );

    let mut duplicate_slot = snapshot();
    let inventory = duplicate_slot
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap();
    inventory.push(inventory[0].clone());
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(duplicate_slot)),
            }),
        )),
        Err(EncodeError::DuplicateSnapshotSlot { slot: 1 })
    );

    let mut oversized = snapshot();
    let item = oversized
        .character
        .as_ref()
        .unwrap()
        .inventory
        .as_ref()
        .unwrap()[0]
        .clone();
    oversized.character.as_mut().unwrap().inventory = Some(vec![item; 61]);
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(oversized)),
            }),
        )),
        Err(EncodeError::SnapshotCollectionTooLong {
            length: 61,
            max: 60,
        })
    );

    let mut duplicate_effect = snapshot();
    let effects = duplicate_effect
        .character
        .as_mut()
        .unwrap()
        .effects
        .as_mut()
        .unwrap();
    effects.push(effects[0]);
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(duplicate_effect)),
            }),
        )),
        Err(EncodeError::DuplicateEffectIcon { icon: 300 })
    );

    let mut long_name = snapshot();
    long_name
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap()[0]
        .name = Some("x".repeat(128));
    assert_eq!(
        encode_frame(&Frame::new(
            0,
            0,
            Message::SnapshotResponse(SnapshotResponse {
                request_id: 1,
                result: SnapshotResult::Ready(Box::new(long_name)),
            }),
        )),
        Err(EncodeError::SnapshotStringTooLong {
            length: 128,
            max: 127,
        })
    );
}

#[test]
fn malformed_snapshot_slots_are_rejected_when_decoding() {
    let frame = Frame::new(
        0,
        0,
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 1,
            result: SnapshotResult::Ready(Box::new(snapshot())),
        }),
    );
    let mut invalid = encode_frame(&frame).unwrap();
    let slot = invalid
        .windows(4)
        .position(|bytes| bytes == [1, 0x23, 0x01, 7])
        .expect("inventory marker is unique");
    invalid[slot] = 0;
    assert_eq!(
        decode_frame(&invalid),
        Err(DecodeError::InvalidSnapshotSlot { slot: 0, max: 60 })
    );

    let mut duplicate = snapshot();
    let inventory = duplicate
        .character
        .as_mut()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap();
    let mut second = inventory[0].clone();
    second.slot = 2;
    second.sprite = 0xdead;
    inventory.push(second);
    let mut duplicate = encode_frame(&Frame::new(
        0,
        0,
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 1,
            result: SnapshotResult::Ready(Box::new(duplicate)),
        }),
    ))
    .unwrap();
    let slot = duplicate
        .windows(3)
        .position(|bytes| bytes == [2, 0xad, 0xde])
        .expect("second inventory marker is unique");
    duplicate[slot] = 1;
    assert_eq!(
        decode_frame(&duplicate),
        Err(DecodeError::DuplicateSnapshotSlot { slot: 1 })
    );

    let mut invalid_duration = encode_frame(&frame).unwrap();
    let duration = invalid_duration
        .windows(3)
        .rposition(|bytes| bytes == [0x2c, 0x01, 6])
        .expect("effect marker is present")
        + 2;
    invalid_duration[duration] = 7;
    assert_eq!(
        decode_frame(&invalid_duration),
        Err(DecodeError::InvalidEffectDuration { actual: 7 })
    );
}
