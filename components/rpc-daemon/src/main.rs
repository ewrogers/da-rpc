//! daRPC daemon.

#[cfg(any(windows, test))]
mod action_source;
#[cfg(any(windows, test))]
mod api;
#[cfg(any(windows, test))]
mod auto_load;
#[cfg(any(windows, test))]
mod bulletin;
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
mod logging;
#[cfg(any(windows, test))]
mod managed;
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
mod shutdown;
#[cfg(any(windows, test))]
mod state;
#[cfg(any(windows, test))]
mod stream;

#[cfg(windows)]
use std::collections::BTreeSet;
use std::{env, process::ExitCode};

use options::{Options, USAGE, parse_options};

fn main() -> ExitCode {
    if let Err(error) = logging::init() {
        eprintln!("darpcd: {error}");
        return ExitCode::from(2);
    }
    let options = match parse_options(env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            tracing::error!(%error, "invalid command line");
            eprintln!("{USAGE}");
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
            tracing::error!(%error, "daemon failed");
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
    use managed::ManagedLifetime;
    use roster::ClientRoster;
    use std::{
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
    let _managed_lifetime = options
        .managed
        .then(|| {
            ManagedLifetime::start(sender.clone())
                .map_err(|error| format!("failed to start managed lifetime worker: {error}"))
        })
        .transpose()?;
    let (command_sender, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let mut roster = ClientRoster::new(explicit_pids, sender.clone());
    roster.reconcile(&discovered_pids, Instant::now());

    let api_state = ApiState::new(roster.snapshot(), Arc::clone(&lifecycle), sender.clone())
        .with_command_sender(command_sender)
        .with_maps_directory(maps_directory.clone());
    for pid in roster.pids() {
        discover_maps_directory(&api_state, pid);
    }
    let api_worker = api::start(options.listen, api_state.clone())
        .map_err(|error| format!("failed to listen on {}: {error}", options.listen))?;
    if !options.listen.ip().is_loopback() {
        tracing::warn!(
            "the HTTP API has no authentication or transport encryption; \
             restrict non-loopback access with a trusted network and Windows Firewall"
        );
    }
    tracing::info!(address = %api_worker.address(), auto_load = options.auto_load,
        managed = options.managed, "HTTP API listening");
    tracing::debug!(loader_path = %loader_path.display(), dll_path = %dll_path.display(),
        "runtime paths resolved");
    tracing::debug!(maps_path = %api_state.maps_directory().as_deref().map_or_else(
            || "automatic discovery pending".into(),
            |path| path.display().to_string()
        ), "maps path resolved");
    for client in roster.snapshot().clients {
        tracing::debug!(pid = client.pid, "client connecting");
    }

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
                                tracing::error!(pid, %error, "auto-load failed to start");
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
            Ok(DaemonEvent::HookBudgetExceeded { pid, timing, delta }) => {
                tracing::warn!(pid, stage = ?timing.stage, budget_us = timing.budget_us,
                    over_budget_delta = delta, over_budget_total = timing.over_budget_count,
                    maximum_duration_us = timing.maximum_duration_us,
                    last_duration_us = timing.last_duration_us, "hook budget exceeded");
            }
            Ok(DaemonEvent::TickRateChanged {
                pid,
                degraded,
                tick_delta,
                sample_ms,
                rate_hz,
                threshold_hz,
            }) => {
                if degraded {
                    tracing::warn!(
                        pid,
                        rate_hz,
                        threshold_hz,
                        tick_delta,
                        sample_ms,
                        "tick rate degraded"
                    );
                } else {
                    tracing::info!(
                        pid,
                        rate_hz,
                        threshold_hz,
                        tick_delta,
                        sample_ms,
                        "tick rate recovered"
                    );
                }
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
                        tracing::info!(pid, changed = outcome.changed, "auto-load completed");
                        publish_event(
                            &mut roster,
                            &api_state,
                            registry::ConnectionEvent::Connecting { pid },
                        );
                    }
                    Ok(outcome) => {
                        tracing::error!(
                            pid,
                            result_pid = outcome.pid,
                            darpc_loaded = outcome.darpc_loaded,
                            "auto-load returned invalid state"
                        );
                        publish_event(
                            &mut roster,
                            &api_state,
                            registry::ConnectionEvent::NotLoaded { pid },
                        );
                    }
                    Err(error) => {
                        tracing::error!(pid, code = error.code, message = %error.message, "auto-load failed");
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
            Ok(DaemonEvent::ManagedShutdown(result)) => {
                api_worker
                    .shutdown()
                    .map_err(|error| format!("failed to stop HTTP worker: {error}"))?;
                result.map_err(|error| format!("managed lifetime pipe failed: {error}"))?;
                tracing::info!("daemon stopped");
                return Ok(());
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
                tracing::warn!(%error, "client discovery failed");
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
        tracing::info!(path = %directory.display(), "maps directory discovered");
    }
}

#[cfg(windows)]
fn publish_event(
    roster: &mut roster::ClientRoster,
    api_state: &api::ApiState,
    event: registry::ConnectionEvent,
) {
    match roster.commit(event) {
        registry::CommitOutcome::Ignored => {}
        registry::CommitOutcome::Applied(change) => {
            logging::committed(&change);
            api_state.publish(roster.snapshot());
            api_state.publish_committed(change);
        }
        registry::CommitOutcome::ObservationRejected {
            pid,
            identity,
            reason,
        } => {
            api_state.publish(roster.snapshot());
            api_state.reject_observation(pid, identity);
            tracing::warn!(pid, %reason, "observation rejected; requesting a fresh baseline");
        }
    }
}

#[cfg(not(windows))]
fn run(_options: Options) -> Result<(), String> {
    Err("the daRPC daemon requires Windows".into())
}
