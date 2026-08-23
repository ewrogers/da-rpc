use super::{action_client, submit_action};
use crate::{
    api::{ApiError, ApiState},
    registry::hex,
    resync_status::ResyncSchedulerStatus,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use darpc_protocol::CommandKind as ProtocolKind;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ResyncRequestStatus {
    pid: u32,
    instance_id: String,
    resync_id: u32,
    coalesced: bool,
    resync: ResyncSchedulerStatus,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/resync",
    summary = "Resynchronize client state",
    description = "Requests the same opcode-only 0x38 client refresh as the F5 key. If a refresh is active, this request joins it and returns the same resync_id without sending another packet. The resync_id correlates client.resync with client.resync_completed. HTTP completion confirms submission or coalescing only.",
    params(("client" = String, Path)),
    responses(
        (status = 200, body = ResyncRequestStatus),
        (status = 202, body = ResyncRequestStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState),
        (status = 429, description = "The command queue is full", body = crate::api::ErrorState),
        (status = 503, body = crate::api::ErrorState),
        (status = 504, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn resync(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<ResyncRequestStatus>), ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    let request_lock = state.resync_request_lock(identity);
    let _guard = request_lock.lock().await;

    let current = state.resync_status(identity);
    if let Some(resync_id) = current.active_resync_id {
        return Ok((
            StatusCode::ACCEPTED,
            Json(ResyncRequestStatus {
                pid,
                instance_id: hex(&identity.dll_instance_id),
                resync_id,
                coalesced: true,
                resync: current,
            }),
        ));
    }

    let (status_code, Json(status)) =
        submit_action(&state, pid, identity, ProtocolKind::Resync).await?;
    Ok((
        status_code,
        Json(ResyncRequestStatus {
            pid: status.pid,
            instance_id: status.instance_id,
            resync_id: status
                .resync_id
                .expect("resync command status has a correlation ID"),
            coalesced: false,
            resync: status
                .resync
                .expect("resync command status has scheduler state"),
        }),
    ))
}
