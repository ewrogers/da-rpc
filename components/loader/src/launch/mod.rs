use crate::{
    error::{ErrorKind, LoaderError, Result},
    patch::LaunchPatches,
    pe::DarpcDll,
    process::ProcessInspection,
};
use std::{ffi::OsString, path::Path};

pub(crate) struct LaunchOutcome {
    pub(crate) pid: u32,
    pub(crate) inspection: ProcessInspection,
    pub(crate) changed: bool,
}

#[cfg(windows)]
mod platform;

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
) -> Result<LaunchOutcome> {
    #[cfg(windows)]
    {
        platform::run(
            executable_path,
            arguments,
            dll,
            patches,
            apply_default_patches,
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
        );
        Err(LoaderError::new(
            ErrorKind::UnsupportedPlatform,
            "loader requires Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::append_argument;

    fn render(value: &str) -> String {
        let mut output = Vec::new();
        append_argument(&mut output, &value.encode_utf16().collect::<Vec<_>>())
            .expect("test argument should be valid");
        String::from_utf16(&output).expect("quoted argument should be valid UTF-16")
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
