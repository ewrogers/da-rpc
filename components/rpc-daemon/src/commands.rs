use crate::{
    api::{ApiError, ApiState, resolve_client},
    registry::{ClientIdentity, ClientSnapshotStatus, hex},
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use darpc_protocol::{
    CommandFailure as ProtocolFailure, CommandKind as ProtocolKind, CommandOperation,
    CommandResult as ProtocolResult, CommandState as ProtocolState,
    CommandStatus as ProtocolStatus, DEFAULT_COMMAND_TIMEOUT_MS, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{sync::oneshot, time::timeout};
use utoipa::ToSchema;

pub(crate) const ROUTER_CAPACITY: usize = 64;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WORKER_CAPACITY: usize = 16;

const ROUTE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct CommandCall {
    pub(crate) pid: u32,
    pub(crate) identity: ClientIdentity,
    pub(crate) operation: CommandOperation,
    pub(crate) reply: oneshot::Sender<CommandReply>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum CommandReply {
    Result(ProtocolResult),
    Busy,
    Unavailable,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticOptions {
    /// Time allowed for the command to begin on a client tick.
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u16,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct CommandStatus {
    pid: u32,
    instance_id: String,
    command_id: u32,
    kind: CommandKind,
    state: CommandState,
    enqueued_tick_ms: u32,
    deadline_tick_ms: u32,
    started_tick_ms: Option<u32>,
    completed_tick_ms: Option<u32>,
    queue_delay_ms: Option<u32>,
    execution_us: Option<u32>,
    main_thread_id: Option<u32>,
    failure: Option<CommandFailure>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandKind {
    Diagnostic,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandState {
    Accepted,
    Executed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandFailure {
    Internal,
}

#[utoipa::path(
    post,
    path = "/clients/{client}/commands/diagnostic",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body = DiagnosticOptions,
    responses(
        (status = 200, description = "The diagnostic executed on the client main thread", body = CommandStatus),
        (status = 202, description = "The diagnostic was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The request body or timeout was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn diagnostic(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DiagnosticOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = request.map_err(|rejection| {
        ApiError::new(
            rejection.status(),
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;
    if request.timeout_ms == 0 || request.timeout_ms > MAX_COMMAND_TIMEOUT_MS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_command_timeout",
            format!("timeout_ms must be from 1 through {MAX_COMMAND_TIMEOUT_MS}"),
            None,
        ));
    }
    let (pid, identity) = connected_client(&state, &identifier)?;
    let status = route(
        &state,
        pid,
        identity,
        CommandOperation::Submit {
            kind: ProtocolKind::Diagnostic,
            timeout_ms: request.timeout_ms,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
    .await?;
    let response_status = if status.state == CommandState::Accepted {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((response_status, Json(status)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/commands/{command_id}",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("command_id" = u32, Path, description = "Nonzero client-local command identifier")
    ),
    responses(
        (status = 200, description = "The latest command state", body = CommandStatus),
        (status = 400, description = "The command identifier was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client or command was not found", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn status(
    State(state): State<ApiState>,
    Path((identifier, command_id)): Path<(String, u32)>,
) -> Result<Json<CommandStatus>, ApiError> {
    validate_command_id(command_id)?;
    let (pid, identity) = connected_client(&state, &identifier)?;
    route(
        &state,
        pid,
        identity,
        CommandOperation::Query {
            command_id,
            wait_ms: 0,
        },
    )
    .await
    .map(Json)
}

#[utoipa::path(
    delete,
    path = "/clients/{client}/commands/{command_id}",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        ("command_id" = u32, Path, description = "Nonzero client-local command identifier")
    ),
    responses(
        (status = 200, description = "The resulting command state", body = CommandStatus),
        (status = 400, description = "The command identifier was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client or command was not found", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn cancel(
    State(state): State<ApiState>,
    Path((identifier, command_id)): Path<(String, u32)>,
) -> Result<Json<CommandStatus>, ApiError> {
    validate_command_id(command_id)?;
    let (pid, identity) = connected_client(&state, &identifier)?;
    route(
        &state,
        pid,
        identity,
        CommandOperation::Cancel { command_id },
    )
    .await
    .map(Json)
}

async fn route(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    operation: CommandOperation,
) -> Result<CommandStatus, ApiError> {
    let receiver = state.route_command(pid, identity, operation)?;
    let reply = timeout(ROUTE_TIMEOUT, receiver)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "command_route_timeout",
                "the daemon command route did not respond within two seconds",
                Some(pid),
            )
        })?
        .map_err(|_| unavailable(pid))?;
    match reply {
        CommandReply::Result(ProtocolResult::Status(status)) => {
            Ok(CommandStatus::new(pid, identity, status))
        }
        CommandReply::Result(ProtocolResult::Busy) | CommandReply::Busy => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "command_queue_full",
            "the bounded command queue is full",
            Some(pid),
        )),
        CommandReply::Result(ProtocolResult::NotFound) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "command_not_found",
            "the command does not exist or its retained result has expired",
            Some(pid),
        )),
        CommandReply::Result(ProtocolResult::Unavailable) | CommandReply::Unavailable => {
            Err(unavailable(pid))
        }
    }
}

fn connected_client(state: &ApiState, identifier: &str) -> Result<(u32, ClientIdentity), ApiError> {
    let registry = state.snapshot();
    let client = resolve_client(&registry, identifier)?;
    let identity = client
        .identity
        .filter(|_| client.status == ClientSnapshotStatus::Connected)
        .ok_or_else(|| unavailable(client.pid))?;
    Ok((client.pid, identity))
}

fn validate_command_id(command_id: u32) -> Result<(), ApiError> {
    if command_id == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_command_id",
            "command_id must be greater than zero",
            None,
        ));
    }
    Ok(())
}

fn unavailable(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "command_unavailable",
        "the client command path is not connected",
        Some(pid),
    )
}

const fn default_timeout_ms() -> u16 {
    DEFAULT_COMMAND_TIMEOUT_MS
}

impl CommandStatus {
    fn new(pid: u32, identity: ClientIdentity, status: ProtocolStatus) -> Self {
        Self {
            pid,
            instance_id: hex(&identity.dll_instance_id),
            command_id: status.command_id,
            kind: status.kind.into(),
            state: status.state.into(),
            enqueued_tick_ms: status.enqueued_tick_ms,
            deadline_tick_ms: status.deadline_tick_ms,
            started_tick_ms: status.started_tick_ms,
            completed_tick_ms: status.completed_tick_ms,
            queue_delay_ms: status
                .started_tick_ms
                .map(|started| started.wrapping_sub(status.enqueued_tick_ms)),
            execution_us: status.execution_us,
            main_thread_id: status.main_thread_id,
            failure: status.failure.map(CommandFailure::from),
        }
    }
}

impl From<ProtocolKind> for CommandKind {
    fn from(kind: ProtocolKind) -> Self {
        match kind {
            ProtocolKind::Diagnostic => Self::Diagnostic,
        }
    }
}

impl From<ProtocolState> for CommandState {
    fn from(state: ProtocolState) -> Self {
        match state {
            ProtocolState::Accepted => Self::Accepted,
            ProtocolState::Executed => Self::Executed,
            ProtocolState::Failed => Self::Failed,
            ProtocolState::Cancelled => Self::Cancelled,
            ProtocolState::TimedOut => Self::TimedOut,
        }
    }
}

impl From<ProtocolFailure> for CommandFailure {
    fn from(failure: ProtocolFailure) -> Self {
        match failure {
            ProtocolFailure::Internal => Self::Internal,
        }
    }
}
