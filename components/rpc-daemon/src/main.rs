//! daRPC daemon.

#[cfg(any(windows, test))]
mod action_source;
#[cfg(any(windows, test))]
mod api;
#[cfg(any(windows, test))]
mod auto_load;
#[cfg(any(windows, test))]
mod commands;
#[cfg(windows)]
mod connection;
#[cfg(any(windows, test))]
mod dialog;
#[cfg(windows)]
mod discovery;
#[cfg(any(windows, test))]
mod event;
#[cfg(any(windows, test))]
mod exchange;
#[cfg(any(windows, test))]
mod field_map;
#[cfg(any(windows, test))]
mod group;
#[cfg(any(windows, test))]
mod lifecycle;
#[cfg(any(windows, test))]
mod message_dialog;
#[cfg(any(windows, test))]
mod messages;
mod options;
#[cfg(any(windows, test))]
mod registry;
#[cfg(any(windows, test))]
mod resync_status;
#[cfg(any(windows, test))]
mod roster;
#[cfg(any(windows, test))]
mod state;
#[cfg(any(windows, test))]
mod stream;

#[cfg(windows)]
use std::collections::BTreeSet;
use std::{env, process::ExitCode};

use options::{Options, USAGE, parse_options};

fn main() -> ExitCode {
    let options = match parse_options(env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("darpcd: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let result = if options.print_openapi {
        print_openapi()
    } else {
        run(options)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("darpcd: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(windows)]
fn print_openapi() -> Result<(), String> {
    let json = serde_json::to_string_pretty(&api::openapi())
        .map_err(|error| format!("failed to serialize OpenAPI: {error}"))?;
    println!("{json}");
    Ok(())
}

#[cfg(not(windows))]
fn print_openapi() -> Result<(), String> {
    Err("OpenAPI export requires Windows".into())
}

#[cfg(windows)]
fn run(options: Options) -> Result<(), String> {
    use api::ApiState;
    use auto_load::{Action as AutoLoadAction, Policy as AutoLoadPolicy};
    use commands::ROUTER_CAPACITY;
    use event::DaemonEvent;
    use lifecycle::{LifecycleControl, LoaderControl};
    use roster::ClientRoster;
    use std::{
        io::Write as _,
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };

    const DISCOVERY_INTERVAL: Duration = Duration::from_secs(1);
    let component_directory = env::current_exe()
        .map_err(|error| format!("failed to resolve darpcd.exe: {error}"))?
        .parent()
        .ok_or_else(|| "darpcd.exe has no parent directory".to_owned())?
        .to_owned();
    let maps_directory = options
        .maps_path
        .map(|path| {
            std::fs::canonicalize(&path)
                .map_err(|error| {
                    format!("failed to resolve maps path `{}`: {error}", path.display())
                })
                .and_then(|resolved| {
                    resolved.is_dir().then_some(resolved).ok_or_else(|| {
                        format!("maps path is not a directory: `{}`", path.display())
                    })
                })
        })
        .transpose()?;
    let loader_path = options
        .loader_path
        .unwrap_or_else(|| component_directory.join("loader.exe"));
    let dll_path = options
        .dll_path
        .unwrap_or_else(|| component_directory.join("darpc.dll"));
    let lifecycle: Arc<dyn LifecycleControl> =
        Arc::new(LoaderControl::new(loader_path.clone(), dll_path.clone()));

    let explicit_pids = options.pids.into_iter().collect::<BTreeSet<_>>();
    let discovered_pids = discovery::client_pids()
        .map_err(|error| format!("failed to enumerate game windows: {error}"))?;
    let (sender, receiver) = mpsc::channel();
    let (command_sender, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let mut roster = ClientRoster::new(explicit_pids, sender.clone());
    roster.reconcile(&discovered_pids, Instant::now());

    let api_state = ApiState::new(roster.snapshot(), Arc::clone(&lifecycle), sender.clone())
        .with_command_sender(command_sender)
        .with_maps_directory(maps_directory.clone());
    for pid in roster.pids() {
        discover_maps_directory(&api_state, pid);
    }
    let _api_worker = api::start(options.listen, api_state.clone())
        .map_err(|error| format!("failed to listen on {}: {error}", options.listen))?;
    if !options.listen.ip().is_loopback() {
        eprintln!(
            "darpcd: warning: the HTTP API has no authentication or transport encryption; \
             restrict non-loopback access with a trusted network and Windows Firewall"
        );
    }
    println!("HTTP API listening on http://{}", options.listen);
    println!("loader path: {}", loader_path.display());
    println!("DLL path: {}", dll_path.display());
    println!(
        "maps path: {}",
        api_state.maps_directory().as_deref().map_or_else(
            || "automatic discovery pending".into(),
            |path| path.display().to_string()
        )
    );
    println!(
        "auto-load: {}",
        if options.auto_load {
            "enabled"
        } else {
            "disabled"
        }
    );
    for client in roster.snapshot().clients {
        println!("client pid={} status=connecting", client.pid);
    }
    let _ = std::io::stdout().flush();

    let mut next_discovery = Instant::now() + DISCOVERY_INTERVAL;
    let mut auto_load = AutoLoadPolicy::new(options.auto_load);

    loop {
        let timeout = next_discovery.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(timeout) {
            Ok(DaemonEvent::Connection(event)) => {
                if roster.contains(event.pid()) {
                    match auto_load.observe(&event) {
                        AutoLoadAction::Publish => {
                            publish_event(&mut roster, &api_state, event);
                        }
                        AutoLoadAction::Suppress => {}
                        AutoLoadAction::Start(attempt) => {
                            let pid = event.pid();
                            publish_event(
                                &mut roster,
                                &api_state,
                                registry::ConnectionEvent::Initializing { pid },
                            );
                            if let Err(error) = auto_load::spawn(
                                pid,
                                attempt,
                                Arc::clone(&lifecycle),
                                sender.clone(),
                            ) {
                                auto_load.finish(pid, attempt);
                                eprintln!(
                                    "darpcd: client pid={pid} auto-load failed to start: {error}"
                                );
                                publish_event(
                                    &mut roster,
                                    &api_state,
                                    registry::ConnectionEvent::NotLoaded { pid },
                                );
                            }
                        }
                    }
                }
            }
            Ok(DaemonEvent::Timing(message)) => {
                println!("{message}");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            Ok(DaemonEvent::Status(event)) => {
                if roster.contains(event.pid()) {
                    if matches!(&event, registry::ConnectionEvent::Initializing { .. }) {
                        auto_load.suppress(event.pid());
                    }
                    publish_event(&mut roster, &api_state, event);
                }
            }
            Ok(DaemonEvent::AutoLoadFinished {
                pid,
                attempt,
                result,
            }) => {
                if !auto_load.finish(pid, attempt) || !roster.contains(pid) {
                    continue;
                }
                match result {
                    Ok(outcome) if outcome.pid == pid && outcome.darpc_loaded => {
                        println!(
                            "client pid={pid} auto-load={}",
                            if outcome.changed {
                                "loaded"
                            } else {
                                "already_loaded"
                            }
                        );
                        publish_event(
                            &mut roster,
                            &api_state,
                            registry::ConnectionEvent::Connecting { pid },
                        );
                    }
                    Ok(outcome) => {
                        eprintln!(
                            concat!(
                                "darpcd: client pid={} auto-load returned invalid state ",
                                "result_pid={} darpc_loaded={}"
                            ),
                            pid, outcome.pid, outcome.darpc_loaded
                        );
                        publish_event(
                            &mut roster,
                            &api_state,
                            registry::ConnectionEvent::NotLoaded { pid },
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "darpcd: client pid={pid} auto-load failed code={} message={:?}",
                            error.code, error.message
                        );
                        publish_event(
                            &mut roster,
                            &api_state,
                            registry::ConnectionEvent::NotLoaded { pid },
                        );
                    }
                }
            }
            Ok(DaemonEvent::Track(pid)) => {
                auto_load.suppress(pid);
                let changed = roster.track_launched(pid, Instant::now());
                discover_maps_directory(&api_state, pid);
                if changed {
                    api_state.publish(roster.snapshot());
                }
            }
            Ok(DaemonEvent::CommandsReady) => {
                if let Ok(call) = command_receiver.try_recv() {
                    roster.route_command(call);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("daemon event channel disconnected".into());
            }
        }

        if Instant::now() < next_discovery {
            continue;
        }
        next_discovery = Instant::now() + DISCOVERY_INTERVAL;
        let discovered = match discovery::client_pids() {
            Ok(discovered) => discovered,
            Err(error) => {
                eprintln!("darpcd: client discovery failed: {error}");
                continue;
            }
        };
        let outcome = roster.reconcile(&discovered, Instant::now());
        for pid in roster.pids() {
            discover_maps_directory(&api_state, pid);
        }
        for pid in outcome.removed {
            auto_load.forget(pid);
        }
        if outcome.changed {
            api_state.publish(roster.snapshot());
            let _ = std::io::stdout().flush();
        }
    }
}

#[cfg(windows)]
fn discover_maps_directory(state: &api::ApiState, pid: u32) {
    if state.maps_directory().is_some() {
        return;
    }
    let Ok(directory) = discovery::client_maps_directory(pid) else {
        return;
    };
    let Ok(directory) = std::fs::canonicalize(directory) else {
        return;
    };
    if directory.is_dir() && state.set_maps_directory_if_unset(directory.clone()) {
        println!("maps path: auto-detected {}", directory.display());
    }
}

#[cfg(windows)]
fn publish_event(
    roster: &mut roster::ClientRoster,
    api_state: &api::ApiState,
    event: registry::ConnectionEvent,
) {
    let rendered = registry::render_event(&event);
    match roster.commit(event) {
        registry::CommitOutcome::Ignored => return,
        registry::CommitOutcome::Applied(change) => {
            api_state.publish(roster.snapshot());
            api_state.publish_committed(change);
            println!("{rendered}");
        }
        registry::CommitOutcome::ObservationRejected {
            pid,
            identity,
            reason,
        } => {
            api_state.publish(roster.snapshot());
            api_state.reject_observation(pid, identity);
            eprintln!("darpcd: client pid={pid} observation rejected: {reason}");
        }
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[cfg(not(windows))]
fn run(_options: Options) -> Result<(), String> {
    Err("the daRPC daemon requires Windows".into())
}
