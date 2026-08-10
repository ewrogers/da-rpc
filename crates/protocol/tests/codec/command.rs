use super::*;

#[test]
fn command_limits_are_strictly_validated() {
    let invalid_timeout = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Submit {
            kind: CommandKind::Diagnostic,
            timeout_ms: MAX_COMMAND_TIMEOUT_MS + 1,
            wait_ms: 0,
        },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_timeout)),
        Err(EncodeError::InvalidCommandTimeout {
            actual: MAX_COMMAND_TIMEOUT_MS + 1,
            max: MAX_COMMAND_TIMEOUT_MS,
        })
    );

    let invalid_wait = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Query {
            command_id: 1,
            wait_ms: MAX_COMMAND_WAIT_MS + 1,
        },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_wait)),
        Err(EncodeError::InvalidCommandWait {
            actual: MAX_COMMAND_WAIT_MS + 1,
            max: MAX_COMMAND_WAIT_MS,
        })
    );

    let invalid_id = Message::CommandRequest(CommandRequest {
        request_id: 1,
        operation: CommandOperation::Cancel { command_id: 0 },
    });
    assert_eq!(
        encode_frame(&Frame::new(0, 0, invalid_id)),
        Err(EncodeError::InvalidCommandId)
    );
}
