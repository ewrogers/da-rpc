use super::*;
use darpc_protocol::FieldMapSelectionCommand;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldMapSelectOptions {
    /// Revision from the active field-map state.
    #[schema(example = 7)]
    revision: u32,
    /// Zero-based destination index from the active field-map state.
    #[schema(example = 1)]
    destination_index: u8,
}

/// Selects a destination from the active native field-map panel.
#[utoipa::path(
    post,
    path = "/clients/{client}/field-map/select",
    params(("client" = String, Path)),
    request_body = FieldMapSelectOptions,
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
pub(crate) async fn select(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<FieldMapSelectOptions>, JsonRejection>,
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
    let field_map = snapshot.active_field_map.as_ref().ok_or_else(|| {
        conflict(
            pid,
            "field_map_unavailable",
            "the native field-map panel is not active",
        )
    })?;
    if field_map.revision != request.revision {
        return Err(conflict(
            pid,
            "stale_field_map",
            "the field map changed after the supplied revision",
        ));
    }
    if field_map.selection.is_some() {
        return Err(conflict(
            pid,
            "field_map_selection_submitted",
            "a destination selection was already submitted",
        ));
    }
    if usize::from(request.destination_index) >= field_map.destinations.len() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_field_map_destination",
            "the destination index is not present on the active field map",
            Some(pid),
        ));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::SelectFieldMapDestination(FieldMapSelectionCommand {
            revision: request.revision,
            destination_index: request.destination_index,
        }),
    )
    .await
}

fn conflict(pid: u32, code: &'static str, message: &str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, code, message, Some(pid))
}
