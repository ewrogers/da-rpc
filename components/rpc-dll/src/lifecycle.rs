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
    event_hook::{self, EventHook},
    identity,
    ipc::IpcWorker,
    map_size_hook::{self, MapSizeHook},
    tick_hook::{self, TickHook},
};

static LIFECYCLE: Mutex<Option<Lifecycle>> = Mutex::new(None);

struct Lifecycle {
    log: File,
    ipc: IpcWorker,
    event_hook: Option<EventHook>,
    map_size_hook: Option<MapSizeHook>,
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
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={}",
                    tick_hook::NAME,
                    darpc_game_client::EVENT_DISPATCHER_TICK_RVA,
                    hook.relocated_bytes()
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        tick_hook::NAME
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
                    tick_hook::NAME
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
            tick_hook::NAME
        );
        None
    };

    let mut map_size_hook = if identity.supported_client && hook_install_warning.is_none() {
        match MapSizeHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={}",
                    map_size_hook::NAME,
                    darpc_game_client::MAP_SIZE_HANDLER_RVA,
                    hook.relocated_bytes()
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        map_size_hook::NAME
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
                    map_size_hook::NAME
                );
                let error = error.into_io_error();
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
                map_size_hook::NAME
            );
        }
        None
    };

    let event_hook = if identity.supported_client && hook_install_warning.is_none() {
        match EventHook::install() {
            Ok(mut hook) => {
                let _ = writeln!(
                    log,
                    "event=hook_installed hook={} rva=0x{:08X} relocated_bytes={} queue_bytes={}",
                    event_hook::NAME,
                    darpc_game_client::EVENT_DISPATCH_RVA,
                    hook.relocated_bytes(),
                    crate::state_events::EVENT_QUEUE_BYTES
                );
                if let Some(warning) = hook.take_install_warning() {
                    let _ = writeln!(
                        log,
                        "event=hook_install_warning hook={} error={warning}",
                        event_hook::NAME
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
                    event_hook::NAME
                );
                let error = error.into_io_error();
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
                        "event hook installation failed: {error}; map-hook rollback: {}; tick-hook rollback: {}; IPC rollback: {}",
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
                event_hook::NAME
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
        map_size_hook,
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

    if let Some(hook) = active.event_hook.as_mut() {
        let final_health = event_hook::health();
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={} observations={} server_events={} events={} parse_errors={} read_failures={} invalid_bodies={}",
                    event_hook::NAME,
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
                        event_hook::NAME,
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
                    event_hook::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.map_size_hook.as_mut() {
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={}",
                    map_size_hook::NAME
                )?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    map_size_hook::NAME
                );
                return Err(error);
            }
        }
    }

    if let Some(hook) = active.tick_hook.as_mut() {
        let final_health = tick_hook::health();
        match hook.uninstall() {
            Ok(true) => {
                writeln!(
                    active.log,
                    "event=hook_removed hook={} ticks={}",
                    tick_hook::NAME,
                    final_health.tick_count
                )?;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = writeln!(
                    active.log,
                    "event=hook_remove_failed hook={} error={error}",
                    tick_hook::NAME
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
