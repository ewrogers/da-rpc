use std::{
    env,
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    process,
    sync::Mutex,
};

use crate::{
    commands,
    hooks::{
        event::{self, EventHook},
        map_size::{self, MapSizeHook},
        outgoing::{self, OutgoingHook},
        path::{self, PathHook},
        tick::{self, TickHook},
    },
    identity,
    ipc::IpcWorker,
};

static LIFECYCLE: Mutex<Option<Lifecycle>> = Mutex::new(None);

struct Lifecycle {
    log: File,
    ipc: IpcWorker,
    event_hook: Option<EventHook>,
    outgoing_hook: Option<OutgoingHook>,
    map_size_hook: Option<MapSizeHook>,
    path_hook: Option<PathHook>,
    tick_hook: Option<TickHook>,
}

#[derive(Debug)]
pub(crate) struct InitializeError {
    source: io::Error,
    unload_safe: bool,
}

impl InitializeError {
    pub(crate) const fn unload_is_safe(&self) -> bool {
        self.unload_safe
    }
}

impl From<io::Error> for InitializeError {
    fn from(source: io::Error) -> Self {
        Self {
            source,
            unload_safe: true,
        }
    }
}

impl fmt::Display for InitializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for InitializeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub(crate) fn initialize() -> Result<(), InitializeError> {
    let mut lifecycle = LIFECYCLE
        .lock()
        .map_err(|_| io::Error::other("lifecycle lock is poisoned"))?;

    if lifecycle.is_some() {
        return Ok(());
    }

    let log_path = log_path()?;
    let log_directory = log_path
        .parent()
        .ok_or_else(|| io::Error::other("log path has no parent directory"))?;

