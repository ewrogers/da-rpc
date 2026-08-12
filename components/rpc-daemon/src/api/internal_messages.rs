use super::*;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InternalMessageChannel {
    Internal,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InternalMessageOptions {
    channel: InternalMessageChannel,
    recipient: Option<String>,
    content: Option<String>,
    #[schema(value_type = Option<Object>)]
    payload: Option<Map<String, Value>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct InternalMessageResult {
    delivered: usize,
}

#[utoipa::path(
    post,
    path = "/messages/send",
    request_body(
        content = InternalMessageOptions,
        description = "Daemon-only message for one named daRPC client or every connected daRPC client",
        content_type = "application/json",
        example = json!({"channel": "internal", "recipient": "Eidolon", "payload": {"action": "ready"}})
    ),
    responses(
        (status = 200, description = "The internal message was delivered; broadcasts may deliver to zero clients", body = InternalMessageResult),
        (status = 400, description = "The request did not contain exactly one of content or payload", body = ErrorState),
        (status = 404, description = "The named recipient was not found", body = ErrorState),
        (status = 409, description = "More than one connected client has the recipient name", body = ErrorState),
        (status = 413, description = "The request body exceeded 4 KiB", body = ErrorState)
    )
)]
pub(super) async fn send(
    State(state): State<ApiState>,
    request: Result<Json<InternalMessageOptions>, JsonRejection>,
) -> Result<Json<InternalMessageResult>, ApiError> {
    let Json(request) = request.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;

    let payload = match (request.content, request.payload) {
        (Some(content), None) if !content.is_empty() => {
            Map::from_iter([("content".into(), Value::String(content))])
        }
        (None, Some(payload)) => payload,
        (Some(_), None) => {
            return Err(invalid_message("content must not be empty"));
        }
        _ => {
            return Err(invalid_message(
                "exactly one of content or payload must be provided",
            ));
        }
    };

    let registry = state.snapshot();
    let (identities, recipient) = match request.recipient {
        Some(recipient) => resolve_recipient(&registry, &recipient)?,
        None => (
            registry
                .clients
                .iter()
                .filter(|client| client.status == ClientSnapshotStatus::Connected)
                .filter_map(|client| client.identity)
                .collect(),
            None,
        ),
    };
    let delivered = identities.len();
    state.publish_internal_message(identities, recipient, payload);
    Ok(Json(InternalMessageResult { delivered }))
}

fn resolve_recipient(
    registry: &RegistrySnapshot,
    recipient: &str,
) -> Result<(Vec<RegistryClientIdentity>, Option<String>), ApiError> {
    if recipient.is_empty() {
        return Err(invalid_message("recipient must not be empty"));
    }
    let mut matches = registry.clients.iter().filter_map(|client| {
        let name = current_character_name(client)?;
        name.eq_ignore_ascii_case(recipient)
            .then_some((client.identity?, name.to_owned()))
    });
    let Some((identity, canonical_name)) = matches.next() else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "recipient_not_found",
            format!("no connected in-game client is named {recipient:?}"),
            None,
        ));
    };
    if matches.next().is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ambiguous_recipient",
            format!("more than one connected in-game client is named {recipient:?}"),
            None,
        ));
    }
    Ok((vec![identity], Some(canonical_name)))
}

fn invalid_message(message: &'static str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_internal_message",
        message,
        None,
    )
}
