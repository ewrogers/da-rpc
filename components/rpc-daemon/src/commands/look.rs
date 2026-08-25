use super::*;
use darpc_protocol::{CommandKind as ProtocolKind, LookTarget};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FarLookOptions {
    position: Destination,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/look",
    summary = "Look at the tile ahead",
    description = "Submits the native 0x09 client Look packet and completes asynchronously when its popup response is emitted through the event stream.",
    params(("client" = String, Path)),
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
pub(crate) async fn look(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    submit_action(&state, pid, identity, ProtocolKind::Look(LookTarget::Ahead)).await
}

#[utoipa::path(
    post,
    path = "/clients/{client}/far-look",
    summary = "Look at a remote map tile",
    description = "Submits the native 0x0A client FarLook packet for one current-map tile. The popup response is suppressed in the game client and emitted through the event stream.",
    params(("client" = String, Path)),
    request_body = FarLookOptions,
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
pub(crate) async fn far_look(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<FarLookOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    validate_destination(pid, &snapshot, request.position)?;
    let x = u16::try_from(request.position.x)
        .map_err(|_| invalid_destination(pid, "position x exceeds the FarLook wire range"))?;
    let y = u16::try_from(request.position.y)
        .map_err(|_| invalid_destination(pid, "position y exceeds the FarLook wire range"))?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Look(LookTarget::Tile { x, y }),
    )
    .await
}

fn invalid_destination(pid: u32, message: &'static str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_destination",
        message,
        Some(pid),
    )
}
