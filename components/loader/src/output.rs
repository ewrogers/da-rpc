use crate::error::LoaderError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommandResult {
    pub(crate) command: &'static str,
    pub(crate) pid: u32,
    pub(crate) creation_time: u64,
    pub(crate) darpc_loaded: bool,
    pub(crate) module_base: Option<usize>,
    pub(crate) changed: bool,
}

impl CommandResult {
    pub(crate) fn render(self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Human => self.render_human(),
            OutputFormat::Json => self.render_json(),
        }
    }

    fn render_human(self) -> String {
        let module_base = self
            .module_base
            .map_or_else(|| "none".to_owned(), |base| format!("0x{base:08X}"));

        format!(
            "{} succeeded: pid={} creation_time={} changed={} darpc_loaded={} module_base={}",
            self.command,
            self.pid,
            self.creation_time,
            self.changed,
            self.darpc_loaded,
            module_base
        )
    }

    fn render_json(self) -> String {
        let module_base = self
            .module_base
            .map_or_else(|| "null".to_owned(), |base| base.to_string());

        format!(
            concat!(
                "{{\"ok\":true,\"command\":{},\"pid\":{},",
                "\"creation_time\":\"{}\",\"changed\":{},",
                "\"darpc_loaded\":{},\"module_base\":{}}}"
            ),
            json_string(self.command),
            self.pid,
            self.creation_time,
            self.changed,
            self.darpc_loaded,
            module_base
        )
    }
}

pub(crate) fn render_error(
    format: OutputFormat,
    command: Option<&str>,
    error: &LoaderError,
) -> String {
    match format {
        OutputFormat::Human => format!("loader: {error}"),
        OutputFormat::Json => {
            let command = command.map_or_else(|| "null".to_owned(), json_string);

            format!(
                concat!(
                    "{{\"ok\":false,\"command\":{},",
                    "\"error\":{{\"kind\":{},\"message\":{}}}}}"
                ),
                command,
                json_string(error.kind().as_str()),
                json_string(error.message())
            )
        }
    }
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');

    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{00}'..='\u{1F}' => {
                use std::fmt::Write as _;
                write!(output, "\\u{:04X}", u32::from(character))
                    .expect("writing to a String cannot fail");
            }
            _ => output.push(character),
        }
    }

    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::{CommandResult, OutputFormat, json_string, render_error};
    use crate::error::{ErrorKind, LoaderError};

    #[test]
    fn json_string_escapes_control_characters() {
        assert_eq!(
            json_string("quote\" slash\\ line\n tab\t\u{01}"),
            "\"quote\\\" slash\\\\ line\\n tab\\t\\u0001\""
        );
    }

    #[test]
    fn success_json_has_a_stable_shape() {
        let result = CommandResult {
            command: "inspect",
            pid: 42,
            creation_time: 123,
            darpc_loaded: true,
            module_base: Some(0x1020_3040),
            changed: false,
        };

        assert_eq!(
            result.render(OutputFormat::Json),
            concat!(
                "{\"ok\":true,\"command\":\"inspect\",\"pid\":42,",
                "\"creation_time\":\"123\",\"changed\":false,",
                "\"darpc_loaded\":true,\"module_base\":270544960}"
            )
        );
    }

    #[test]
    fn error_json_includes_command_and_error_kind() {
        let error = LoaderError::new(ErrorKind::AlreadyLoaded, "already \"loaded\"");

        assert_eq!(
            render_error(OutputFormat::Json, Some("attach"), &error),
            concat!(
                "{\"ok\":false,\"command\":\"attach\",",
                "\"error\":{\"kind\":\"already_loaded\",",
                "\"message\":\"already \\\"loaded\\\"\"}}"
            )
        );
    }
}
