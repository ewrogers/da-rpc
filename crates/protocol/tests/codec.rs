use darpc_protocol::{
    Architecture, ComponentVersion, DecodeError, EchoRequest, EchoResponse, EncodeError,
    FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_VERSION, Frame, FrameHeader, Hello, HelloAck,
    MAX_ECHO_TEXT_LEN, MAX_PAYLOAD_LEN, Message, MessageType, PROTOCOL_VERSION_1_0, Ping, Pong,
    VersionRange, decode_frame, decode_header, encode_frame, protocol_version,
    protocol_version_major, protocol_version_minor,
};

fn hello() -> Hello {
    Hello {
        protocol_versions: VersionRange {
            min: PROTOCOL_VERSION_1_0,
            max: PROTOCOL_VERSION_1_0,
        },
        dll_instance_id: [0x5a; 16],
        process_id: 42,
        process_creation_time: 0x1122_3344_5566_7788,
        architecture: Architecture::X86,
        dll_version: ComponentVersion {
            major: 1,
            minor: 2,
            patch: 3,
        },
        executable_fingerprint: [0xa5; 32],
        layout_id: 741,
    }
}

fn frame_for(message_type: MessageType, payload: &[u8]) -> Vec<u8> {
    let payload_len = u32::try_from(payload.len()).unwrap();
    let mut bytes = header_with_payload_len(message_type.wire_value(), payload_len);
    bytes.extend_from_slice(payload);
    bytes
}

fn header_with_payload_len(message_type: u16, payload_len: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_HEADER_LEN);
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.extend_from_slice(&FRAME_VERSION.to_le_bytes());
    bytes.extend_from_slice(&message_type.to_le_bytes());
    bytes.extend_from_slice(&7_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&123_u32.to_le_bytes());
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes
}

#[test]
fn every_message_round_trips() {
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
    ];

    for message in messages {
        let frame = Frame::new(7, 123, message);
        let bytes = encode_frame(&frame).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), frame);
    }
}

#[test]
fn protocol_version_packs_major_and_minor_bytes() {
    let version = protocol_version(1, 2);

    assert_eq!(version, 0x0102);
    assert_eq!(protocol_version_major(version), 1);
    assert_eq!(protocol_version_minor(version), 2);
}

#[test]
fn header_can_be_validated_before_reading_the_payload() {
    let bytes = encode_frame(&Frame::new(7, 123, Message::Ping(Ping { request_id: 9 }))).unwrap();
    let header = decode_header(&bytes[..FRAME_HEADER_LEN]).unwrap();

    assert_eq!(header.message_type, MessageType::Ping);
    assert_eq!(header.sequence, 7);
    assert_eq!(header.sender_tick_ms, 123);
    assert_eq!(header.payload_len, 4);
    assert_eq!(header.frame_len().unwrap(), FRAME_HEADER_LEN + 4);
}

#[test]
fn every_truncated_prefix_is_rejected() {
    let bytes = encode_frame(&Frame::new(7, 123, Message::Hello(hello()))).unwrap();

    for length in 0..bytes.len() {
        assert!(
            decode_frame(&bytes[..length]).is_err(),
            "prefix {length} decoded"
        );
    }
}

#[test]
fn oversized_payload_length_is_rejected_from_the_header() {
    let payload_len = u32::try_from(MAX_PAYLOAD_LEN + 1).unwrap();
    let bytes = header_with_payload_len(MessageType::Ping.wire_value(), payload_len);

    assert_eq!(
        decode_header(&bytes),
        Err(DecodeError::PayloadTooLarge {
            length: MAX_PAYLOAD_LEN + 1,
            max: MAX_PAYLOAD_LEN,
        })
    );
}

#[test]
fn hostile_u32_payload_length_is_rejected_without_allocation() {
    let bytes = header_with_payload_len(MessageType::Ping.wire_value(), u32::MAX);

    assert!(matches!(
        decode_header(&bytes),
        Err(DecodeError::PayloadTooLarge { .. })
    ));
}

#[test]
fn frame_length_arithmetic_is_checked() {
    let header = FrameHeader {
        message_type: MessageType::Ping,
        sequence: 0,
        sender_tick_ms: 0,
        payload_len: usize::MAX,
    };

    assert_eq!(header.frame_len(), Err(DecodeError::LengthOverflow));
}