    fs::create_dir_all(log_directory)?;

    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let identity = match identity::current() {
        Ok(identity) => identity,
        Err(error) => {
            let _ = writeln!(
                log,
                "event=initialization_failed stage=client_validation error={error}"
            );
            return Err(error.into());
        }
    };
    let mut ipc = IpcWorker::start(identity.hello, log.try_clone()?)?;
    let mut hook_install_warning = None;
    let mut tick_hook = if identity.supported_client {
        match TickHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    concat!(
                        "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={} ",
                        "command_capacity={} commands_per_tick={}"
                    ),
                    tick::NAME,
                    darpc_game_client::EVENT_DISPATCHER_TICK_RVA,
                    hook.relocated_bytes(),
                    commands::COMMAND_CAPACITY,
                    commands::COMMANDS_PER_TICK
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        tick::NAME
                    );
                    hook_install_warning = Some(warning);
                }
                Some(hook)
            }
            Err(error) => {
                let unload_safe = error.unload_is_safe();
                let _ = writeln!(
                    log,
                    "event=initialization_failed stage=hook_install hook={} error={error}",
                    tick::NAME
                );
                let error = error.into_io_error();
                if let Err(shutdown_error) = ipc.shutdown() {
                    return Err(InitializeError {
                        source: io::Error::other(format!(
                            "tick hook installation failed: {error}; IPC rollback failed: {shutdown_error}"
                        )),
                        unload_safe,
                    });
                }
                return Err(InitializeError {
                    source: error,
                    unload_safe,
                });
            }
        }
    } else {
        let _ = writeln!(
            log,
            "event=hook_skipped hook={} reason=unsupported_client_debug_bypass",
            tick::NAME
        );
        None
    };

    let mut map_size_hook = if identity.supported_client && hook_install_warning.is_none() {
        match MapSizeHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={}",
                    map_size::NAME,
                    darpc_game_client::MAP_SIZE_HANDLER_RVA,
                    hook.relocated_bytes()
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        map_size::NAME
                    );
                    hook_install_warning = Some(warning);
                }
                Some(hook)
            }
            Err(error) => {
                let mut unload_safe = error.unload_is_safe();
                let _ = writeln!(
                    log,
                    "event=initialization_failed stage=hook_install hook={} error={error}",
                    map_size::NAME
                );
                let error = error.into_io_error();
                commands::cancel_pending();
                let rollback_error = tick_hook.as_mut().and_then(|hook| hook.uninstall().err());
                if rollback_error.is_some() {
                    unload_safe = false;
                }
                let ipc_error = ipc.shutdown().err();
                let source = match (rollback_error, ipc_error) {
                    (None, None) => error,
                    (hook_error, ipc_error) => io::Error::other(format!(
                        "map-size hook installation failed: {error}; tick-hook rollback: {}; IPC rollback: {}",
                        hook_error
                            .map(|error| error.to_string())
                            .unwrap_or_else(|| "ok".to_owned()),
                        ipc_error
                            .map(|error| error.to_string())
                            .unwrap_or_else(|| "ok".to_owned())
                    )),
                };
                return Err(InitializeError {
                    source,
                    unload_safe,
                });
            }
        }
    } else {
        if !identity.supported_client {
            let _ = writeln!(
                log,
                "event=hook_skipped hook={} reason=unsupported_client_debug_bypass",
                map_size::NAME
            );
        }
        None
    };

    let mut path_hook = if identity.supported_client && hook_install_warning.is_none() {
        match PathHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={}",
                    path::NAME,
                    darpc_game_client::BUILD_BREADTH_FIRST_PATH_RVA,
                    hook.relocated_bytes()
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        path::NAME
                    );
                    hook_install_warning = Some(warning);
                }
                Some(hook)
            }
            Err(error) => {
                let mut unload_safe = error.unload_is_safe();
                let _ = writeln!(
                    log,
                    "event=initialization_failed stage=hook_install hook={} error={error}",
                    path::NAME
                );
                let error = error.into_io_error();
                commands::cancel_pending();
                let map_error = map_size_hook
                    .as_mut()
                    .and_then(|hook| hook.uninstall().err());
                let tick_error = tick_hook.as_mut().and_then(|hook| hook.uninstall().err());
                if map_error.is_some() || tick_error.is_some() {
                    unload_safe = false;
                }
                let ipc_error = ipc.shutdown().err();
                let source = match (&map_error, &tick_error, &ipc_error) {
                    (None, None, None) => error,
                    _ => io::Error::other(format!(
                        "path hook installation failed: {error}; map-hook rollback: {}; tick-hook rollback: {}; IPC rollback: {}",
                        rollback_result(map_error),
                        rollback_result(tick_error),
                        rollback_result(ipc_error)
                    )),
                };
                return Err(InitializeError {
                    source,
                    unload_safe,
                });
            }
        }
    } else {
        if !identity.supported_client {
            let _ = writeln!(
                log,
                "event=hook_skipped hook={} reason=unsupported_client_debug_bypass",
                path::NAME
            );
        }
        None
    };

    let mut event_hook = if identity.supported_client && hook_install_warning.is_none() {
        match EventHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={} queue_bytes={}",
                    event::NAME,
                    darpc_game_client::EVENT_DISPATCH_RVA,
                    hook.relocated_bytes(),
                    crate::state::EVENT_QUEUE_BYTES
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        event::NAME
                    );
                    hook_install_warning = Some(warning);
                }
                Some(hook)
            }
            Err(error) => {
                let mut unload_safe = error.unload_is_safe();
                let _ = writeln!(
                    log,
                    "event=initialization_failed stage=hook_install hook={} error={error}",
                    event::NAME
                );
                let error = error.into_io_error();
                commands::cancel_pending();
                let path_error = path_hook.as_mut().and_then(|hook| hook.uninstall().err());
                let map_error = map_size_hook
                    .as_mut()
                    .and_then(|hook| hook.uninstall().err());
                let tick_error = tick_hook.as_mut().and_then(|hook| hook.uninstall().err());
                if path_error.is_some() || map_error.is_some() || tick_error.is_some() {
                    unload_safe = false;
                }
                let ipc_error = ipc.shutdown().err();
                let source = match (&path_error, &map_error, &tick_error, &ipc_error) {
                    (None, None, None, None) => error,
                    _ => io::Error::other(format!(
                        "event hook installation failed: {error}; path-hook rollback: {}; map-hook rollback: {}; tick-hook rollback: {}; IPC rollback: {}",
                        rollback_result(path_error),
                        rollback_result(map_error),
                        rollback_result(tick_error),
                        rollback_result(ipc_error)
                    )),
                };
                return Err(InitializeError {
                    source,
                    unload_safe,
                });
            }
        }
    } else {
        if !identity.supported_client {
            let _ = writeln!(
                log,
                "event=hook_skipped hook={} reason=unsupported_client_debug_bypass",
                event::NAME
            );
        }
        None
    };

    let outgoing_hook = if identity.supported_client && hook_install_warning.is_none() {
        match OutgoingHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={}",
                    outgoing::NAME,
                    darpc_game_client::CLIENT_PACKET_SUBMIT_RVA,
                    hook.relocated_bytes()
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        outgoing::NAME
                    );
                    hook_install_warning = Some(warning);
                }
                Some(hook)
            }
            Err(error) => {
                let mut unload_safe = error.unload_is_safe();
                let _ = writeln!(
                    log,
                    "event=initialization_failed stage=hook_install hook={} error={error}",
                    outgoing::NAME
                );
                let error = error.into_io_error();
                commands::cancel_pending();
                let event_error = event_hook.as_mut().and_then(|hook| hook.uninstall().err());
                let path_error = path_hook.as_mut().and_then(|hook| hook.uninstall().err());
                let map_error = map_size_hook
                    .as_mut()
                    .and_then(|hook| hook.uninstall().err());
                let tick_error = tick_hook.as_mut().and_then(|hook| hook.uninstall().err());
                if event_error.is_some()
                    || path_error.is_some()
                    || map_error.is_some()
                    || tick_error.is_some()
                {
                    unload_safe = false;
                }
                let ipc_error = ipc.shutdown().err();
                let source = match (
                    &event_error,
                    &path_error,
                    &map_error,
                    &tick_error,
                    &ipc_error,
                ) {
                    (None, None, None, None, None) => error,
                    _ => io::Error::other(format!(
                        "outgoing hook installation failed: {error}; event-hook rollback: {}; path-hook rollback: {}; map-hook rollback: {}; tick-hook rollback: {}; IPC rollback: {}",
                        rollback_result(event_error),
                        rollback_result(path_error),
                        rollback_result(map_error),
                        rollback_result(tick_error),
                        rollback_result(ipc_error)
                    )),
                };
                return Err(InitializeError {
                    source,
                    unload_safe,
                });
            }
        }
    } else {
        if !identity.supported_client {
            let _ = writeln!(
                log,
                "event=hook_skipped hook={} reason=unsupported_client_debug_bypass",
                outgoing::NAME
            );
        }
        None
    };

    if hook_install_warning.is_none() {
        let _ = writeln!(
            log,
            "event=initialized pid={} version={}",
            process::id(),
            env!("CARGO_PKG_VERSION")
        );
    }

    *lifecycle = Some(Lifecycle {
        log,
        ipc,
        event_hook,
        outgoing_hook,
        map_size_hook,
        path_hook,
        tick_hook,
    });

    if let Some(warning) = hook_install_warning {
        return Err(InitializeError {
            source: warning,
            unload_safe: false,
        });
    }

    Ok(())
}

