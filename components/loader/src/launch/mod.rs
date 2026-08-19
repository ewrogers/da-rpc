use crate::{
    error::{ErrorKind, LoaderError, Result},
    patch::LaunchPatches,
    pe::DarpcDll,
    process::ProcessInspection,
};
use darpc_win32::lifecycle::InitializeOptions;
use std::{ffi::OsString, path::Path};

pub(crate) struct LaunchOutcome {
    pub(crate) pid: u32,
    pub(crate) inspection: ProcessInspection,
    pub(crate) changed: bool,
}

#[cfg(windows)]
mod platform;

#[cfg(any(windows, test))]
fn normalize_windows_launch_path(path: &[u16]) -> Vec<u16> {
    const BACKSLASH: u16 = b'\\' as u16;
    const COLON: u16 = b':' as u16;
    const VERBATIM_PREFIX: &[u16] = &[BACKSLASH, BACKSLASH, b'?' as u16, BACKSLASH];
    const UNC_PREFIX: &[u16] = &[b'U' as u16, b'N' as u16, b'C' as u16, BACKSLASH];

    let Some(remainder) = path.strip_prefix(VERBATIM_PREFIX) else {
        return path.to_vec();
    };

    if let Some(unc_path) = remainder.strip_prefix(UNC_PREFIX) {
        let mut normalized = Vec::with_capacity(unc_path.len() + 2);
        normalized.extend_from_slice(&[BACKSLASH, BACKSLASH]);
        normalized.extend_from_slice(unc_path);
        return normalized;
    }

    if matches!(remainder, [drive, COLON, BACKSLASH, ..] if
        (*drive >= b'A' as u16 && *drive <= b'Z' as u16)
            || (*drive >= b'a' as u16 && *drive <= b'z' as u16))
    {
        return remainder.to_vec();
    }

    path.to_vec()
}

#[cfg(any(windows, test))]
fn append_argument(output: &mut Vec<u16>, argument: &[u16]) -> Result<()> {
    const BACKSLASH: u16 = b'\\' as u16;
    const QUOTE: u16 = b'"' as u16;
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;

    if argument.contains(&0) {
        return Err(LoaderError::new(
            ErrorKind::InvalidArguments,
            "process argument contains a NUL character",
        ));
    }

    let requires_quotes = argument.is_empty()
        || argument
            .iter()
            .any(|code_unit| matches!(*code_unit, SPACE | TAB | QUOTE));

    if !requires_quotes {
        output.extend_from_slice(argument);
        return Ok(());
    }

    output.push(QUOTE);
    let mut backslashes = 0;

    for &code_unit in argument {
        if code_unit == BACKSLASH {
            backslashes += 1;
            continue;
        }

        if code_unit == QUOTE {
            output.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
        } else {
            output.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        }

        backslashes = 0;
        output.push(code_unit);
    }

    output.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    output.push(QUOTE);
    Ok(())
}

pub(crate) fn launch(
    executable_path: &Path,
    arguments: &[OsString],
    dll: &DarpcDll,
    patches: LaunchPatches,
    apply_default_patches: bool,
    initialize_options: InitializeOptions,
) -> Result<LaunchOutcome> {
    #[cfg(windows)]
    {
        platform::run(
            executable_path,
            arguments,
            dll,
            patches,
            apply_default_patches,
            initialize_options,
        )
    }

    #[cfg(not(windows))]
    {
        let _ = (
            executable_path,
            arguments,
            dll,
            patches,
            apply_default_patches,
            initialize_options,
        );
        Err(LoaderError::new(
            ErrorKind::UnsupportedPlatform,
            "loader requires Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{append_argument, normalize_windows_launch_path};

    fn render(value: &str) -> String {
        let mut output = Vec::new();
        append_argument(&mut output, &value.encode_utf16().collect::<Vec<_>>())
            .expect("test argument should be valid");
        String::from_utf16(&output).expect("quoted argument should be valid UTF-16")
    }

    fn normalize(value: &str) -> String {
        String::from_utf16(&normalize_windows_launch_path(
            &value.encode_utf16().collect::<Vec<_>>(),
        ))
        .expect("normalized path should remain UTF-16")
    }

    #[test]
    fn normalizes_verbatim_drive_paths_for_client_launch() {
        assert_eq!(
            normalize(r"\\?\C:\Dark Ages\Darkages.exe"),
            r"C:\Dark Ages\Darkages.exe"
        );
    }

    #[test]
    fn normalizes_verbatim_unc_paths_for_client_launch() {
        assert_eq!(
            normalize(r"\\?\UNC\server\share\Darkages.exe"),
            r"\\server\share\Darkages.exe"
        );
    }

    #[test]
    fn preserves_conventional_and_non_file_device_paths() {
        assert_eq!(
            normalize(r"C:\Dark Ages\Darkages.exe"),
            r"C:\Dark Ages\Darkages.exe"
        );
        assert_eq!(
            normalize(r"\\?\Volume{01234567}\Darkages.exe"),
            r"\\?\Volume{01234567}\Darkages.exe"
        );
    }

    #[test]
    fn quotes_windows_arguments_only_when_required() {
        assert_eq!(render(""), "\"\"");
        assert_eq!(render("plain"), "plain");
        assert_eq!(render("two words"), "\"two words\"");
        assert_eq!(render("quote\"value"), "\"quote\\\"value\"");
        assert_eq!(render("trailing\\"), "trailing\\");
        assert_eq!(render("two words\\"), "\"two words\\\\\"");
        assert_eq!(render("two\\\\\"quotes"), "\"two\\\\\\\\\\\"quotes\"");
        assert_eq!(render("雪"), "雪");
    }
}
