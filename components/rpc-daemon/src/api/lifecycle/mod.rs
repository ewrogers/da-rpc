use super::*;

#[utoipa::path(
    post,
    path = "/clients/{client}/load",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The DLL was already loaded", body = LoadResult),
        (status = 202, description = "The DLL was loaded and the daemon is connecting", body = LoadResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "The client is busy or another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
pub(super) async fn load(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<LoadResult>), ApiError> {
    let (pid, status) = tracked_status(&state, &identifier)?;
    match status {
        ClientSnapshotStatus::Connected => {
            return Ok((StatusCode::OK, Json(LoadResult::unchanged(pid))));
        }
        ClientSnapshotStatus::Busy | ClientSnapshotStatus::Incompatible => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "client_busy",
                "the client DLL is already owned by another or incompatible controller",
                Some(pid),
            ));
        }
        ClientSnapshotStatus::Initializing => {
            return Err(operation_in_progress(pid));
        }
        ClientSnapshotStatus::Connecting
        | ClientSnapshotStatus::NotLoaded
        | ClientSnapshotStatus::Disconnected => {}
    }

    state.emit(DaemonEvent::Status(ConnectionEvent::Initializing { pid }))?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.load(pid)).await?;
    state.emit(DaemonEvent::Status(ConnectionEvent::Connecting { pid }))?;
    Ok((StatusCode::ACCEPTED, Json(LoadResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/{client}/unload",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The DLL is unloaded", body = UnloadResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "Another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
pub(super) async fn unload(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<UnloadResult>), ApiError> {
    let (pid, status) = tracked_status(&state, &identifier)?;
    match status {
        ClientSnapshotStatus::NotLoaded => {
            return Ok((StatusCode::OK, Json(UnloadResult::unchanged(pid))));
        }
        ClientSnapshotStatus::Initializing => return Err(operation_in_progress(pid)),
        ClientSnapshotStatus::Connecting
        | ClientSnapshotStatus::Connected
        | ClientSnapshotStatus::Busy
        | ClientSnapshotStatus::Disconnected
        | ClientSnapshotStatus::Incompatible => {}
    }

    state.emit(DaemonEvent::Status(ConnectionEvent::Initializing { pid }))?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.unload(pid)).await?;
    state.emit(DaemonEvent::Status(ConnectionEvent::NotLoaded { pid }))?;
    Ok((StatusCode::OK, Json(UnloadResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/launch",
    request_body(content = LaunchOptions, description = "Supported Dark Ages executable path and launch options", content_type = "application/json"),
    responses(
        (status = 201, description = "The configured client was launched with the DLL initialized", body = LifecycleResult),
        (status = 400, description = "The launch options were invalid", body = ErrorState),
        (status = 413, description = "The request body exceeded 4 KiB", body = ErrorState),
        (status = 422, description = "Loader validation rejected the configured client", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
pub(super) async fn launch(
    State(state): State<ApiState>,
    request: Result<Json<LaunchOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<LifecycleResult>), ApiError> {
    let Json(request) = request.map_err(|rejection| {
        ApiError::new(
            rejection.status(),
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;
    let options = ManagedLaunchOptions::try_from(request)?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.launch(&options)).await?;
    state.emit(DaemonEvent::Track(outcome.pid))?;
    Ok((StatusCode::CREATED, Json(LifecycleResult::from(outcome))))
}

pub(crate) fn resolve_client<'a>(
    registry: &'a RegistrySnapshot,
    identifier: &str,
) -> Result<&'a RegistryClientSnapshot, ApiError> {
    if identifier.bytes().all(|byte| byte.is_ascii_digit()) {
        let pid = identifier.parse::<u32>().map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_client",
                "numeric client identifiers must be valid nonzero process IDs",
                None,
            )
        })?;
        if pid == 0 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_client",
                "numeric client identifiers must be valid nonzero process IDs",
                Some(pid),
            ));
        }
        return registry
            .clients
            .iter()
            .find(|client| client.pid == pid)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "client_not_found",
                    format!("process {pid} is not a discovered or configured client"),
                    Some(pid),
                )
            });
    }

    let mut matches = registry.clients.iter().filter(|client| {
        current_character_name(client).is_some_and(|name| name.eq_ignore_ascii_case(identifier))
    });
    let Some(client) = matches.next() else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "client_not_found",
            format!("no connected in-game client is named {identifier:?}"),
            None,
        ));
    };
    if matches.next().is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ambiguous_client",
            format!("more than one connected in-game client is named {identifier:?}"),
            None,
        ));
    }
    Ok(client)
}

pub(super) fn resolve_game_snapshot<'a>(
    registry: &'a RegistrySnapshot,
    identifier: &str,
) -> Result<(u32, &'a darpc_model::ClientSnapshot), ApiError> {
    let client = resolve_client(registry, identifier)?;
    let pid = client.pid;
    if let Some(reason) = client.snapshot_reason.as_deref() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "observation_unavailable",
            reason,
            Some(pid),
        ));
    }
    let snapshot = client.game_snapshot.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "observation_unavailable",
            "the client has not published an observation yet",
            Some(pid),
        )
    })?;
    Ok((pid, snapshot.as_ref()))
}

pub(super) fn current_character_name(client: &RegistryClientSnapshot) -> Option<&str> {
    if client.status != ClientSnapshotStatus::Connected || client.snapshot_reason.is_some() {
        return None;
    }
    let snapshot = client.game_snapshot.as_ref()?;
    if snapshot.lifecycle != darpc_model::ClientLifecycle::InGame {
        return None;
    }
    snapshot
        .character
        .as_ref()?
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
}

pub(super) fn tracked_status(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientSnapshotStatus), ApiError> {
    let registry = state.snapshot();
    let client = resolve_client(&registry, identifier)?;
    Ok((client.pid, client.status))
}

pub(super) async fn run_lifecycle(
    operation: impl FnOnce() -> Result<LifecycleOutcome, ManagementError> + Send + 'static,
) -> Result<LifecycleOutcome, ApiError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "management_worker_failed",
                format!("management worker failed: {error}"),
                None,
            )
        })?
        .map_err(ApiError::from)
}

pub(super) fn operation_in_progress(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "operation_in_progress",
        "another lifecycle operation is already active for this client",
        Some(pid),
    )
}
