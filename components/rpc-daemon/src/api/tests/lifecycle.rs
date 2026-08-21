use super::*;

#[test]
fn delegates_typed_lifecycle_operations() {
    let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(state, "/clients/42/load", "");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let result = response_json(response);
    assert_eq!(result["was_loaded"], true);
    assert!(result.get("changed").is_none());
    assert!(matches!(
        receiver.recv().unwrap(),
        DaemonEvent::Status(ConnectionEvent::Initializing { pid: 42 })
    ));
    assert!(matches!(
        receiver.recv().unwrap(),
        DaemonEvent::Status(ConnectionEvent::Connecting { pid: 42 })
    ));

    let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(
        state,
        "/clients/launch",
        r#"{"client_path":"C:\\Darkages.exe","allow_multiple":true,"server":"127.0.0.1"}"#,
    );
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(matches!(receiver.recv().unwrap(), DaemonEvent::Track(77)));
}

#[test]
fn reports_no_transition_when_the_dll_is_already_in_the_requested_state() {
    let result = response_json(post_json(state(), "/clients/42/load", ""));
    assert_eq!(result["was_loaded"], false);
    assert!(result.get("changed").is_none());

    let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let result = response_json(post_json(state, "/clients/42/unload", ""));
    assert_eq!(result["was_unloaded"], false);
    assert!(result.get("changed").is_none());
}

#[test]
fn rejects_arbitrary_launch_arguments() {
    let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
    let response = post_json(
        state,
        "/clients/launch",
        r#"{"client_path":"C:\\Darkages.exe","arguments":["unsafe"]}"#,
    );
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn accepts_a_full_client_path_and_defaults_the_server_port() {
    let request: LaunchOptions = serde_json::from_str(
        r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"da0.kru.com","show_items_with_alt":true,"skip_exchange_alerts":true}"#,
    )
    .unwrap();
    let options = ManagedLaunchOptions::try_from(request).unwrap();
    assert_eq!(
        options.client_path,
        std::path::PathBuf::from(r"D:\Games\Dark Ages\Darkages.exe")
    );
    assert!(options.skip_exchange_alerts);
    assert!(options.show_items_with_alt);
    let server = options.server.unwrap();
    assert_eq!(server.host, "da0.kru.com");
    assert_eq!(server.port, 2610);

    let request: LaunchOptions = serde_json::from_str(
        r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"127.0.0.1:3000"}"#,
    )
    .unwrap();
    let server = ManagedLaunchOptions::try_from(request)
        .unwrap()
        .server
        .unwrap();
    assert_eq!(server.host, "127.0.0.1");
    assert_eq!(server.port, 3000);

    assert!(serde_json::from_str::<LaunchOptions>("{}").is_err());
    assert!(
        serde_json::from_str::<LaunchOptions>(
            r#"{"client_path":"C:\\Darkages.exe","server":{"host":"da0.kru.com"}}"#
        )
        .is_err()
    );
    for field in ["loader_path", "dll_path"] {
        let body = format!(r#"{{"{field}":"unsafe"}}"#);
        assert!(serde_json::from_str::<LaunchOptions>(&body).is_err());
    }
}

#[test]
fn rejects_relative_client_paths() {
    let request: LaunchOptions = serde_json::from_str(r#"{"client_path":"Darkages.exe"}"#).unwrap();
    let error = ManagedLaunchOptions::try_from(request).unwrap_err();
    assert_eq!(error.body.error.code, "invalid_client_path");
}

#[test]
fn rejects_invalid_server_strings() {
    for server in ["", ":2610", "host:", "host:0", "host:nope", "::1"] {
        let request = LaunchOptions {
            client_path: r"C:\Darkages.exe".into(),
            allow_multiple: false,
            show_items_with_alt: false,
            skip_exchange_alerts: false,
            skip_intro: false,
            skip_notice: false,
            server: Some(server.into()),
        };
        let error = ManagedLaunchOptions::try_from(request).unwrap_err();
        assert_eq!(error.body.error.code, "invalid_server");
    }
}

#[test]
fn rejects_request_bodies() {
    let response = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            router(state())
                .oneshot(
                    Request::get("/health")
                        .header("content-length", "1")
                        .body(Body::from("x"))
                        .unwrap(),
                )
                .await
                .unwrap()
        });
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn refuses_an_occupied_port() {
    let held = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let result = start(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port), state());
    assert!(result.is_err());
}
