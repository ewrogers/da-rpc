//! Local test host for validating the `darpc.dll` lifecycle.

use std::process::ExitCode;

#[cfg(windows)]
use std::{env, fs, io};

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("lifecycle-host requires Windows");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lifecycle-host: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn run() -> io::Result<()> {
    let mut arguments = env::args_os().skip(1);

    let dll_path = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: lifecycle-host <path-to-darpc.dll>",
        )
    })?;

    if arguments.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly one DLL path",
        ));
    }

    let dll_path = fs::canonicalize(dll_path)?;

    if !dll_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DLL path is not a file",
        ));
    }

    println!("DLL: {}", dll_path.display());

    Ok(())
}
