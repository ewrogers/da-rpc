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
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkDestinationOptions {
    destination: Destination,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteOptions {
    map_id: u32,
    tiles: Vec<Destination>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkRouteOptions {
    route: RouteOptions,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapExclusionsOptions {
    tiles: Vec<Destination>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct MapPathExclusions {
    map_id: u32,
    tiles: Vec<Destination>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct MapExclusionsSummary {
    map_id: u32,
    tile_count: u16,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct MapExclusionsCollection {
    maps: Vec<MapExclusionsSummary>,
    total_tiles: u32,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum WalkOptions {
    Direction(WalkDirectionOptions),
    Destination(WalkDestinationOptions),
    Route(WalkRouteOptions),
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
        WalkOptions::Route(options) => {
            let tiles = validate_route(pid, &snapshot, &options.route)?;
            WalkTarget::Route(
                WalkRoute::new(options.route.map_id, &tiles)
                    .expect("validated route fits the protocol bound"),
            )
        }
    };
    submit_action(&state, pid, identity, ProtocolKind::Walk(target)).await
}

#[utoipa::path(
    put,
    path = "/clients/{client}/maps/{map_id}/path-exclusions",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("map_id" = u32, Path, description = "Map identifier")
    ),
    request_body = MapExclusionsOptions,
    responses(
        (status = 200, description = "The map exclusion resource was replaced", body = CommandStatus),
        (status = 202, description = "The replacement command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The map identifier or exclusion tiles were invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn replace_map_exclusions(
    State(state): State<ApiState>,
    Path((identifier, map_id)): Path<(String, u32)>,
    request: Result<Json<MapExclusionsOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let tiles = validate_map_exclusions(pid, map_id, &request.tiles)?;
    validate_map_exclusion_capacity(pid, &snapshot, map_id, tiles.len())?;
    let exclusions =
        PathExclusions::new(map_id, &tiles).expect("validated exclusions fit the protocol bound");
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::SetPathExclusions(exclusions),
    )
    .await
}

#[utoipa::path(
    get,
    path = "/clients/{client}/maps/{map_id}/path-exclusions",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("map_id" = u32, Path, description = "Map identifier")
    ),
    responses(
        (status = 200, description = "The configured exclusions for this map", body = MapPathExclusions),
        (status = 400, description = "The map identifier was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client or map exclusion resource was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client state is unavailable", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn map_exclusions(
    State(state): State<ApiState>,
    Path((identifier, map_id)): Path<(String, u32)>,
) -> Result<Json<MapPathExclusions>, ApiError> {
    let (pid, _, snapshot) = action_client(&state, &identifier)?;
    validate_exclusion_map_id(pid, map_id)?;
    let exclusions = snapshot
        .map_exclusions
        .binary_search_by_key(&map_id, |entry| entry.map_id)
        .ok()
        .map(|index| &snapshot.map_exclusions[index])
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "map_exclusions_not_found",
                format!("map {map_id} has no configured path exclusions"),
                Some(pid),
            )
        })?;
    Ok(Json(map_exclusions_resource(exclusions)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/maps/path-exclusions",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "All configured map exclusion resources", body = MapExclusionsCollection),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client state is unavailable", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn map_exclusions_collection(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<Json<MapExclusionsCollection>, ApiError> {
    let (_, _, snapshot) = action_client(&state, &identifier)?;
    Ok(Json(MapExclusionsCollection {
        total_tiles: snapshot
            .map_exclusions
            .iter()
            .map(|entry| u32::try_from(entry.tiles.len()).expect("bounded tile count fits u32"))
            .sum(),
        maps: snapshot
            .map_exclusions
            .iter()
            .map(|entry| MapExclusionsSummary {
                map_id: entry.map_id,
                tile_count: u16::try_from(entry.tiles.len())
                    .expect("bounded map tile count fits u16"),
            })
            .collect(),
    }))
}

#[utoipa::path(
    delete,
    path = "/clients/{client}/maps/{map_id}/path-exclusions",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("map_id" = u32, Path, description = "Map identifier")
    ),
    responses(
        (status = 200, description = "The map exclusion resource is absent", body = CommandStatus),
        (status = 202, description = "The removal command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The map identifier was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn remove_map_exclusions(
    State(state): State<ApiState>,
    Path((identifier, map_id)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    validate_exclusion_map_id(pid, map_id)?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::RemovePathExclusions { map_id },
    )
    .await
}

#[utoipa::path(
    delete,
    path = "/clients/{client}/maps/path-exclusions",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The map exclusion registry is empty", body = CommandStatus),
        (status = 202, description = "The clear command was accepted and remains pending", body = CommandStatus),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn clear_map_exclusions(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    submit_action(&state, pid, identity, ProtocolKind::ClearPathExclusions).await
}

fn validate_map_exclusions(
    pid: u32,
    map_id: u32,
    requested: &[Destination],
) -> Result<Vec<RouteTile>, ApiError> {
    validate_exclusion_map_id(pid, map_id)?;
    if requested.is_empty() || requested.len() > MAX_PATH_EXCLUSION_TILES {
        return Err(invalid_path_exclusions(
            pid,
            format!("tile count must be from 1 through {MAX_PATH_EXCLUSION_TILES}"),
        ));
    }
    let coordinate_limit =
        i32::try_from(MAX_PATH_EXCLUSION_DIMENSION).expect("exclusion dimension fits i32");
    let mut tiles = requested
        .iter()
        .map(|tile| {
            if tile.x < 0 || tile.y < 0 || tile.x >= coordinate_limit || tile.y >= coordinate_limit
            {
                return Err(invalid_path_exclusions(
                    pid,
                    format!("tiles must satisfy 0 <= x,y < {MAX_PATH_EXCLUSION_DIMENSION}"),
                ));
            }
            Ok(RouteTile {
                x: u16::try_from(tile.x).expect("validated exclusion x fits u16"),
                y: u16::try_from(tile.y).expect("validated exclusion y fits u16"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    tiles.sort_unstable_by_key(|tile| (tile.y, tile.x));
    tiles.dedup();
    Ok(tiles)
}

fn validate_exclusion_map_id(pid: u32, map_id: u32) -> Result<(), ApiError> {
    if map_id > u32::from(u16::MAX) {
        return Err(invalid_path_exclusions(
            pid,
            format!("map_id must be from 0 through {}", u16::MAX),
        ));
    }
    Ok(())
}

fn validate_map_exclusion_capacity(
    pid: u32,
    snapshot: &GameSnapshot,
    map_id: u32,
    tile_count: usize,
) -> Result<(), ApiError> {
    let previous_count = snapshot
        .map_exclusions
        .binary_search_by_key(&map_id, |entry| entry.map_id)
        .ok()
        .map_or(0, |index| snapshot.map_exclusions[index].tiles.len());
    if previous_count == 0 && snapshot.map_exclusions.len() >= MAX_PATH_EXCLUSION_MAPS {
        return Err(invalid_path_exclusions(
            pid,
            format!("a client session accepts at most {MAX_PATH_EXCLUSION_MAPS} maps"),
        ));
    }
    let total_tiles = snapshot
        .map_exclusions
        .iter()
        .map(|entry| entry.tiles.len())
        .sum::<usize>()
        .checked_sub(previous_count)
        .and_then(|total| total.checked_add(tile_count))
        .ok_or_else(|| invalid_path_exclusions(pid, "path-exclusion tile count overflow"))?;
    if total_tiles > MAX_PATH_EXCLUSION_TOTAL_TILES {
        return Err(invalid_path_exclusions(
            pid,
            format!(
                "a client session accepts at most {MAX_PATH_EXCLUSION_TOTAL_TILES} total tiles"
            ),
        ));
    }
    Ok(())
}

fn invalid_path_exclusions(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_path_exclusions",
        message,
        Some(pid),
    )
}

fn map_exclusions_resource(exclusions: &darpc_model::MapExclusions) -> MapPathExclusions {
    MapPathExclusions {
        map_id: exclusions.map_id,
        tiles: exclusions
            .tiles
            .iter()
            .map(|tile| Destination {
                x: tile.x,
                y: tile.y,
            })
            .collect(),
    }
}

fn validate_route(
    pid: u32,
    snapshot: &GameSnapshot,
    route: &RouteOptions,
) -> Result<Vec<RouteTile>, ApiError> {
    let tiles = validate_tile_list(
        pid,
        snapshot,
        route.map_id,
        &route.tiles,
        MAX_WALK_ROUTE_TILES,
        false,
    )?;
    let location = snapshot
        .character
        .as_ref()
        .and_then(|character| character.location.as_ref())
        .expect("tile-list validation established a current location");
    let current = location.x.zip(location.y).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "position_unavailable",
            "the client's current position is unavailable",
            Some(pid),
        )
    })?;
    if route.tiles.first().map(|tile| (tile.x, tile.y)) != Some(current) {
        return Err(invalid_route(
            pid,
            "the first route tile must equal the client's current position",
        ));
    }
    for edge in route.tiles.windows(2) {
        let dx = (edge[1].x - edge[0].x).unsigned_abs();
        let dy = (edge[1].y - edge[0].y).unsigned_abs();
        if dx + dy != 1 {
            return Err(invalid_route(
                pid,
                "each route edge must move exactly one cardinal tile",
            ));
        }
    }
    for (index, tile) in route.tiles.iter().enumerate() {
        if route.tiles[..index].contains(tile) {
            return Err(invalid_route(pid, "route tiles must not repeat"));
        }
    }
    Ok(tiles)
}

fn validate_tile_list(
    pid: u32,
    snapshot: &GameSnapshot,
    map_id: u32,
    tiles: &[Destination],
    max: usize,
    allow_empty: bool,
) -> Result<Vec<RouteTile>, ApiError> {
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
    if location.id != map_id {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "map_mismatch",
            format!("the client is on map {}, not map {map_id}", location.id),
            Some(pid),
        ));
    }
    if (!allow_empty && tiles.is_empty()) || tiles.len() > max {
        return Err(invalid_route(
            pid,
            format!(
                "tile count must be from {} through {max}",
                usize::from(!allow_empty)
            ),
        ));
    }
    tiles
        .iter()
        .map(|tile| {
            validate_destination(pid, snapshot, *tile)?;
            Ok(RouteTile {
                x: u16::try_from(tile.x).map_err(|_| invalid_route(pid, "tile x is too large"))?,
                y: u16::try_from(tile.y).map_err(|_| invalid_route(pid, "tile y is too large"))?,
            })
        })
        .collect()
}

fn invalid_route(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_route", message, Some(pid))
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
