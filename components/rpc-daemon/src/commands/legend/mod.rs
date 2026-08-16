use super::*;
use darpc_model::{LegendIcon as ModelLegendIcon, LegendMark as ModelLegendMark};
use std::time::Instant;

const LEGEND_TIMEOUT: Duration = Duration::from_secs(3);
const ROUTE_WAIT: Duration = Duration::from_millis(1_250);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct LegendSnapshot {
    pid: u32,
    received_tick_ms: u32,
    marks: Vec<LegendMark>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct LegendMark {
    pub(crate) text: String,
    pub(crate) tag: String,
    pub(crate) color: u8,
    pub(crate) icon: LegendIcon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LegendIcon {
    Aisling,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Heart,
    Victory,
    None,
    Unknown,
}

/// Requests and returns the current legend marks from the character's self-look data.
#[utoipa::path(
    get,
    path = "/clients/{client}/legend",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The current legend marks", body = LegendSnapshot),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not currently in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The game server did not return self-look data within three seconds", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn legend(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<Json<LegendSnapshot>, ApiError> {
    let (pid, identity, _) = action_client(&state, &identifier)?;
    let (status, marks) = request(&state, pid, identity).await?;
    Ok(Json(LegendSnapshot {
        pid,
        received_tick_ms: status.completed_tick_ms.unwrap_or(status.enqueued_tick_ms),
        marks: marks.into_iter().map(LegendMark::from).collect(),
    }))
}

async fn request(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
) -> Result<(darpc_protocol::CommandStatus, Vec<ModelLegendMark>), ApiError> {
    let deadline = Instant::now() + LEGEND_TIMEOUT;
    let mut result = route_legend(
        state,
        pid,
        identity,
        CommandOperation::Submit {
            kind: ProtocolKind::Legend,
            timeout_ms: LEGEND_TIMEOUT.as_millis() as u16,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
    .await?;
    loop {
        match result {
            ProtocolResult::Legend { status, marks } => return Ok((status, marks)),
            ProtocolResult::Status(status) if status.state == ProtocolState::Accepted => {
                if Instant::now() >= deadline {
                    return Err(legend_timeout(pid));
                }
                result = route_legend(
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
                return Err(legend_timeout(pid));
            }
            ProtocolResult::Status(status) => {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "legend_request_failed",
                    format!(
                        "the client legend request ended in state {:?}",
                        status.state
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
            ProtocolResult::NotFound => return Err(legend_timeout(pid)),
            ProtocolResult::Unavailable => return Err(unavailable(pid)),
            ProtocolResult::Who { .. } => return Err(unavailable(pid)),
            ProtocolResult::Player { .. } => return Err(unavailable(pid)),
        }
    }
}

async fn route_legend(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    operation: CommandOperation,
) -> Result<ProtocolResult, ApiError> {
    let receiver = state.route_command(pid, identity, operation)?;
    let reply = timeout(ROUTE_WAIT, receiver)
        .await
        .map_err(|_| legend_timeout(pid))?
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
        CommandReply::Snapshot(_) | CommandReply::Diagnostics(_) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_snapshot_result",
            "the legend route returned a state snapshot",
            Some(pid),
        )),
    }
}

fn legend_timeout(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "legend_timeout",
        "the game server did not return self-look data within three seconds",
        Some(pid),
    )
}

impl From<ModelLegendMark> for LegendMark {
    fn from(mark: ModelLegendMark) -> Self {
        Self {
            text: mark.text,
            tag: mark.tag,
            color: mark.color,
            icon: mark.icon.into(),
        }
    }
}

impl From<ModelLegendIcon> for LegendIcon {
    fn from(icon: ModelLegendIcon) -> Self {
        match icon {
            ModelLegendIcon::Aisling => Self::Aisling,
            ModelLegendIcon::Warrior => Self::Warrior,
            ModelLegendIcon::Rogue => Self::Rogue,
            ModelLegendIcon::Wizard => Self::Wizard,
            ModelLegendIcon::Priest => Self::Priest,
            ModelLegendIcon::Monk => Self::Monk,
            ModelLegendIcon::Heart => Self::Heart,
            ModelLegendIcon::Victory => Self::Victory,
            ModelLegendIcon::None => Self::None,
            ModelLegendIcon::Unknown(_) => Self::Unknown,
        }
    }
}
