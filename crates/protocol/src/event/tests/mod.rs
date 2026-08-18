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
