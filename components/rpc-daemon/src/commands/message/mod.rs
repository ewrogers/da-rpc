use super::*;
use darpc_protocol::{
    MAX_MESSAGE_CONTENT_LEN, MAX_MESSAGE_RECIPIENT_LEN, MessageCommand, MessageContent,
    MessageRecipient,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SendMessageChannel {
    Say,
    Shout,
    Guild,
    Group,
    Whisper,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendMessageOptions {
    channel: SendMessageChannel,
    /// Required for whisper and rejected for every other channel.
    recipient: Option<String>,
    /// Nonempty ASCII content of at most 100 characters.
    content: String,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/messages/send",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body(
        content = SendMessageOptions,
        example = json!({"channel": "whisper", "recipient": "Eidolon", "content": "hello"})
    ),
    responses(
        (status = 200, description = "The message was submitted on the client main thread", body = CommandStatus),
        (status = 202, description = "The message was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The channel, recipient, or content was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn send(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<SendMessageOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    let content = MessageContent::new(&request.content).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_message_content",
            format!(
                "content must contain from 1 through {MAX_MESSAGE_CONTENT_LEN} ASCII characters"
            ),
            Some(pid),
        )
    })?;
    let message = match (request.channel, request.recipient.as_deref()) {
        (SendMessageChannel::Whisper, Some(recipient)) => {
            let recipient = MessageRecipient::new(recipient).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_message_recipient",
                    format!(
                        "a whisper recipient must contain from 1 through {MAX_MESSAGE_RECIPIENT_LEN} ASCII characters without whitespace"
                    ),
                    Some(pid),
                )
            })?;
            MessageCommand::Whisper { recipient, content }
        }
        (SendMessageChannel::Whisper, None) => {
            return Err(recipient_error(pid, "recipient is required for whisper"));
        }
        (channel, Some(_)) => {
            return Err(recipient_error(
                pid,
                format!("recipient is not allowed for the {channel:?} channel").to_lowercase(),
            ));
        }
        (SendMessageChannel::Say, None) => MessageCommand::Say(content),
        (SendMessageChannel::Shout, None) => MessageCommand::Shout(content),
        (SendMessageChannel::Guild, None) => MessageCommand::Guild(content),
        (SendMessageChannel::Group, None) => MessageCommand::Group(content),
    };
    submit_action(&state, pid, identity, ProtocolKind::Message(message)).await
}

fn recipient_error(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_message_recipient",
        message,
        Some(pid),
    )
}
