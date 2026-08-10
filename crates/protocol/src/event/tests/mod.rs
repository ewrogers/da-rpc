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
            },
        ),
        Err(EncodeError::InvalidMovementOutcome)
    );
}
