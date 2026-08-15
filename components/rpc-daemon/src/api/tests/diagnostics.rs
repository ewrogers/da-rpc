use super::*;
use crate::commands::{ClientOperation, CommandReply};
use darpc_protocol::{
    DiagnosticsMode, DiagnosticsOperation, DiagnosticsResponse, HookTimingRecord, HookTimingStage,
};

fn diagnostics_state() -> (ApiState, std::thread::JoinHandle<()>) {
    let mut registry = Registry::new();
    let hello = hello();
    let identity = RegistryClientIdentity::from_hello(hello);
    registry.apply(&ConnectionEvent::Connected {
        pid: 42,
        hello,
        selected_version: SUPPORTED_VERSIONS.max,
    });
    let (events, event_receiver) = mpsc::channel();
    let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
        .with_command_sender(commands);
    let worker = std::thread::spawn(move || {
        assert!(matches!(
            event_receiver.recv().unwrap(),
            DaemonEvent::CommandsReady
        ));
        let call = command_receiver.recv().unwrap();
        assert_eq!(call.pid, 42);
        assert_eq!(call.identity, identity);
        let operation = match call.operation {
            ClientOperation::Diagnostics(operation) => operation,
            operation => panic!("unexpected operation {operation:?}"),
        };
        let mode = if operation == DiagnosticsOperation::Disable {
            DiagnosticsMode::Disabled
        } else {
            DiagnosticsMode::HookTiming
        };
        call.reply
            .send(CommandReply::Diagnostics(DiagnosticsResponse {
                request_id: 7,
                mode,
                hook_timings: std::array::from_fn(|index| HookTimingRecord {
                    stage: [
                        HookTimingStage::Tick,
                        HookTimingStage::Movement,
                        HookTimingStage::Commands,
                        HookTimingStage::Player,
                        HookTimingStage::State,
                        HookTimingStage::Snapshot,
                        HookTimingStage::Event,
                    ][index],
                    budget_us: 5_000,
                    call_count: 2,
                    total_duration_us: 30,
                    maximum_duration_us: 20,
                    over_budget_count: 1,
                    last_duration_us: 10,
                }),
            }))
            .unwrap();
    });
    (state, worker)
}

#[test]
fn queries_hook_timing_counters() {
    let (state, worker) = diagnostics_state();
    let body = json_with_state(state, "/clients/42/diagnostics/hooks");
    worker.join().unwrap();
    assert_eq!(body["mode"], "hook_timing");
    assert_eq!(body["hook_timings"][0]["stage"], "tick");
    assert_eq!(body["hook_timings"][0]["average_duration_us"], 15);
}

#[test]
fn disables_runtime_diagnostics() {
    let (state, worker) = diagnostics_state();
    let response = put_json(state, "/clients/42/diagnostics", r#"{"mode":"disabled"}"#);
    worker.join().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response)["mode"], "disabled");
}
