use super::{action_client, action_request, submit_action};
use crate::api::{ApiError, ApiState};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use darpc_protocol::{
    CommandKind as ProtocolKind, MAX_RAW_PACKET_PAYLOAD_LEN, RawPacket,
    RawPacketDirection as ProtocolDirection,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawDirection {
    Client,
    Server,
}

impl From<RawDirection> for ProtocolDirection {
    fn from(value: RawDirection) -> Self {
        match value {
            RawDirection::Client => Self::Client,
            RawDirection::Server => Self::Server,
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSendOptions {
    /// `client` submits a packet to the game server; `server` dispatches a
    /// synthetic server packet inside the game client.
    pub(crate) direction: RawDirection,
    /// One command byte as exactly two hexadecimal digits, with an optional
    /// `0x` prefix.
    #[schema(example = "7E")]
    pub(crate) command: String,
    /// Up to 255 space-separated hexadecimal bytes, or an empty string.
    #[schema(example = "00 03 02")]
    pub(crate) payload: String,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/raw/send",
    summary = "Send a raw client or server packet",
    description = "Low-level escape hatch for protocol research. Malformed packets can disconnect sessions, corrupt client state, or crash the game client or server.",
    params(("client" = String, Path)),
    request_body = RawSendOptions,
    responses(
        (status = 200, body = super::CommandStatus),
        (status = 202, body = super::CommandStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState),
        (status = 429, body = crate::api::ErrorState),
        (status = 503, body = crate::api::ErrorState),
        (status = 504, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn send(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<RawSendOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<super::CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    let command = parse_command(&request.command)
        .map_err(|message| bad_request(pid, format!("invalid command: {message}")))?;
    let payload = parse_payload(&request.payload)
        .map_err(|message| bad_request(pid, format!("invalid payload: {message}")))?;
    let packet = RawPacket::new(request.direction.into(), command, &payload)
        .ok_or_else(|| bad_request(pid, "raw packet payload is too large"))?;
    submit_action(&state, pid, identity, ProtocolKind::Raw(packet)).await
}

fn parse_command(value: &str) -> Result<u8, &'static str> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("expected exactly two hexadecimal digits, optionally prefixed by 0x");
    }
    u8::from_str_radix(digits, 16).map_err(|_| "command is not a byte")
}

fn parse_payload(value: &str) -> Result<Vec<u8>, String> {
    let mut payload = Vec::new();
    for token in value.split_ascii_whitespace() {
        if token.len() != 2 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("`{token}` is not exactly two hexadecimal digits"));
        }
        if payload.len() == MAX_RAW_PACKET_PAYLOAD_LEN {
            return Err(format!(
                "payload exceeds the {MAX_RAW_PACKET_PAYLOAD_LEN}-byte limit"
            ));
        }
        payload.push(
            u8::from_str_radix(token, 16)
                .map_err(|_| format!("`{token}` is not a hexadecimal byte"))?,
        );
    }
    Ok(payload)
}

fn bad_request(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_raw_packet",
        message,
        Some(pid),
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_command, parse_payload};
    use darpc_protocol::MAX_RAW_PACKET_PAYLOAD_LEN;

    #[test]
    fn parses_explicit_hex_command_and_payload() {
        assert_eq!(parse_command("0x7E"), Ok(0x7e));
        assert_eq!(parse_command("0X00"), Ok(0));
        assert_eq!(parse_command("7E"), Ok(0x7e));
        assert_eq!(parse_command("00"), Ok(0));
        assert_eq!(parse_payload("00 03 02").unwrap(), [0x00, 0x03, 0x02]);
        assert!(parse_payload("").unwrap().is_empty());
    }

    #[test]
    fn rejects_ambiguous_or_oversized_hex() {
        for command in ["7", "777", "0x7", "0x7EE", "0xGG", " 7E", "7E "] {
            assert!(parse_command(command).is_err());
        }
        for payload in ["0", "0003", "0x03", "GG", "03,"] {
            assert!(parse_payload(payload).is_err());
        }
        let oversized = std::iter::repeat_n("00", MAX_RAW_PACKET_PAYLOAD_LEN + 1)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(parse_payload(&oversized).is_err());
    }
}
