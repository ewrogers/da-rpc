use darpc_protocol::{
    Architecture, ComponentVersion, EndpointRole, Handshake, HandshakePhase, Hello, Message,
    MessageDirection, PROTOCOL_VERSION_1_0, PROTOCOL_VERSION_1_1, PROTOCOL_VERSION_1_6, Ping,
    SequenceCounter, SequenceError, SessionError, VersionRange, elapsed_tick_ms, negotiate_version,
};

fn hello() -> Hello {
    Hello {
        protocol_versions: VersionRange {
            min: PROTOCOL_VERSION_1_6,
            max: PROTOCOL_VERSION_1_6,
        },
        dll_instance_id: [0x5a; 16],
        process_id: 42,
        process_creation_time: 123,
        architecture: Architecture::X86,
        dll_version: ComponentVersion {
            major: 0,
            minor: 1,
            patch: 0,
        },
        executable_fingerprint: [0xa5; 32],
        client_version: 741,
    }
}

#[test]
fn dll_and_controller_complete_the_same_handshake() {
    let hello = Message::Hello(hello());
    let mut dll = Handshake::new(EndpointRole::Dll);
    let mut controller = Handshake::new(EndpointRole::Controller);

    dll.observe(MessageDirection::Outbound, &hello).unwrap();
    controller
        .observe(MessageDirection::Inbound, &hello)
        .unwrap();
    let acknowledgement = Message::HelloAck(controller.pending_acknowledgement().unwrap());
    controller
        .observe(MessageDirection::Outbound, &acknowledgement)
        .unwrap();
    dll.observe(MessageDirection::Inbound, &acknowledgement)
        .unwrap();

    assert!(dll.is_ready());
    assert!(controller.is_ready());
    assert_eq!(dll.selected_version(), Some(PROTOCOL_VERSION_1_6));
    assert_eq!(dll.dll_instance_id(), Some([0x5a; 16]));

    let ping = Message::Ping(Ping { request_id: 7 });
    dll.observe(MessageDirection::Outbound, &ping).unwrap();
    controller
        .observe(MessageDirection::Inbound, &ping)
        .unwrap();
}

#[test]
fn application_messages_are_rejected_before_the_handshake() {
    let ping = Message::Ping(Ping { request_id: 7 });
    let mut dll = Handshake::new(EndpointRole::Dll);

    assert!(matches!(
        dll.observe(MessageDirection::Outbound, &ping),
        Err(SessionError::UnexpectedMessage {
            phase: HandshakePhase::SendHello,
            ..
        })
    ));
}

#[test]
fn handshake_direction_and_order_are_strict() {
    let hello = Message::Hello(hello());
    let mut controller = Handshake::new(EndpointRole::Controller);

    assert!(matches!(
        controller.observe(MessageDirection::Outbound, &hello),
        Err(SessionError::UnexpectedMessage {
            phase: HandshakePhase::ReceiveHello,
            ..
        })
    ));

    controller
        .observe(MessageDirection::Inbound, &hello)
        .unwrap();
    let acknowledgement = Message::HelloAck(controller.pending_acknowledgement().unwrap());
    controller
        .observe(MessageDirection::Outbound, &acknowledgement)
        .unwrap();
    assert!(matches!(
        controller.observe(MessageDirection::Inbound, &hello),
        Err(SessionError::UnexpectedMessage {
            phase: HandshakePhase::Ready,
            ..
        })
    ));
}

#[test]
fn invalid_and_unsupported_versions_are_distinct() {
    assert_eq!(
        negotiate_version(VersionRange { min: 0, max: 1 }),
        Err(SessionError::InvalidVersionRange { min: 0, max: 1 })
    );
    assert_eq!(
        negotiate_version(VersionRange {
            min: 0x0107,
            max: 0x0108,
        }),
        Err(SessionError::UnsupportedVersionRange {
            min: 0x0107,
            max: 0x0108,
        })
    );

    assert_eq!(
        negotiate_version(VersionRange {
            min: PROTOCOL_VERSION_1_1,
            max: PROTOCOL_VERSION_1_1,
        }),
        Err(SessionError::UnsupportedVersionRange {
            min: PROTOCOL_VERSION_1_1,
            max: PROTOCOL_VERSION_1_1,
        })
    );
}

#[test]
fn dll_rejects_an_acknowledgement_for_the_wrong_offer() {
    let hello = Message::Hello(hello());
    let mut dll = Handshake::new(EndpointRole::Dll);
    dll.observe(MessageDirection::Outbound, &hello).unwrap();

    let wrong_version = Message::HelloAck(darpc_protocol::HelloAck {
        selected_version: PROTOCOL_VERSION_1_0,
        dll_instance_id: [0x5a; 16],
    });
    assert_eq!(
        dll.observe(MessageDirection::Inbound, &wrong_version),
        Err(SessionError::InvalidSelectedVersion {
            selected: PROTOCOL_VERSION_1_0
        })
    );

    let wrong_instance = Message::HelloAck(darpc_protocol::HelloAck {
        selected_version: PROTOCOL_VERSION_1_6,
        dll_instance_id: [0x6b; 16],
    });
    assert_eq!(
        dll.observe(MessageDirection::Inbound, &wrong_instance),
        Err(SessionError::InstanceMismatch)
    );
}

#[test]
fn controller_must_send_the_exact_acknowledgement() {
    let hello = Message::Hello(hello());
    let mut controller = Handshake::new(EndpointRole::Controller);
    controller
        .observe(MessageDirection::Inbound, &hello)
        .unwrap();

    let wrong = Message::HelloAck(darpc_protocol::HelloAck {
        selected_version: PROTOCOL_VERSION_1_6,
        dll_instance_id: [0x6b; 16],
    });
    assert!(matches!(
        controller.observe(MessageDirection::Outbound, &wrong),
        Err(SessionError::UnexpectedMessage {
            phase: HandshakePhase::SendHelloAck,
            ..
        })
    ));
}

#[test]
fn sequence_counters_wrap_and_do_not_advance_on_mismatch() {
    let mut outgoing = SequenceCounter::from_next(u16::MAX);
    assert_eq!(outgoing.take(), u16::MAX);
    assert_eq!(outgoing.take(), 0);

    let mut incoming = SequenceCounter::from_next(u16::MAX);
    assert_eq!(
        incoming.observe(7),
        Err(SequenceError {
            expected: u16::MAX,
            actual: 7,
        })
    );
    assert_eq!(incoming.expected(), u16::MAX);
    incoming.observe(u16::MAX).unwrap();
    incoming.observe(0).unwrap();
    assert_eq!(incoming.expected(), 1);
}

#[test]
fn sender_tick_elapsed_time_uses_wrapping_subtraction() {
    assert_eq!(elapsed_tick_ms(0xffff_fffa, 3), 9);
}
