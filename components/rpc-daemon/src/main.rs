//! daRPC daemon.

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
mod group;
#[cfg(any(windows, test))]
mod lifecycle;
#[cfg(any(windows, test))]
mod messages;
#[cfg(any(windows, test))]
mod registry;
#[cfg(any(windows, test))]
mod state;
#[cfg(any(windows, test))]
mod stream;

use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    process::ExitCode,
};

const DEFAULT_PORT: u16 = 2626;
const USAGE: &str = concat!(
    "usage: darpcd [--pid <pid> ...] [--port <port> | --listen <ipv4[:port]>] ",
    "[--auto-load] ",
    "[--loader-path <path>] [--dll-path <path>] [--maps-path <path>]\n       ",
    "darpcd --print-openapi"
);

#[derive(Debug, Eq, PartialEq)]
struct Options {
    pids: Vec<u32>,
    listen: SocketAddrV4,
    auto_load: bool,
    loader_path: Option<PathBuf>,
    dll_path: Option<PathBuf>,
    maps_path: Option<PathBuf>,
    print_openapi: bool,
}

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

fn parse_options(arguments: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut arguments = arguments.into_iter();
    let mut pids = Vec::new();
    let mut unique = BTreeSet::new();
    let mut port = None;
    let mut listen = None;
    let mut auto_load = false;
    let mut loader_path = None;
    let mut dll_path = None;
    let mut maps_path = None;
    let mut print_openapi = false;

    while let Some(option) = arguments.next() {
        if option == "--pid" {
            let value = arguments
                .next()
                .ok_or_else(|| "--pid requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "PID must be valid Unicode".to_owned())?;
            let pid: u32 = value
                .parse()
                .map_err(|_| "PID must be an unsigned 32-bit integer".to_owned())?;
            if pid == 0 {
                return Err("PID must be greater than zero".into());
            }
            if !unique.insert(pid) {
                return Err(format!("PID {pid} was provided more than once"));
            }
            pids.push(pid);
        } else if option == "--port" {
            if port.is_some() {
                return Err("--port may be provided only once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| "--port requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "port must be valid Unicode".to_owned())?;
            let parsed: u16 = value
                .parse()
                .map_err(|_| "port must be an integer from 1 through 65535".to_owned())?;
            if parsed == 0 {
                return Err("port must be greater than zero".into());
            }
            port = Some(parsed);
        } else if option == "--listen" {
            if listen.is_some() {
                return Err("--listen may be provided only once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| "--listen requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "listen address must be valid Unicode".to_owned())?;
            listen = Some(parse_listen_address(value)?);
        } else if option == "--auto-load" {
            if auto_load {
                return Err("--auto-load may be provided only once".into());
            }
            auto_load = true;
        } else if option == "--loader-path" {
            parse_path_option(&mut arguments, &mut loader_path, "--loader-path")?;
        } else if option == "--dll-path" {
            parse_path_option(&mut arguments, &mut dll_path, "--dll-path")?;
        } else if option == "--maps-path" {
            parse_path_option(&mut arguments, &mut maps_path, "--maps-path")?;
        } else if option == "--print-openapi" {
            if print_openapi {
                return Err("--print-openapi may be provided only once".into());
            }
            print_openapi = true;
        } else {
            return Err(format!("unknown option `{}`", option.to_string_lossy()));
        }
    }

    if print_openapi
        && (!pids.is_empty()
            || port.is_some()
            || listen.is_some()
            || auto_load
            || loader_path.is_some()
            || dll_path.is_some()
            || maps_path.is_some())
    {
        return Err("--print-openapi cannot be combined with server options".into());
    }
    if port.is_some() && listen.is_some() {
        return Err("--port and --listen cannot be combined".into());
    }

    Ok(Options {
        pids,
        listen: listen.unwrap_or_else(|| {
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, port.unwrap_or(DEFAULT_PORT))
        }),
        auto_load,
        loader_path,
        dll_path,
        maps_path,
        print_openapi,
    })
}

fn parse_listen_address(value: &str) -> Result<SocketAddrV4, String> {
    if let Ok(address) = value.parse::<SocketAddrV4>() {
        if address.port() == 0 {
            return Err("listen port must be greater than zero".into());
        }
        return Ok(address);
    }
    value
        .parse::<Ipv4Addr>()
        .map(|address| SocketAddrV4::new(address, DEFAULT_PORT))
        .map_err(|_| "listen address must be an IPv4 address with an optional port".to_owned())
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

fn parse_path_option(
    arguments: &mut impl Iterator<Item = OsString>,
    destination: &mut Option<PathBuf>,
    option: &str,
) -> Result<(), String> {
    if destination.is_some() {
        return Err(format!("{option} may be provided only once"));
    }
    let value = arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))?;
    if value.is_empty() {
        return Err(format!("{option} requires a nonempty path"));
    }
    *destination = Some(PathBuf::from(value));
    Ok(())
}

#[cfg(windows)]
fn run(options: Options) -> Result<(), String> {
    use api::ApiState;
    use auto_load::{Action as AutoLoadAction, Policy as AutoLoadPolicy};
    use commands::{CommandReply, ROUTER_CAPACITY};
    use connection::Worker;
    use event::DaemonEvent;
    use lifecycle::{LifecycleControl, LoaderControl};
    use registry::Registry;
    use std::{
        collections::BTreeMap,
        io::Write as _,
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };

    const DISCOVERY_INTERVAL: Duration = Duration::from_secs(1);
    const LAUNCH_DISCOVERY_GRACE: Duration = Duration::from_secs(5);

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
    let mut desired_pids = explicit_pids.clone();
    desired_pids.extend(discovered_pids);

    let (sender, receiver) = mpsc::channel();
    let (command_sender, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
    let mut registry = Registry::new();
    let mut workers = BTreeMap::<u32, Worker>::new();
    for pid in desired_pids {
        track_client(pid, &sender, &mut workers, &mut registry);
    }

    let api_state = ApiState::new(registry.snapshot(), Arc::clone(&lifecycle), sender.clone())
        .with_command_sender(command_sender)
        .with_maps_directory(maps_directory.clone());
    for &pid in workers.keys() {
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
    for client in registry.snapshot().clients {
        println!("client pid={} status=connecting", client.pid);
    }
    let _ = std::io::stdout().flush();

    let mut next_discovery = Instant::now() + DISCOVERY_INTERVAL;
    let mut launch_grace = BTreeMap::<u32, Instant>::new();
    let mut auto_load = AutoLoadPolicy::new(options.auto_load);

    loop {
        let timeout = next_discovery.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(timeout) {
            Ok(DaemonEvent::Connection(event)) => {
                if workers.contains_key(&event.pid()) {
                    match auto_load.observe(&event) {
                        AutoLoadAction::Publish => {
                            publish_event(&mut registry, &api_state, &event);
                        }
                        AutoLoadAction::Suppress => {}
                        AutoLoadAction::Start(attempt) => {
                            let pid = event.pid();
                            publish_event(
                                &mut registry,
                                &api_state,
                                &registry::ConnectionEvent::Initializing { pid },
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
                                    &mut registry,
                                    &api_state,
                                    &registry::ConnectionEvent::NotLoaded { pid },
                                );
                            }
                        }
                    }
                }
            }
            Ok(DaemonEvent::Status(event)) => {
                if workers.contains_key(&event.pid()) {
                    if matches!(event, registry::ConnectionEvent::Initializing { .. }) {
                        auto_load.suppress(event.pid());
                    }
                    publish_event(&mut registry, &api_state, &event);
                }
            }
            Ok(DaemonEvent::AutoLoadFinished {
                pid,
                attempt,
                result,
            }) => {
                if !auto_load.finish(pid, attempt) || !workers.contains_key(&pid) {
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
                            &mut registry,
                            &api_state,
                            &registry::ConnectionEvent::Connecting { pid },
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
                            &mut registry,
                            &api_state,
                            &registry::ConnectionEvent::NotLoaded { pid },
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "darpcd: client pid={pid} auto-load failed code={} message={:?}",
                            error.code, error.message
                        );
                        publish_event(
                            &mut registry,
                            &api_state,
                            &registry::ConnectionEvent::NotLoaded { pid },
                        );
                    }
                }
            }
            Ok(DaemonEvent::Track(pid)) => {
                auto_load.suppress(pid);
                launch_grace.insert(pid, Instant::now() + LAUNCH_DISCOVERY_GRACE);
                track_client(pid, &sender, &mut workers, &mut registry);
                discover_maps_directory(&api_state, pid);
                api_state.publish(registry.snapshot());
            }
            Ok(DaemonEvent::CommandsReady) => {
                if let Ok(call) = command_receiver.try_recv() {
                    if let Some(worker) = workers.get(&call.pid) {
                        worker.route_command(call);
                    } else {
                        let _ = call.reply.send(CommandReply::Unavailable);
                    }
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
        launch_grace
            .retain(|pid, deadline| !discovered.contains(pid) && *deadline > Instant::now());

        let mut desired = explicit_pids.clone();
        desired.extend(discovered);
        desired.extend(launch_grace.keys().copied());
        let mut changed = false;
        for &pid in &desired {
            changed |= track_client(pid, &sender, &mut workers, &mut registry);
            discover_maps_directory(&api_state, pid);
        }

        let removed = workers
            .keys()
            .copied()
            .filter(|pid| !desired.contains(pid))
            .collect::<Vec<_>>();
        for pid in removed {
            if let Some(worker) = workers.remove(&pid) {
                worker.stop();
            }
            changed |= registry.remove(pid);
            auto_load.forget(pid);
            println!("client pid={pid} status=removed");
        }
        if changed {
            api_state.publish(registry.snapshot());
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
fn track_client(
    pid: u32,
    sender: &std::sync::mpsc::Sender<event::DaemonEvent>,
    workers: &mut std::collections::BTreeMap<u32, connection::Worker>,
    registry: &mut registry::Registry,
) -> bool {
    if workers.contains_key(&pid) {
        return false;
    }
    let event = registry::ConnectionEvent::Connecting { pid };
    registry.apply(&event);
    match connection::spawn(pid, sender.clone()) {
        Ok(worker) => {
            workers.insert(pid, worker);
        }
        Err(error) => {
            let event = registry::ConnectionEvent::Disconnected {
                pid,
                identity: None,
                reason: format!("failed to start connection worker: {error}"),
            };
            registry.apply(&event);
            eprintln!("darpcd: {}", registry::render_event(&event));
        }
    }
    true
}

#[cfg(windows)]
fn publish_event(
    registry: &mut registry::Registry,
    api_state: &api::ApiState,
    event: &registry::ConnectionEvent,
) {
    if registry.apply(event) {
        api_state.publish(registry.snapshot());
        api_state.publish_connection_event(event);
        println!("{}", registry::render_event(event));
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

#[cfg(not(windows))]
fn run(_options: Options) -> Result<(), String> {
    Err("the daRPC daemon requires Windows".into())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PORT, Options, parse_options};
    use std::{
        ffi::OsString,
        net::{Ipv4Addr, SocketAddrV4},
    };

    fn default_listen() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_PORT)
    }

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_repeated_pid_options_in_order() {
        assert_eq!(
            parse_options(arguments(&["--pid", "42", "--pid", "7"])).unwrap(),
            Options {
                pids: vec![42, 7],
                listen: default_listen(),
                auto_load: false,
                loader_path: None,
                dll_path: None,
                maps_path: None,
                print_openapi: false,
            }
        );
        assert_eq!(
            parse_options(arguments(&["--port", "3000", "--pid", "42"])).unwrap(),
            Options {
                pids: vec![42],
                listen: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3000),
                auto_load: false,
                loader_path: None,
                dll_path: None,
                maps_path: None,
                print_openapi: false,
            }
        );
    }

    #[test]
    fn parses_discovery_and_management_options() {
        assert_eq!(
            parse_options(arguments(&[
                "--loader-path",
                "tools/loader.exe",
                "--dll-path",
                "tools/darpc.dll",
                "--maps-path",
                "C:\\Dark Ages\\Maps",
            ]))
            .unwrap(),
            Options {
                pids: Vec::new(),
                listen: default_listen(),
                auto_load: false,
                loader_path: Some("tools/loader.exe".into()),
                dll_path: Some("tools/darpc.dll".into()),
                maps_path: Some("C:\\Dark Ages\\Maps".into()),
                print_openapi: false,
            }
        );
        assert!(parse_options(Vec::<OsString>::new()).is_ok());
        assert!(
            parse_options(arguments(&["--auto-load"]))
                .unwrap()
                .auto_load
        );
        assert!(
            parse_options(arguments(&["--print-openapi"]))
                .unwrap()
                .print_openapi
        );
    }

    #[test]
    fn parses_explicit_listen_addresses() {
        assert_eq!(
            parse_options(arguments(&["--listen", "0.0.0.0:2620"]))
                .unwrap()
                .listen,
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 2620)
        );
        assert_eq!(
            parse_options(arguments(&["--listen", "192.168.1.5"]))
                .unwrap()
                .listen,
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), DEFAULT_PORT)
        );
    }

    #[test]
    fn rejects_invalid_targets_and_ports() {
        assert!(parse_options(arguments(&["--pid", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--pid", "7"])).is_err());
        assert!(parse_options(arguments(&["--pids", "7,8"])).is_err());
        assert!(parse_options(arguments(&["--client-path", "Darkages.exe"])).is_err());
        assert!(parse_options(arguments(&["--maps-path", ""])).is_err());
        assert!(parse_options(arguments(&["--auto-load", "--auto-load"])).is_err());
        assert!(parse_options(arguments(&["--print-openapi", "--port", "2626"])).is_err());
        assert!(parse_options(arguments(&["--print-openapi", "--print-openapi"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "65536"])).is_err());
        assert!(parse_options(arguments(&["--listen"])).is_err());
        assert!(parse_options(arguments(&["--listen", "localhost:2626"])).is_err());
        assert!(parse_options(arguments(&["--listen", "0.0.0.0:0"])).is_err());
        assert!(parse_options(arguments(&["--listen", "0.0.0.0:2626", "--port", "2626"])).is_err());
        assert!(
            parse_options(arguments(&["--listen", "0.0.0.0", "--listen", "127.0.0.1"])).is_err()
        );
        assert!(parse_options(arguments(&["--print-openapi", "--listen", "127.0.0.1"])).is_err());
        assert!(
            parse_options(arguments(&[
                "--pid", "7", "--port", "2626", "--port", "2627"
            ]))
            .is_err()
        );
        assert!(
            parse_options(arguments(&[
                "--loader-path",
                "first.exe",
                "--loader-path",
                "second.exe",
            ]))
            .is_err()
        );
        assert!(
            parse_options(arguments(&[
                "--maps-path",
                "first",
                "--maps-path",
                "second",
            ]))
            .is_err()
        );
    }
}
