use super::*;

#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "The daemon HTTP server is available", body = HealthState))
)]
pub(super) async fn health() -> Json<HealthState> {
    Json(HealthState {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[utoipa::path(
    get,
    path = "/clients",
    responses((status = 200, description = "Configured client targets and their current connection state", body = ClientList))
)]
pub(super) async fn clients(State(state): State<ApiState>) -> Json<ClientList> {
    let snapshot = state.snapshot();
    Json(ClientList::from(snapshot.as_ref()))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/status",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest character, map, and lifecycle status", body = GameStatus),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_status(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<GameStatus>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(GameStatus::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/dialog",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The current NPC dialog, or null when none is open", body = DialogSnapshot),
        (status = 400, body = ErrorState),
        (status = 404, body = ErrorState),
        (status = 503, body = ErrorState)
    )
)]
pub(super) async fn client_dialog(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<DialogSnapshot>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(DialogSnapshot::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/group",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The current group roster and open invitations", body = GroupSnapshot),
        (status = 400, body = ErrorState),
        (status = 404, body = ErrorState),
        (status = 503, body = ErrorState)
    )
)]
pub(super) async fn client_group(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<GroupSnapshot>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(GroupSnapshot::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/items",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest inventory observation", body = Inventory),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_items(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Inventory>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Inventory::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/equipment",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest equipment observation", body = Equipment),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_equipment(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Equipment>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Equipment::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/spells",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest spellbook observation", body = Spellbook),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_spells(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Spellbook>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Spellbook::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/skills",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest skillbook observation", body = Skillbook),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_skills(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Skillbook>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Skillbook::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/effects",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest spell effect observation", body = Effects),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_effects(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Effects>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Effects::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/objects",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        WorldObjectQuery
    ),
    responses(
        (status = 200, description = "The latest world objects observed by this client", body = WorldObjects),
        (status = 400, description = "The process identifier or object filter was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
pub(super) async fn client_objects(
    Path(identifier): Path<String>,
    query: Result<Query<WorldObjectQuery>, QueryRejection>,
    State(state): State<ApiState>,
) -> Result<Json<WorldObjects>, ApiError> {
    let Query(query) = query
        .map_err(|rejection| invalid_object_query(format!("invalid object query: {rejection}")))?;
    let kinds = query.into_kinds()?;
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(WorldObjects::from_model(
        pid,
        snapshot,
        kinds.as_deref(),
    )))
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct WorldObjectQuery {
    /// Comma-separated object types: `mundane`, `player`, `monster`, and `item`.
    #[param(example = "npc,player")]
    types: Option<String>,
}

impl WorldObjectQuery {
    fn into_kinds(self) -> Result<Option<Vec<WorldObjectKind>>, ApiError> {
        self.types
            .map(|value| {
                value
                    .split(',')
                    .map(|kind| {
                        kind.parse::<WorldObjectKind>().map_err(|()| {
                            invalid_object_query(format!(
                                "unknown object type `{kind}`; expected mundane, player, monster, or item"
                            ))
                        })
                    })
                    .collect()
            })
            .transpose()
    }
}

pub(super) fn invalid_object_query(message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_object_query",
        message,
        None,
    )
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct MessageQuery {
    /// Comma-separated message channels, such as `say,shout`.
    channels: Option<String>,
    /// Return only messages observed after this ISO 8601 timestamp.
    #[param(format = DateTime)]
    since: Option<String>,
    /// Number of matching messages to skip after newest-first sorting.
    #[param(minimum = 0, default = 0)]
    skip: Option<usize>,
    /// Maximum messages to return. Defaults to 20 and cannot exceed 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    count: Option<usize>,
}

impl MessageQuery {
    fn into_filter(self) -> Result<MessageFilter, ApiError> {
        let channels = self
            .channels
            .map(|value| {
                value
                    .split(',')
                    .map(|channel| {
                        channel.parse::<MessageChannel>().map_err(|()| {
                            invalid_message_query(format!(
                                "unknown message channel `{channel}`; expected say, shout, whisper, guild, group, system, or world"
                            ))
                        })
                    })
                    .collect::<Result<_, _>>()
            })
            .transpose()?;
        let since = self
            .since
            .map(|value| {
                DateTime::parse_from_rfc3339(&value)
                    .map(|timestamp| timestamp.with_timezone(&Utc))
                    .map_err(|_| {
                        invalid_message_query(
                            "since must be an ISO 8601 timestamp with a UTC offset",
                        )
                    })
            })
            .transpose()?;
        let count = self.count.unwrap_or(DEFAULT_MESSAGE_COUNT);
        if count == 0 || count > MAX_MESSAGE_COUNT {
            return Err(invalid_message_query(format!(
                "count must be between 1 and {MAX_MESSAGE_COUNT}"
            )));
        }
        Ok(MessageFilter {
            channels,
            since,
            skip: self.skip.unwrap_or(0),
            count,
        })
    }
}

pub(super) fn invalid_message_query(message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_message_query",
        message,
        None,
    )
}

#[utoipa::path(
    get,
    path = "/clients/{client}/messages",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        MessageQuery
    ),
    responses(
        (status = 200, description = "Recent typed chat and system messages observed for this DLL instance", body = Messages),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No DLL identity has been observed for this client", body = ErrorState)
    )
)]
pub(super) async fn client_messages(
    Path(identifier): Path<String>,
    query: Result<Query<MessageQuery>, QueryRejection>,
    State(state): State<ApiState>,
) -> Result<Json<Messages>, ApiError> {
    let Query(query) = query.map_err(|rejection| {
        invalid_message_query(format!("invalid message query: {rejection}"))
    })?;
    let filter = query.into_filter()?;
    let registry = state.snapshot();
    let client = resolve_client(&registry, &identifier)?;
    let identity = client.identity.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "message_history_unavailable",
            "the client DLL identity is unavailable",
            Some(client.pid),
        )
    })?;
    Ok(Json(state.messages(identity, &filter)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/events",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "Server-Sent Events stream beginning with stream.ready. Each frame has an event name, sequence ID, and a ClientEvent JSON envelope in data.", body = ClientEvent, content_type = "text/event-stream"),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "The client is not connected with a current observation", body = ErrorState)
    )
)]
pub(super) async fn client_events(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Response, ApiError> {
    let receiver = state.subscribe();
    let registry = state.snapshot();
    let client = resolve_client(&registry, &identifier)?;
    if client.status != ClientSnapshotStatus::Connected {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the client is not currently connected",
            Some(client.pid),
        ));
    }
    let identity = client.identity.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the connected client identity is unavailable",
            Some(client.pid),
        )
    })?;
    let snapshot = client.game_snapshot.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the client has not published an observation yet",
            Some(client.pid),
        )
    })?;
    Ok(stream::response(
        client.pid,
        identity,
        snapshot.revision,
        snapshot.event_sequence,
        receiver,
    )
    .into_response())
}
