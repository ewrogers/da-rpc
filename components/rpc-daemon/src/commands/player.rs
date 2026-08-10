use super::*;
use std::time::Instant;

const INSPECT_TIMEOUT: Duration = Duration::from_secs(3);
const ROUTE_WAIT: Duration = Duration::from_millis(1_250);

/// Refreshes one visible player's cached equipment, identity, group state, and legend.
#[utoipa::path(
    post,
    path = "/clients/{client}/players/{player}/inspect",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("player" = String, Path, description = "Case-insensitive visible player name")
    ),
    responses(
        (status = 200, description = "The refreshed complete visible player", body = crate::state::WorldObject),
        (status = 404, description = "The client or visible player was not found", body = crate::api::ErrorState),
        (status = 409, description = "The target is ambiguous or the client is not in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The server did not return object info within three seconds", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn inspect_player(
    State(state): State<ApiState>,
    Path((identifier, player_name)): Path<(String, String)>,
) -> Result<Json<crate::state::WorldObject>, ApiError> {
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let mut matches = snapshot
        .objects
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|object| {
            matches!(
                object,
                WorldObject::Player { name: Some(name), .. }
                    if name.eq_ignore_ascii_case(&player_name)
            )
        });
    let player = matches.next().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "visible_player_not_found",
            format!("no visible player is named {player_name:?}"),
            Some(pid),
        )
    })?;
    if matches.next().is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ambiguous_visible_player",
            format!("more than one visible player is named {player_name:?}"),
            Some(pid),
        ));
    }
    let id = NonZeroU32::new(player.id()).ok_or_else(|| unavailable(pid))?;
    let player = request(&state, pid, identity, id).await?;
    Ok(Json(crate::state::WorldObject::from(&player)))
}

async fn request(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    id: NonZeroU32,
) -> Result<WorldObject, ApiError> {
    let deadline = Instant::now() + INSPECT_TIMEOUT;
    let mut result = route(
        state,
        pid,
        identity,
        CommandOperation::Submit {
            kind: ProtocolKind::InspectPlayer(id),
            timeout_ms: INSPECT_TIMEOUT.as_millis() as u16,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
    .await?;
    loop {
        match result {
            ProtocolResult::Player { player, .. } if player.id() == id.get() => return Ok(*player),
            ProtocolResult::Status(status) if status.state == ProtocolState::Accepted => {
                if Instant::now() >= deadline {
                    return Err(inspect_timeout(pid));
                }
                result = route(
                    state,
                    pid,
                    identity,
                    CommandOperation::Query {
                        command_id: status.command_id,
                        wait_ms: MAX_COMMAND_WAIT_MS,
                    },
                )
                .await?;
            }
            ProtocolResult::Status(status) if status.state == ProtocolState::TimedOut => {
                return Err(inspect_timeout(pid));
            }
            ProtocolResult::Status(status) => {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "player_inspection_failed",
                    format!(
                        "player inspection ended in state {:?} with failure {:?}",
                        status.state, status.failure
                    ),
                    Some(pid),
                ));
            }
            ProtocolResult::Busy => {
                return Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "command_queue_full",
                    "the bounded command queue is full",
                    Some(pid),
                ));
            }
            ProtocolResult::NotFound => return Err(inspect_timeout(pid)),
            ProtocolResult::Unavailable
            | ProtocolResult::Who { .. }
            | ProtocolResult::Legend { .. }
            | ProtocolResult::Player { .. } => return Err(unavailable(pid)),
        }
    }
}

async fn route(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    operation: CommandOperation,
) -> Result<ProtocolResult, ApiError> {
    let receiver = state.route_command(pid, identity, operation)?;
    let reply = timeout(ROUTE_WAIT, receiver)
        .await
        .map_err(|_| inspect_timeout(pid))?
        .map_err(|_| unavailable(pid))?;
    match reply {
        CommandReply::Result(result) => Ok(result),
        CommandReply::Busy => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "command_router_full",
            "the bounded daemon command router is full",
            Some(pid),
        )),
        CommandReply::Unavailable => Err(unavailable(pid)),
    }
}

fn inspect_timeout(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "player_inspection_timeout",
        "the server did not return player object info within three seconds",
        Some(pid),
    )
}
