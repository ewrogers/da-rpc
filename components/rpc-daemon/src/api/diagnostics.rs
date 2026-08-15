use super::*;
use crate::commands::{CommandReply, connected_client, unavailable};
use darpc_protocol::{DiagnosticsOperation, HookTimingStage as ProtocolStage};
use tokio::time::timeout;

const ROUTE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticsMode {
    Disabled,
    HookTiming,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HookTimingStage {
    Tick,
    Movement,
    Commands,
    Player,
    State,
    Snapshot,
    Event,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticsOptions {
    pub(crate) mode: DiagnosticsMode,
    #[serde(default)]
    pub(crate) reset: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct DiagnosticsState {
    pub(crate) pid: u32,
    pub(crate) mode: DiagnosticsMode,
    pub(crate) hook_timings: Vec<HookTiming>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct HookTiming {
    pub(crate) stage: HookTimingStage,
    pub(crate) budget_us: u32,
    pub(crate) call_count: u64,
    pub(crate) total_duration_us: u64,
    pub(crate) average_duration_us: u64,
    pub(crate) maximum_duration_us: u32,
    pub(crate) over_budget_count: u64,
    pub(crate) last_duration_us: u32,
}

#[utoipa::path(
    get,
    path = "/clients/{client}/diagnostics/hooks",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "Current hook timing counters", body = DiagnosticsState),
        (status = 404, description = "The client was not found", body = ErrorState),
        (status = 503, description = "The client diagnostics path is unavailable", body = ErrorState),
        (status = 504, description = "The diagnostics query timed out", body = ErrorState)
    )
)]
pub(super) async fn hooks(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<DiagnosticsState>, ApiError> {
    query(&state, &identifier, DiagnosticsOperation::Query)
        .await
        .map(Json)
}

#[utoipa::path(
    put,
    path = "/clients/{client}/diagnostics",
    request_body(content = DiagnosticsOptions, description = "Runtime diagnostics mode and optional counter reset", content_type = "application/json"),
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "Updated diagnostics state", body = DiagnosticsState),
        (status = 400, description = "The diagnostics options were invalid", body = ErrorState),
        (status = 404, description = "The client was not found", body = ErrorState),
        (status = 503, description = "The client diagnostics path is unavailable", body = ErrorState),
        (status = 504, description = "The diagnostics update timed out", body = ErrorState)
    )
)]
pub(super) async fn update(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
    request: Result<Json<DiagnosticsOptions>, JsonRejection>,
) -> Result<Json<DiagnosticsState>, ApiError> {
    let Json(options) = request.map_err(|rejection| {
        ApiError::new(
            rejection.status(),
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;
    if options.reset {
        query(&state, &identifier, DiagnosticsOperation::Reset).await?;
    }
    let operation = match options.mode {
        DiagnosticsMode::Disabled => DiagnosticsOperation::Disable,
        DiagnosticsMode::HookTiming => DiagnosticsOperation::EnableHookTiming,
    };
    query(&state, &identifier, operation).await.map(Json)
}

async fn query(
    state: &ApiState,
    identifier: &str,
    operation: DiagnosticsOperation,
) -> Result<DiagnosticsState, ApiError> {
    let (pid, identity) = connected_client(state, identifier)?;
    let receiver = state.route_diagnostics(pid, identity, operation)?;
    let reply = timeout(ROUTE_TIMEOUT, receiver)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "diagnostics_route_timeout",
                "the diagnostics route did not respond within two seconds",
                Some(pid),
            )
        })?
        .map_err(|_| unavailable(pid))?;
    match reply {
        CommandReply::Diagnostics(response) => Ok(DiagnosticsState::from_response(pid, response)),
        CommandReply::Busy => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "diagnostics_queue_full",
            "the bounded client request queue is full",
            Some(pid),
        )),
        CommandReply::Unavailable => Err(unavailable(pid)),
        CommandReply::Result(_) | CommandReply::Snapshot(_) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_diagnostics_result",
            "the diagnostics route returned a different result",
            Some(pid),
        )),
    }
}

impl DiagnosticsState {
    fn from_response(pid: u32, response: darpc_protocol::DiagnosticsResponse) -> Self {
        Self {
            pid,
            mode: match response.mode {
                darpc_protocol::DiagnosticsMode::Disabled => DiagnosticsMode::Disabled,
                darpc_protocol::DiagnosticsMode::HookTiming => DiagnosticsMode::HookTiming,
            },
            hook_timings: response
                .hook_timings
                .into_iter()
                .map(HookTiming::from)
                .collect(),
        }
    }
}

impl From<darpc_protocol::HookTimingRecord> for HookTiming {
    fn from(value: darpc_protocol::HookTimingRecord) -> Self {
        Self {
            stage: match value.stage {
                ProtocolStage::Tick => HookTimingStage::Tick,
                ProtocolStage::Movement => HookTimingStage::Movement,
                ProtocolStage::Commands => HookTimingStage::Commands,
                ProtocolStage::Player => HookTimingStage::Player,
                ProtocolStage::State => HookTimingStage::State,
                ProtocolStage::Snapshot => HookTimingStage::Snapshot,
                ProtocolStage::Event => HookTimingStage::Event,
            },
            budget_us: value.budget_us,
            call_count: value.call_count,
            total_duration_us: value.total_duration_us,
            average_duration_us: value
                .total_duration_us
                .checked_div(value.call_count)
                .unwrap_or(0),
            maximum_duration_us: value.maximum_duration_us,
            over_budget_count: value.over_budget_count,
            last_duration_us: value.last_duration_us,
        }
    }
}
