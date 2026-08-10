use super::*;
use axum::extract::{Query, rejection::QueryRejection};
use darpc_model::{CharacterClass as ModelClass, UserState as ModelUserState, WhoList as ModelWho};
use std::{str::FromStr, time::Instant};
use utoipa::IntoParams;

const WHO_TIMEOUT: Duration = Duration::from_secs(3);
const ROUTE_WAIT: Duration = Duration::from_millis(1_250);

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct WhoQuery {
    /// Comma-separated classes, such as `warrior,rogue`.
    #[param(example = "warrior,rogue")]
    classes: Option<String>,
    /// Return only players marked as guildmates by this client.
    #[serde(default)]
    guild_only: bool,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct WhoList {
    pid: u32,
    received_tick_ms: u32,
    world_count: u16,
    country_count: u16,
    players: Vec<WhoPlayer>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct WhoPlayer {
    name: String,
    title: String,
    class: WhoClass,
    state: UserState,
    color: u8,
    is_master: bool,
    is_guildmate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WhoClass {
    Peasant,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Unknown,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserState {
    Awake,
    DoNotDisturb,
    Daydreaming,
    NeedGroup,
    Grouped,
    LoneHunter,
    GroupHunting,
    NeedHelp,
    Unknown,
}

/// Requests the current ordered online-player list without opening the client panel.
#[utoipa::path(
    get,
    path = "/clients/{client}/who",
    params(
        ("client" = String, Path, description = "Process ID or current in-game character name"),
        WhoQuery
    ),
    responses(
        (status = 200, description = "The server-ordered online-player list", body = WhoList),
        (status = 400, description = "The class filter was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not currently in game", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The game server did not return the list within three seconds", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn who(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    query: Result<Query<WhoQuery>, QueryRejection>,
) -> Result<Json<WhoList>, ApiError> {
    let Query(query) = query.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_who_query",
            rejection.body_text(),
            None,
        )
    })?;
    let classes = parse_classes(query.classes.as_deref())?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    let (status, list) = request(&state, pid, identity).await?;
    Ok(Json(WhoList::from_model(
        pid,
        status.completed_tick_ms.unwrap_or(status.enqueued_tick_ms),
        list,
        classes.as_deref(),
        query.guild_only,
    )))
}

async fn request(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
) -> Result<(darpc_protocol::CommandStatus, ModelWho), ApiError> {
    let deadline = Instant::now() + WHO_TIMEOUT;
    let mut result = route_who(
        state,
        pid,
        identity,
        CommandOperation::Submit {
            kind: ProtocolKind::Who,
            timeout_ms: WHO_TIMEOUT.as_millis() as u16,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
    .await?;
    loop {
        match result {
            ProtocolResult::Who { status, list } => return Ok((status, list)),
            ProtocolResult::Status(status) if status.state == ProtocolState::Accepted => {
                if Instant::now() >= deadline {
                    return Err(who_timeout(pid));
                }
                result = route_who(
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
                return Err(who_timeout(pid));
            }
            ProtocolResult::Status(status) => {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "who_request_failed",
                    format!("the client Who request ended in state {:?}", status.state),
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
            ProtocolResult::NotFound => return Err(who_timeout(pid)),
            ProtocolResult::Unavailable => return Err(unavailable(pid)),
            ProtocolResult::Legend { .. } => return Err(unavailable(pid)),
            ProtocolResult::Player { .. } => return Err(unavailable(pid)),
        }
    }
}

async fn route_who(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    operation: CommandOperation,
) -> Result<ProtocolResult, ApiError> {
    let receiver = state.route_command(pid, identity, operation)?;
    let reply = timeout(ROUTE_WAIT, receiver)
        .await
        .map_err(|_| who_timeout(pid))?
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

fn parse_classes(value: Option<&str>) -> Result<Option<Vec<WhoClass>>, ApiError> {
    value
        .map(|value| {
            if value.is_empty() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_who_classes",
                    "classes cannot be empty",
                    None,
                ));
            }
            value
                .split(',')
                .map(str::trim)
                .map(|value| WhoClass::from_str(value).map_err(|()| invalid_classes(value)))
                .collect()
        })
        .transpose()
}

fn invalid_classes(value: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_who_classes",
        format!(
            "unknown class `{value}`; expected peasant, warrior, rogue, wizard, priest, or monk"
        ),
        None,
    )
}

fn who_timeout(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "who_timeout",
        "the game server did not return the online-player list within three seconds",
        Some(pid),
    )
}

impl WhoList {
    fn from_model(
        pid: u32,
        received_tick_ms: u32,
        model: ModelWho,
        classes: Option<&[WhoClass]>,
        guild_only: bool,
    ) -> Self {
        Self {
            pid,
            received_tick_ms,
            world_count: model.world_count,
            country_count: model.country_count,
            players: model
                .players
                .into_iter()
                .filter(|player| {
                    (!guild_only || player.is_guildmate)
                        && classes
                            .is_none_or(|classes| classes.contains(&WhoClass::from(player.class)))
                })
                .map(WhoPlayer::from)
                .collect(),
        }
    }
}

impl From<darpc_model::WhoPlayer> for WhoPlayer {
    fn from(player: darpc_model::WhoPlayer) -> Self {
        Self {
            name: player.name,
            title: player.title,
            class: WhoClass::from(player.class),
            state: UserState::from(player.state),
            color: player.color,
            is_master: player.is_master,
            is_guildmate: player.is_guildmate,
        }
    }
}

impl From<ModelClass> for WhoClass {
    fn from(value: ModelClass) -> Self {
        match value {
            ModelClass::Peasant => Self::Peasant,
            ModelClass::Warrior => Self::Warrior,
            ModelClass::Rogue => Self::Rogue,
            ModelClass::Wizard => Self::Wizard,
            ModelClass::Priest => Self::Priest,
            ModelClass::Monk => Self::Monk,
            ModelClass::Unknown(_) => Self::Unknown,
        }
    }
}

impl FromStr for WhoClass {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "peasant" => Ok(Self::Peasant),
            "warrior" => Ok(Self::Warrior),
            "rogue" => Ok(Self::Rogue),
            "wizard" => Ok(Self::Wizard),
            "priest" => Ok(Self::Priest),
            "monk" => Ok(Self::Monk),
            _ => Err(()),
        }
    }
}

