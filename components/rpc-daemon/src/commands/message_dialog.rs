use super::*;
use darpc_protocol::MessageDialogCommand;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageDialogDismissOptions {
    /// Revision from the current message-dialog state.
    #[schema(example = 7)]
    revision: u32,
    /// Opaque dialog identifier from the current message-dialog state.
    #[schema(example = 3)]
    id: u32,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/message-dialogs/dismiss",
    params(("client" = String, Path)),
    request_body = MessageDialogDismissOptions,
    responses(
        (status = 200, body = CommandStatus),
        (status = 202, body = CommandStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState),
        (status = 429, body = crate::api::ErrorState),
        (status = 503, body = crate::api::ErrorState),
        (status = 504, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn dismiss(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<MessageDialogDismissOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = super::fresh_snapshot(&state, &identifier).await?;
    if snapshot.lifecycle != ClientLifecycle::InGame {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "client_not_in_game",
            "the client is not currently in game",
            Some(pid),
        ));
    }
    let dialogs = &snapshot.message_dialogs;
    if dialogs.revision != request.revision {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "stale_message_dialogs",
            "message dialogs changed after the supplied revision",
            Some(pid),
        ));
    }
    if !dialogs.dialogs.iter().any(|dialog| dialog.id == request.id) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_message_dialog",
            "the dialog ID is not present in the current message-dialog state",
            Some(pid),
        ));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::DismissMessageDialog(MessageDialogCommand {
            revision: request.revision,
            id: request.id,
        }),
    )
    .await
}
