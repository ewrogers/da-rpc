use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionDirection {
    North,
    East,
    South,
    West,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TurnOptions {
    direction: ActionDirection,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkDirectionOptions {
    direction: ActionDirection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Destination {
    pub(super) x: i32,
    pub(super) y: i32,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkDestinationOptions {
    destination: Destination,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum WalkOptions {
    Direction(WalkDirectionOptions),
    Destination(WalkDestinationOptions),
}

#[utoipa::path(
    post,
    path = "/clients/{client}/turn",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body = TurnOptions,
    responses(
        (status = 200, description = "The turn command completed", body = CommandStatus),
        (status = 202, description = "The turn command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The direction was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not currently in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn turn(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<TurnOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Turn(request.direction.into()),
    )
    .await
}

#[utoipa::path(
    post,
    path = "/clients/{client}/walk",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body = WalkOptions,
    responses(
        (status = 200, description = "The walk command completed; a valid unreachable tile reports failure `no_path`", body = CommandStatus),
        (status = 202, description = "The walk command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The direction, body shape, or zero-based destination was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game or its map is unavailable", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn walk(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<WalkOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let target = match request {
        WalkOptions::Direction(options) => WalkTarget::Direction(options.direction.into()),
        WalkOptions::Destination(options) => {
            validate_destination(pid, &snapshot, options.destination)?;
            WalkTarget::Destination {
                x: options.destination.x,
                y: options.destination.y,
            }
        }
    };
    submit_action(&state, pid, identity, ProtocolKind::Walk(target)).await
}

pub(super) fn validate_destination(
    pid: u32,
    snapshot: &GameSnapshot,
    destination: Destination,
) -> Result<(), ApiError> {
    let location = snapshot
        .character
        .as_ref()
        .and_then(|character| character.location.as_ref())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "map_unavailable",
                "the client's current map is unavailable",
                Some(pid),
            )
        })?;
    if destination.x < 0
        || destination.y < 0
        || destination.x >= location.width
        || destination.y >= location.height
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_destination",
            format!(
                "destination must satisfy 0 <= x < {} and 0 <= y < {}",
                location.width, location.height
            ),
            Some(pid),
        ));
    }
    Ok(())
}

impl From<ActionDirection> for ModelDirection {
    fn from(direction: ActionDirection) -> Self {
        match direction {
            ActionDirection::North => Self::North,
            ActionDirection::East => Self::East,
            ActionDirection::South => Self::South,
            ActionDirection::West => Self::West,
        }
    }
}
