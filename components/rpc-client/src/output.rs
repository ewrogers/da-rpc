use crate::{command_output, error::ClientError, snapshot_output};
use darpc_model::ClientSnapshot;
use darpc_protocol::{
    Architecture, CommandResult as ProtocolCommandResult, Hello, protocol_version_major,
    protocol_version_minor,
};
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum CommandResult {
    Hello {
        requested_pid: u32,
        hello: Hello,
        selected_version: u16,
        sequence: u16,
        sender_tick_ms: u32,
    },
    Ping {
        pid: u32,
        request_id: u32,
        request_sequence: u16,
        response_sequence: u16,
        request_tick_ms: u32,
        response_tick_ms: u32,
        round_trip_ms: u32,
    },
    TickHealth {
        pid: u32,
        installed: bool,
        relocated_bytes: u8,
        first_tick_count: u32,
        tick_count: u32,
        tick_delta: u32,
        sample_ms: u32,
    },
    Snapshot {
        pid: u32,
        request_id: u32,
        snapshot: Box<ClientSnapshot>,
        round_trip_ms: u32,
    },
    Echo {
        pid: u32,
        request_id: u32,
        text: String,
        round_trip_ms: u32,
    },
    Command {
        pid: u32,
        action: &'static str,
        request_id: u32,
        result: ProtocolCommandResult,
        round_trip_ms: u32,
    },
}

impl CommandResult {
    pub(crate) fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Table => self.render_human(),
            OutputFormat::Json => self.render_json(),
        }
    }

    fn render_human(&self) -> String {
        match self {
            Self::Hello {
                requested_pid,
                hello,
                selected_version,
                sequence,
                sender_tick_ms,
            } => format!(
                concat!(
                    "hello succeeded: pid={} protocol={} instance={} creation_time={} ",
                    "architecture={} dll_version={}.{}.{} fingerprint={} client_version={} ",
                    "sequence={} sender_tick_ms={}"
                ),
                requested_pid,
                protocol_version(*selected_version),
                hex(&hello.dll_instance_id),
                hello.process_creation_time,
                architecture(hello.architecture),
                hello.dll_version.major,
                hello.dll_version.minor,
                hello.dll_version.patch,
                hex(&hello.executable_fingerprint),
                hello.client_version,
                sequence,
                sender_tick_ms,
            ),
            Self::Ping {
                pid,
                request_id,
                request_sequence,
                response_sequence,
                request_tick_ms,
                response_tick_ms,
                round_trip_ms,
            } => format!(
                concat!(
                    "ping succeeded: pid={} request_id={} round_trip_ms={} ",
                    "request_sequence={} response_sequence={} request_tick_ms={} ",
                    "response_tick_ms={}"
                ),
                pid,
                request_id,
                round_trip_ms,
                request_sequence,
                response_sequence,
                request_tick_ms,
                response_tick_ms,
            ),
            Self::TickHealth {
                pid,
                installed,
                relocated_bytes,
                first_tick_count,
                tick_count,
                tick_delta,
                sample_ms,
            } => format!(
                concat!(
                    "tick-health succeeded: pid={} installed={} advancing={} ",
                    "relocated_bytes={} first_ticks={} ticks={} delta_ticks={} sample_ms={}"
                ),
                pid,
                installed,
                *installed && *tick_delta != 0,
                relocated_bytes,
                first_tick_count,
                tick_count,
                tick_delta,
                sample_ms,
            ),
            Self::Snapshot {
                pid,
                request_id,
                snapshot,
                round_trip_ms,
            } => snapshot_output::render_human(*pid, *request_id, *round_trip_ms, snapshot),
            Self::Echo {
                pid,
                request_id,
                text,
                round_trip_ms,
            } => format!(
                "echo succeeded: pid={pid} request_id={request_id} bytes={} round_trip_ms={round_trip_ms} text={}",
                text.len(),
                json_string(text)
            ),
            Self::Command {
                pid,
                action,
                request_id,
                result,
                round_trip_ms,
            } => command_output::render_human(action, *pid, *request_id, *round_trip_ms, *result),
        }
    }

    fn render_json(&self) -> String {
        match self {
            Self::Hello {
                requested_pid,
                hello,
                selected_version,
                sequence,
                sender_tick_ms,
            } => format!(
                concat!(
                    "{{\"ok\":true,\"command\":\"hello\",\"pid\":{},",
                    "\"protocol_version\":{},\"dll_instance_id\":{},",
                    "\"process_creation_time\":{},\"architecture\":{},",
                    "\"dll_version\":{},\"executable_fingerprint\":{},",
                    "\"client_version\":{},\"sequence\":{},\"sender_tick_ms\":{}}}"
                ),
                requested_pid,
                json_string(&protocol_version(*selected_version)),
                json_string(&hex(&hello.dll_instance_id)),
                json_string(&hello.process_creation_time.to_string()),
                json_string(architecture(hello.architecture)),
                json_string(&format!(
                    "{}.{}.{}",
                    hello.dll_version.major, hello.dll_version.minor, hello.dll_version.patch
                )),
                json_string(&hex(&hello.executable_fingerprint)),
                hello.client_version,
                sequence,
                sender_tick_ms,
            ),
            Self::Ping {
                pid,
                request_id,
                request_sequence,
                response_sequence,
                request_tick_ms,
                response_tick_ms,
                round_trip_ms,
            } => format!(
                concat!(
                    "{{\"ok\":true,\"command\":\"ping\",\"pid\":{},",
                    "\"request_id\":{},\"round_trip_ms\":{},",
                    "\"request_sequence\":{},\"response_sequence\":{},",
                    "\"request_tick_ms\":{},\"response_tick_ms\":{}}}"
                ),
                pid,
                request_id,
                round_trip_ms,
                request_sequence,
                response_sequence,
                request_tick_ms,
                response_tick_ms,
            ),
            Self::TickHealth {
                pid,
                installed,
                relocated_bytes,
                first_tick_count,
                tick_count,
                tick_delta,
                sample_ms,
            } => format!(
                concat!(
                    "{{\"ok\":true,\"command\":\"tick-health\",\"pid\":{},",
                    "\"installed\":{},\"advancing\":{},\"relocated_bytes\":{},",
                    "\"first_tick_count\":{},\"tick_count\":{},\"tick_delta\":{},",
                    "\"sample_ms\":{}}}"
                ),
                pid,
                installed,
                *installed && *tick_delta != 0,
                relocated_bytes,
                first_tick_count,
                tick_count,
                tick_delta,
                sample_ms,
            ),
            Self::Snapshot {
                pid,
                request_id,
                snapshot,
                round_trip_ms,
            } => snapshot_output::render_json(*pid, *request_id, *round_trip_ms, snapshot),
            Self::Echo {
                pid,
                request_id,
                text,
                round_trip_ms,
            } => format!(
                concat!(
                    "{{\"ok\":true,\"command\":\"echo\",\"pid\":{},",
                    "\"request_id\":{},\"bytes\":{},\"round_trip_ms\":{},",
                    "\"text\":{}}}"
                ),
                pid,
                request_id,
                text.len(),
                round_trip_ms,
                json_string(text),
            ),
            Self::Command {
                pid,
                action,
                request_id,
                result,
                round_trip_ms,
            } => command_output::render_json(action, *pid, *request_id, *round_trip_ms, *result),
        }
    }
}