impl From<ModelUserState> for UserState {
    fn from(value: ModelUserState) -> Self {
        match value {
            ModelUserState::Awake => Self::Awake,
            ModelUserState::DoNotDisturb => Self::DoNotDisturb,
            ModelUserState::Daydreaming => Self::Daydreaming,
            ModelUserState::NeedGroup => Self::NeedGroup,
            ModelUserState::Grouped => Self::Grouped,
            ModelUserState::LoneHunter => Self::LoneHunter,
            ModelUserState::GroupHunting => Self::GroupHunting,
            ModelUserState::NeedHelp => Self::NeedHelp,
            ModelUserState::Unknown(_) => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_filters_are_case_insensitive() {
        assert_eq!(
            parse_classes(Some("WARRIOR,rogue")).unwrap().unwrap(),
            vec![WhoClass::Warrior, WhoClass::Rogue]
        );
        assert!(parse_classes(Some("bard")).is_err());
    }

    #[test]
    fn filters_without_reordering_server_rows() {
        let player = |name: &str, class, is_guildmate| darpc_model::WhoPlayer {
            name: name.into(),
            title: String::new(),
            class,
            state: ModelUserState::Awake,
            color: 0,
            is_master: false,
            is_guildmate,
        };
        let list = WhoList::from_model(
            42,
            100,
            ModelWho {
                world_count: 50,
                country_count: 3,
                players: vec![
                    player("First", ModelClass::Warrior, true),
                    player("Second", ModelClass::Rogue, false),
                    player("Third", ModelClass::Warrior, true),
                ],
            },
            Some(&[WhoClass::Warrior]),
            true,
        );
        assert_eq!(
            list.players
                .iter()
                .map(|player| player.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Third"]
        );
        assert_eq!(list.country_count, 3);
    }
}
