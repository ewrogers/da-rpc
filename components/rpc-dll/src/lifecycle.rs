use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    process,
    sync::Mutex,
};

static LIFECYCLE: Mutex<Option<Lifecycle>> = Mutex::new(None);

struct Lifecycle {
    log: File,
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

    writeln!(
        log,
        "event=initialized pid={} version={}",
        process::id(),
        env!("CARGO_PKG_VERSION")
    )?;

    *lifecycle = Some(Lifecycle { log });

    Ok(())
}

pub(crate) fn shutdown() -> io::Result<()> {
    let mut lifecycle = LIFECYCLE
        .lock()
        .map_err(|_| io::Error::other("lifecycle lock is poisoned"))?;

    let Some(mut lifecycle) = lifecycle.take() else {
        return Ok(());
    };

    writeln!(
        lifecycle.log,
        "event=shutdown pid={} version={}",
        process::id(),
        env!("CARGO_PKG_VERSION")
    )?;

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
