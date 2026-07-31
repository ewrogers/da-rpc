use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    process,
    sync::Mutex,
};

use crate::{identity, ipc::IpcWorker};

static LIFECYCLE: Mutex<Option<Lifecycle>> = Mutex::new(None);

struct Lifecycle {
    log: File,
    ipc: IpcWorker,
}

pub(crate) fn initialize() -> io::Result<()> {
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

    let hello = identity::hello()?;
    let ipc = IpcWorker::start(hello, log.try_clone()?)?;

    let _ = writeln!(
        log,
        "event=initialized pid={} version={}",
        process::id(),
        env!("CARGO_PKG_VERSION")
    );

    *lifecycle = Some(Lifecycle { log, ipc });

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
