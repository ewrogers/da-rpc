//! Local test host for validating the `darpc.dll` lifecycle.

use std::process::ExitCode;

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
fn run() -> std::io::Result<()> {
    Ok(())
}