pub(crate) fn shutdown() -> io::Result<()> {
    let mut lifecycle = LIFECYCLE
        .lock()
        .map_err(|_| io::Error::other("lifecycle lock is poisoned"))?;

    let Some(active) = lifecycle.as_mut() else {
        return Ok(());
    };

    active.ipc.shutdown()?;
    commands::cancel_pending();

    if let Some(hook) = active.outgoing_hook.as_mut() {
        let final_health = outgoing::health();
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={} observations={} read_failures={}",
                    outgoing::NAME,
                    final_health.observation_count,
                    final_health.read_failure_count
                )?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    outgoing::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.event_hook.as_mut() {
        let final_health = event::health();
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={} observations={} server_events={} events={} parse_errors={} read_failures={} invalid_bodies={}",
                    event::NAME,
                    final_health.observation_count,
                    final_health.server_event_count,
                    final_health.event_count,
                    final_health.parse_error_count,
                    final_health.read_failure_count,
                    final_health.invalid_body_count
                )?;
                if final_health.parse_error_count != 0 {
                    writeln!(
                        active.log,
                        "event=hook_parse_failure hook={} opcode=0x{:02X} fields=0x{:02X} body_length={} offset={} needed={} remaining={}",
                        event::NAME,
                        final_health.last_parse_opcode,
                        final_health.last_parse_fields,
                        final_health.last_parse_body_length,
                        final_health.last_parse_offset,
                        final_health.last_parse_needed,
                        final_health.last_parse_remaining
                    )?;
                }
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    event::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.path_hook.as_mut() {
        match hook.uninstall() {
            Ok(true) => {
                writeln!(active.log, "event=hook_removed hook={}", path::NAME)?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    path::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.map_size_hook.as_mut() {
        match hook.uninstall() {
            Ok(true) => {
                writeln!(active.log, "event=hook_removed hook={}", map_size::NAME)?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    map_size::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.tick_hook.as_mut() {
        let final_health = tick::health();
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={} ticks={}",
                    tick::NAME,
                    final_health.tick_count
                )?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    tick::NAME
                );
                return Err(error);
            }
        }
    }

    writeln!(
        active.log,
        "event=shutdown pid={} version={}",
        process::id(),
        env!("CARGO_PKG_VERSION")
    )?;

    *lifecycle = None;

    Ok(())
}

pub(crate) fn log_path() -> io::Result<PathBuf> {
    let user_profile = env::var_os("USERPROFILE")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "USERPROFILE is not set"))?;

    Ok(PathBuf::from(user_profile)
        .join("darpc")
        .join("logs")
        .join(format!("pid-{}.log", process::id())))
}

fn rollback_result(error: Option<io::Error>) -> String {
    error.map_or_else(|| "ok".to_owned(), |error| error.to_string())
}
