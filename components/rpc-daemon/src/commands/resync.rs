use super::{action_client, submit_action};
use crate::api::{ApiError, ApiState};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use darpc_protocol::CommandKind as ProtocolKind;

#[utoipa::path(
    post,
    path = "/clients/{client}/resync",
    summary = "Resynchronize client state",
    description = "Submits the same opcode-only 0x38 client refresh packet as the F5 key. The response resync_id correlates client.resync and client.resync_completed SSE events; HTTP completion confirms command submission only.",
    params(("client" = String, Path)),
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
pub(crate) async fn resync(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<super::CommandStatus>), ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    submit_action(&state, pid, identity, ProtocolKind::Resync).await
}
