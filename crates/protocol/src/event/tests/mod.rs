use super::*;

#[test]
fn empty_status_update_is_detected() {
    assert!(StatusUpdate::default().is_empty());
}

#[test]
fn movement_outcome_requires_a_known_destination() {
    let mut output = Vec::new();
    assert_eq!(
        encode_movement(
            &mut output,
            MovementUpdate::Stopped {
                current: TilePosition { x: 2, y: 8 },
                destination: None,
                reached_destination: Some(false),
                reason: MovementStopReason::Cancelled,
            },
        ),
        Err(EncodeError::InvalidMovementOutcome)
    );
}

#[test]
fn movement_stop_reason_codes_are_stable() {
    for (reason, code) in [
        (MovementStopReason::Completed, 1),
        (MovementStopReason::Obstructed, 2),
        (MovementStopReason::Replaced, 3),
        (MovementStopReason::Cancelled, 4),
        (MovementStopReason::PositionCorrected, 5),
    ] {
        let mut output = Vec::new();
        encode_movement(
            &mut output,
            MovementUpdate::Stopped {
                current: TilePosition { x: 2, y: 8 },
                destination: Some(TilePosition { x: 2, y: 8 }),
                reached_destination: Some(true),
                reason,
            },
        )
        .unwrap();
        assert_eq!(output.last(), Some(&code));
    }
}

#[test]
fn map_download_update_codes_are_stable() {
    for (update, code) in [
        (
            MapDownloadUpdate::Requested(MapDownload {
                map_id: 3001,
                width: 100,
                height: 80,
            }),
            1,
        ),
        (
            MapDownloadUpdate::Downloaded(MapDownload {
                map_id: 3001,
                width: 100,
                height: 80,
            }),
            2,
        ),
    ] {
        let mut output = Vec::new();
        encode_event(
            &mut output,
            &StateEvent {
                sequence: 1,
                revision: 2,
                tick_ms: 3,
                update: StateUpdate::MapDownload(update),
            },
        )
        .unwrap();
        assert_eq!(output[12], 26);
        assert_eq!(output[13], code);
        assert_eq!(&output[14..], &[0xB9, 0x0B, 0, 0, 100, 80]);
    }
}