pub(crate) fn render_error(
    format: OutputFormat,
    command: Option<&str>,
    error: &ClientError,
) -> String {
    match format {
        OutputFormat::Table => format!("darpc: {error}"),
        OutputFormat::Json => format!(
            concat!(
                "{{\"ok\":false,\"command\":{},\"pid\":{},",
                "\"error\":{{\"kind\":{},\"message\":{}}}}}"
            ),
            command.map_or_else(|| "null".into(), json_string),
            error
                .pid()
                .map_or_else(|| "null".into(), |pid| pid.to_string()),
            json_string(error.kind().as_str()),
            json_string(error.message()),
        ),
    }
}

fn protocol_version(version: u16) -> String {
    format!(
        "{}.{}",
        protocol_version_major(version),
        protocol_version_minor(version)
    )
}

fn architecture(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X86 => "x86",
        Architecture::X86_64 => "x86_64",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1F}' => {
                write!(output, "\\u{:04X}", character as u32)
                    .expect("writing to String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::{CommandResult, OutputFormat, hex};

    #[test]
    fn hexadecimal_identifiers_are_lowercase() {
        assert_eq!(hex(&[0x01, 0xAB, 0xCD, 0xEF]), "01abcdef");
    }

    #[test]
    fn echo_json_has_a_stable_shape_and_escaping() {
        let result = CommandResult::Echo {
            pid: 42,
            request_id: 1,
            text: "quote\" line\n".into(),
            round_trip_ms: 3,
        };
        assert_eq!(
            result.render(OutputFormat::Json),
            concat!(
                "{\"ok\":true,\"command\":\"echo\",\"pid\":42,",
                "\"request_id\":1,\"bytes\":12,\"round_trip_ms\":3,",
                "\"text\":\"quote\\\" line\\n\"}"
            )
        );
    }

    #[test]
    fn tick_health_json_reports_progress() {
        let result = CommandResult::TickHealth {
            pid: 42,
            installed: true,
            relocated_bytes: 5,
            first_tick_count: u32::MAX - 1,
            tick_count: 3,
            tick_delta: 5,
            sample_ms: 250,
        };
        assert_eq!(
            result.render(OutputFormat::Json),
            concat!(
                "{\"ok\":true,\"command\":\"tick-health\",\"pid\":42,",
                "\"installed\":true,\"advancing\":true,\"relocated_bytes\":5,",
                "\"first_tick_count\":4294967294,\"tick_count\":3,",
                "\"tick_delta\":5,\"sample_ms\":250}"
            )
        );
    }
}
