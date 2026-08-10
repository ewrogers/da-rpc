use super::*;

#[test]
fn basic_messages_round_trip() {
    let mut disconnected = snapshot();
    disconnected.lifecycle = ClientLifecycle::Disconnected;
    let messages = [
        Message::Hello(hello()),
        Message::HelloAck(HelloAck {
            selected_version: PROTOCOL_VERSION_1_0,
            dll_instance_id: [0x5a; 16],
        }),
        Message::Ping(Ping { request_id: 1 }),
        Message::Pong(Pong { request_id: 2 }),
        Message::EchoRequest(EchoRequest {
            request_id: 3,
            text: "hello".into(),
        }),
        Message::EchoResponse(EchoResponse {
            request_id: 4,
            text: "world".into(),
        }),
        Message::TickHealthRequest(TickHealthRequest { request_id: 5 }),
        Message::TickHealthResponse(TickHealthResponse {
            request_id: 6,
            installed: true,
            relocated_bytes: 5,
            tick_count: u32::MAX,
        }),
        Message::SnapshotRequest(SnapshotRequest { request_id: 7 }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 8,
            result: SnapshotResult::Ready(Box::new(snapshot())),
        }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 9,
            result: SnapshotResult::Unavailable(SnapshotUnavailableReason::CaptureTimedOut),
        }),
        Message::SnapshotResponse(SnapshotResponse {
            request_id: 10,
            result: SnapshotResult::Ready(Box::new(disconnected)),
        }),
        Message::EventPollRequest(EventPollRequest {
            request_id: 11,
            after_sequence: 40,
            max_events: 64,
            wait_ms: 50,
        }),
    ];

    for message in messages {
        let frame = Frame::new(7, 123, message);
        let bytes = encode_frame(&frame).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), frame);
    }
}
