use std::{env, io, path::PathBuf, process, sync::Mutex};

static LIFECYCLE: Mutex<Option<Lifecycle>> = Mutex::new(None);

struct Lifecycle;

pub(crate) fn initialize() -> io::Result<()> {
    let mut lifecycle = LIFECYCLE
        .lock()
        .map_err(|_| io::Error::other("lifecycle lock is poisoned"))?;

    if lifecycle.is_some() {
        return Ok(());
    }

    let _log_path = log_path()?;
    *lifecycle = Some(Lifecycle);

    Ok(())
}

pub(crate) fn shutdown() -> io::Result<()> {
    let mut lifecycle = LIFECYCLE
        .lock()
        .map_err(|_| io::Error::other("lifecycle lock is poisoned"))?;

    lifecycle.take();
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
