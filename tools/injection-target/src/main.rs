//! Inert process used for safe loader integration testing.

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process, thread,
    time::Duration,
};

fn main() -> io::Result<()> {
    let darpc_loaded_at_start = darpc_is_loaded();
    let initialized_at_start = lifecycle_log_contains_initialization();
    let standard_handles_unavailable_at_start = standard_handles_are_unavailable();
    let options = parse_options()?;

    if let Some(report_path) = options.report_path.as_ref() {
        write_launch_report(
            report_path,
            &options.arguments,
            darpc_loaded_at_start,
            initialized_at_start,
            standard_handles_unavailable_at_start,
        )?;
    }

    let mut stdout = io::stdout();
    if options.report_path.is_none() {
        let _ = writeln!(stdout, "Injection target ready: pid={}", process::id());
        let _ = stdout.flush();
    }

    match options.wait {
        Wait::Input => {
            let _ = write!(stdout, "Press enter to exit...");
            let _ = stdout.flush();

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
        }
        Wait::Duration(duration) => thread::sleep(duration),
    }

    Ok(())
}

struct Options {
    wait: Wait,
    report_path: Option<PathBuf>,
    arguments: Vec<OsString>,
}

enum Wait {
    Input,
    Duration(Duration),
}

fn parse_options() -> io::Result<Options> {
    let mut arguments = env::args_os().skip(1);
    let mut wait = Wait::Input;
    let mut report_path = None;
    let mut forwarded = Vec::new();

    while let Some(option) = arguments.next() {
        if option == "--" {
            forwarded.extend(arguments);
            break;
        }

        if option == "--wait-ms" {
            let milliseconds = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(invalid_arguments)?;
            wait = Wait::Duration(Duration::from_millis(milliseconds));
            continue;
        }

        if option == "--launch-report" {
            report_path = Some(
                arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(invalid_arguments)?,
            );
            continue;
        }

        return Err(invalid_arguments());
    }

    Ok(Options {
        wait,
        report_path,
        arguments: forwarded,
    })
}

fn write_launch_report(
    path: &Path,
    arguments: &[OsString],
    darpc_loaded_at_start: bool,
    initialized_at_start: bool,
    standard_handles_unavailable_at_start: bool,
) -> io::Result<()> {
    let current_directory = env::current_dir()?;
    let mut report = format!(
        "cwd={}\ndarpc_loaded_at_start={darpc_loaded_at_start}\n\
        initialized_at_start={initialized_at_start}\n\
        standard_handles_unavailable_at_start={standard_handles_unavailable_at_start}\n",
        current_directory.display()
    );

    for argument in arguments {
        let argument = argument.to_string_lossy();

        if argument.contains(['\r', '\n']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "launch report arguments cannot contain newlines",
            ));
        }

        report.push_str("arg=");
        report.push_str(&argument);
        report.push('\n');
    }

    fs::write(path, report)
}

fn lifecycle_log_contains_initialization() -> bool {
    let Some(user_profile) = env::var_os("USERPROFILE") else {
        return false;
    };
    let log_path = PathBuf::from(user_profile)
        .join("darpc")
        .join("logs")
        .join(format!("pid-{}.log", process::id()));
    let expected = format!("event=initialized pid={}", process::id());

    fs::read_to_string(log_path)
        .map(|log| log.lines().any(|line| line.starts_with(&expected)))
        .unwrap_or(false)
}

#[cfg(windows)]
fn darpc_is_loaded() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    let module_name: Vec<u16> = std::ffi::OsStr::new("darpc.dll")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: module_name is a live, NUL-terminated UTF-16 string.
    !unsafe { GetModuleHandleW(module_name.as_ptr()) }.is_null()
}

#[cfg(windows)]
fn standard_handles_are_unavailable() -> bool {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };

    [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .into_iter()
        .all(|kind| {
            // SAFETY: kind is one of the three supported standard handle
            // identifiers and GetStdHandle has no pointer parameters.
            let handle = unsafe { GetStdHandle(kind) };
            handle.is_null() || handle == INVALID_HANDLE_VALUE
        })
}

#[cfg(not(windows))]
fn darpc_is_loaded() -> bool {
    false
}

#[cfg(not(windows))]
fn standard_handles_are_unavailable() -> bool {
    true
}

fn invalid_arguments() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        concat!(
            "usage: injection-target [--wait-ms <milliseconds>] ",
            "[--launch-report <path>] [-- <argument>...]"
        ),
    )
}