#[test]
fn trailing_frame_bytes_are_rejected() {
    let mut bytes = encode_frame(&Frame::new(0, 0, Message::Ping(Ping { request_id: 1 }))).unwrap();
    bytes.push(0);

    assert!(matches!(
        decode_frame(&bytes),
        Err(DecodeError::TrailingFrameBytes { .. })
    ));
}

#[test]
fn malformed_headers_are_rejected() {
    let valid = header_with_payload_len(MessageType::Ping.wire_value(), 0);

    let mut invalid_magic = valid.clone();
    invalid_magic[0] = b'X';
    assert!(matches!(
        decode_header(&invalid_magic),
        Err(DecodeError::InvalidMagic { .. })
    ));

    let mut invalid_version = valid.clone();
    invalid_version[4..6].copy_from_slice(&2_u16.to_le_bytes());
    assert_eq!(
        decode_header(&invalid_version),
        Err(DecodeError::UnsupportedFrameVersion { actual: 2 })
    );

    let mut unknown_message = valid.clone();
    unknown_message[6..8].copy_from_slice(&99_u16.to_le_bytes());
    assert_eq!(
        decode_header(&unknown_message),
        Err(DecodeError::UnknownMessageType { actual: 99 })
    );

    let mut nonzero_flags = valid;
    nonzero_flags[10..12].copy_from_slice(&1_u16.to_le_bytes());
    assert_eq!(
        decode_header(&nonzero_flags),
        Err(DecodeError::NonZeroFlags { actual: 1 })
    );
}

#[test]
fn fixed_payload_sizes_are_exact() {
    let short = frame_for(MessageType::Ping, &[0; 3]);
    assert!(matches!(
        decode_frame(&short),
        Err(DecodeError::TruncatedMessage { .. })
    ));

    let long = frame_for(MessageType::Ping, &[0; 5]);
    assert!(matches!(
        decode_frame(&long),
        Err(DecodeError::TrailingMessageBytes { .. })
    ));
}

#[test]
fn invalid_hello_fields_are_rejected() {
    let mut invalid_range = encode_frame(&Frame::new(0, 0, Message::Hello(hello()))).unwrap();
    invalid_range[20..24].copy_from_slice(&[0x01, 0x01, 0x00, 0x01]);
    assert_eq!(
        decode_frame(&invalid_range),
        Err(DecodeError::InvalidVersionRange {
            min: 0x0101,
            max: 0x0100,
        })
    );

    let mut invalid_architecture =
        encode_frame(&Frame::new(0, 0, Message::Hello(hello()))).unwrap();
    invalid_architecture[52] = 99;
    assert_eq!(
        decode_frame(&invalid_architecture),
        Err(DecodeError::InvalidArchitecture { actual: 99 })
    );
}

#[test]
fn invalid_hello_range_is_rejected_when_encoding() {
    let mut message = hello();
    message.protocol_versions = VersionRange {
        min: 0,
        max: PROTOCOL_VERSION_1_0,
    };

    assert_eq!(
        encode_frame(&Frame::new(0, 0, Message::Hello(message))),
        Err(EncodeError::InvalidVersionRange {
            min: 0,
            max: PROTOCOL_VERSION_1_0,
        })
    );
}

#[test]
fn echo_requires_bounded_utf8() {
    let invalid_utf8 = frame_for(MessageType::EchoRequest, &[1, 0, 0, 0, 1, 0, 0xff]);
    assert_eq!(decode_frame(&invalid_utf8), Err(DecodeError::InvalidUtf8));

    let too_long = "a".repeat(MAX_ECHO_TEXT_LEN + 1);
    let message = Message::EchoRequest(EchoRequest {
        request_id: 1,
        text: too_long,
    });
    assert!(matches!(
        encode_frame(&Frame::new(0, 0, message)),
        Err(EncodeError::EchoTooLong { .. })
    ));

    let mut payload = vec![0; 6 + MAX_ECHO_TEXT_LEN + 1];
    payload[4..6].copy_from_slice(&u16::try_from(MAX_ECHO_TEXT_LEN + 1).unwrap().to_le_bytes());
    let oversized = frame_for(MessageType::EchoResponse, &payload);
    assert!(matches!(
        decode_frame(&oversized),
        Err(DecodeError::EchoTooLong { .. })
    ));
}
