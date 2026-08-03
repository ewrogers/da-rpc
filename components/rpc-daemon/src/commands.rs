use crate::{
    api::{ApiError, ApiState, resolve_client},
    registry::{ClientIdentity, ClientSnapshotStatus, hex},
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use darpc_model::{
    ClientLifecycle, ClientSnapshot as GameSnapshot, CreatureKind, Direction as ModelDirection,
    SpellTargetType as ModelSpellTargetType, WorldObject,
};
use darpc_protocol::{
    CommandFailure as ProtocolFailure, CommandKind as ProtocolKind, CommandOperation,
    CommandResult as ProtocolResult, CommandState as ProtocolState,
    CommandStatus as ProtocolStatus, DEFAULT_COMMAND_TIMEOUT_MS, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS, MAX_SKILL_SLOT, MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT, SkillSlot,
    SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, WalkTarget,
};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use tokio::{sync::oneshot, time::timeout};
use utoipa::ToSchema;

pub(crate) const ROUTER_CAPACITY: usize = 64;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WORKER_CAPACITY: usize = 16;

const ROUTE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SKILL_NAME_BYTES: usize = 128;
const MAX_SPELL_NAME_BYTES: usize = 128;
const SPELL_TARGET_DISTANCE: u32 = 14;

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
    x: i32,
    y: i32,
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

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillSlotOptions {
    slot: u8,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillNameOptions {
    name: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum UseSkillOptions {
    Slot(SkillSlotOptions),
    Name(SkillNameOptions),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum SpellTargetOptions {
    Name(String),
    Id(u32),
    Tile(Destination),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CastSpellBySlot {
    slot: u8,
    #[serde(default)]
    target: Option<SpellTargetOptions>,
    #[serde(default)]
    input: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CastSpellByName {
    name: String,
    #[serde(default)]
    target: Option<SpellTargetOptions>,
    #[serde(default)]
    input: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum CastSpellOptions {
    Slot(CastSpellBySlot),
    Name(CastSpellByName),
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
    Turn,
    Walk,
    UseSkill,
    CastSpell,
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
    InvalidState,
    InvalidDestination,
    Rejected,
    NoPath,
    InvalidSkill,
    InvalidSpell,
    InvalidArguments,
    InvalidTarget,
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

#[utoipa::path(
    post,
    path = "/clients/{client}/skills/use",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body(
        content = UseSkillOptions,
        example = json!({"name": "Assail"})
    ),
    responses(
        (status = 200, description = "The normal client skill activation routine completed or reported a local rejection", body = CommandStatus),
        (status = 202, description = "The skill command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The selector body, slot, or name was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client or selected learned skill was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game or its skillbook is unavailable", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn use_skill(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<UseSkillOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let slot = resolve_skill(pid, &snapshot, request)?;
    submit_action(&state, pid, identity, ProtocolKind::UseSkill(slot)).await
}

#[utoipa::path(
    post,
    path = "/clients/{client}/spells/cast",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body(
        content = CastSpellOptions,
        example = json!({"name": "Beag Ioc", "target": "Eidolon"})
    ),
    responses(
        (status = 200, description = "The native spell cast was started or submitted immediately", body = CommandStatus),
        (status = 202, description = "The spell command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The spell selector, argument shape, input, or tile was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client, learned spell, or requested target was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game or required state is unavailable", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn cast_spell(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<CastSpellOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let cast = resolve_spell(pid, &snapshot, request)?;
    submit_action(&state, pid, identity, ProtocolKind::CastSpell(cast)).await
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

async fn submit_action(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    kind: ProtocolKind,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let status = route(
        state,
        pid,
        identity,
        CommandOperation::Submit {
            kind,
            timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
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

fn action_request<T>(request: Result<Json<T>, JsonRejection>) -> Result<Json<T>, ApiError> {
    request.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })
}

fn action_client(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientIdentity, GameSnapshot), ApiError> {
    let registry = state.snapshot();
    let client = resolve_client(&registry, identifier)?;
    let identity = client
        .identity
        .filter(|_| client.status == ClientSnapshotStatus::Connected)
        .ok_or_else(|| unavailable(client.pid))?;
    let snapshot = client.game_snapshot.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "client_state_unavailable",
            "the client does not have a ready state snapshot",
            Some(client.pid),
        )
    })?;
    if snapshot.lifecycle != ClientLifecycle::InGame {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "client_not_in_game",
            "the client is not currently in game",
            Some(client.pid),
        ));
    }
    Ok((client.pid, identity, snapshot))
}

fn validate_destination(
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

fn resolve_skill(
    pid: u32,
    snapshot: &GameSnapshot,
    request: UseSkillOptions,
) -> Result<SkillSlot, ApiError> {
    let skills = snapshot
        .character
        .as_ref()
        .and_then(|character| character.skillbook.as_ref())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "skillbook_unavailable",
                "the client's current skillbook is unavailable",
                Some(pid),
            )
        })?;
    match request {
        UseSkillOptions::Slot(options) => {
            let slot = SkillSlot::new(options.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_skill_slot",
                    format!("slot must be from 1 through {MAX_SKILL_SLOT}"),
                    Some(pid),
                )
            })?;
            skills
                .iter()
                .any(|skill| skill.slot == slot.get())
                .then_some(slot)
                .ok_or_else(|| skill_not_found(pid))
        }
        UseSkillOptions::Name(options) => {
            if options.name.is_empty() || options.name.len() > MAX_SKILL_NAME_BYTES {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_skill_name",
                    format!("name must contain from 1 through {MAX_SKILL_NAME_BYTES} bytes"),
                    Some(pid),
                ));
            }
            let mut matches = skills.iter().filter(|skill| {
                skill
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&options.name))
            });
            let skill = matches.next().ok_or_else(|| skill_not_found(pid))?;
            if matches.next().is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ambiguous_skill_name",
                    "more than one learned skill has that case-insensitive name",
                    Some(pid),
                ));
            }
            SkillSlot::new(skill.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "invalid_skillbook",
                    "the retained skillbook contains an invalid slot",
                    Some(pid),
                )
            })
        }
    }
}

fn skill_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "skill_not_found",
        "the selected skill is not currently learned",
        Some(pid),
    )
}

fn resolve_spell(
    pid: u32,
    snapshot: &GameSnapshot,
    request: CastSpellOptions,
) -> Result<SpellCast, ApiError> {
    let spells = snapshot
        .character
        .as_ref()
        .and_then(|character| character.spellbook.as_ref())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "spellbook_unavailable",
                "the client's current spellbook is unavailable",
                Some(pid),
            )
        })?;
    let (slot, target, input) = match request {
        CastSpellOptions::Slot(options) => {
            let slot = SpellSlot::new(options.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_slot",
                    format!("slot must be from 1 through {MAX_SPELL_SLOT}"),
                    Some(pid),
                )
            })?;
            (slot, options.target, options.input)
        }
        CastSpellOptions::Name(options) => {
            validate_spell_name(pid, &options.name)?;
            let mut matches = spells.iter().filter(|spell| {
                spell
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&options.name))
            });
            let spell = matches.next().ok_or_else(|| spell_not_found(pid))?;
            if matches.next().is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ambiguous_spell_name",
                    "more than one learned spell has that case-insensitive name",
                    Some(pid),
                ));
            }
            let slot = SpellSlot::new(spell.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "invalid_spellbook",
                    "the retained spellbook contains an invalid slot",
                    Some(pid),
                )
            })?;
            (slot, options.target, options.input)
        }
    };
    let spell = spells
        .iter()
        .find(|spell| spell.slot == slot.get())
        .ok_or_else(|| spell_not_found(pid))?;
    let arguments = match spell.target_type {
        ModelSpellTargetType::None if target.is_none() && input.is_none() => SpellArguments::None,
        ModelSpellTargetType::TextInput if target.is_none() => {
            let input = input.ok_or_else(|| invalid_spell_arguments(pid, "input is required"))?;
            let input = SpellInput::new(&input).ok_or_else(|| {
                invalid_spell_arguments(
                    pid,
                    format!("input must contain from 1 through {MAX_SPELL_INPUT_LEN} ASCII bytes"),
                )
            })?;
            SpellArguments::Input(input)
        }
        ModelSpellTargetType::Target if input.is_none() => target
            .map_or(Ok(SpellArguments::None), |target| {
                resolve_spell_target(pid, snapshot, target).map(SpellArguments::Target)
            })?,
        ModelSpellTargetType::Unknown(_) => {
            return Err(invalid_spell_arguments(
                pid,
                "this spell uses a numeric or unsupported argument type",
            ));
        }
        _ => {
            return Err(invalid_spell_arguments(
                pid,
                "the supplied target or input does not match this spell's argument type",
            ));
        }
    };
    Ok(SpellCast { slot, arguments })
}

fn resolve_spell_target(
    pid: u32,
    snapshot: &GameSnapshot,
    target: SpellTargetOptions,
) -> Result<SpellTarget, ApiError> {
    match target {
        SpellTargetOptions::Tile(tile) => {
            validate_destination(pid, snapshot, tile)?;
            Ok(SpellTarget::Tile {
                x: tile.x,
                y: tile.y,
            })
        }
        SpellTargetOptions::Id(id) => {
            let id = NonZeroU32::new(id).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_target",
                    "target object ID must be greater than zero",
                    Some(pid),
                )
            })?;
            object_target(snapshot, id.get())
                .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
                .map(|_| SpellTarget::Object(id))
                .ok_or_else(|| target_not_found(pid))
        }
        SpellTargetOptions::Name(name) => {
            if name.is_empty() || name.len() > MAX_SPELL_NAME_BYTES {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_target_name",
                    format!("target name must contain from 1 through {MAX_SPELL_NAME_BYTES} bytes"),
                    Some(pid),
                ));
            }
            let id = named_target(snapshot, &name).ok_or_else(|| target_not_found(pid))?;
            NonZeroU32::new(id)
                .map(SpellTarget::Object)
                .ok_or_else(|| target_not_found(pid))
        }
    }
}

fn named_target(snapshot: &GameSnapshot, requested: &str) -> Option<u32> {
    let character = snapshot.character.as_ref()?;
    let (self_x, self_y) = character
        .location
        .as_ref()?
        .x
        .zip(character.location.as_ref()?.y)?;
    let objects = snapshot.objects.as_deref().unwrap_or_default();
    let player = objects
        .iter()
        .filter_map(|object| match object {
            WorldObject::Player { id, name, x, y, .. }
                if name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
            {
                Some((*id, tile_distance(self_x, self_y, *x, *y)))
            }
            _ => None,
        })
        .chain(
            character
                .id
                .zip(character.name.as_deref())
                .and_then(|(id, name)| name.eq_ignore_ascii_case(requested).then_some((id, 0))),
        )
        .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
        .min_by_key(|(id, distance)| (*distance, *id));
    if let Some((id, _)) = player {
        return Some(id);
    }
    objects
        .iter()
        .filter_map(|object| match object {
            WorldObject::Creature {
                id,
                kind: CreatureKind::Npc,
                name,
                x,
                y,
                ..
            } if name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
            {
                Some((*id, tile_distance(self_x, self_y, *x, *y)))
            }
            _ => None,
        })
        .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
        .min_by_key(|(id, distance)| (*distance, *id))
        .map(|(id, _)| id)
}

fn object_target(snapshot: &GameSnapshot, requested_id: u32) -> Option<((i32, i32), u32)> {
    let character = snapshot.character.as_ref()?;
    let location = character.location.as_ref()?;
    let (self_x, self_y) = location.x.zip(location.y)?;
    if character.id == Some(requested_id) {
        return Some(((self_x, self_y), 0));
    }
    snapshot.objects.as_ref()?.iter().find_map(|object| {
        (object.id() == requested_id).then(|| {
            let (x, y) = object.position();
            ((x, y), tile_distance(self_x, self_y, x, y))
        })
    })
}

const fn tile_distance(left_x: i32, left_y: i32, right_x: i32, right_y: i32) -> u32 {
    left_x
        .abs_diff(right_x)
        .saturating_add(left_y.abs_diff(right_y))
}

fn validate_spell_name(pid: u32, name: &str) -> Result<(), ApiError> {
    if name.is_empty() || name.len() > MAX_SPELL_NAME_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_spell_name",
            format!("name must contain from 1 through {MAX_SPELL_NAME_BYTES} bytes"),
            Some(pid),
        ));
    }
    Ok(())
}

fn spell_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "spell_not_found",
        "the selected spell is not currently learned",
        Some(pid),
    )
}

fn target_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "spell_target_not_found",
        "the selected player or NPC is not currently visible within 14 tiles",
        Some(pid),
    )
}

fn invalid_spell_arguments(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_spell_arguments",
        message.into(),
        Some(pid),
    )
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
            ProtocolKind::Turn(_) => Self::Turn,
            ProtocolKind::Walk(_) => Self::Walk,
            ProtocolKind::UseSkill(_) => Self::UseSkill,
            ProtocolKind::CastSpell(_) => Self::CastSpell,
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
            ProtocolFailure::InvalidState => Self::InvalidState,
            ProtocolFailure::InvalidDestination => Self::InvalidDestination,
            ProtocolFailure::Rejected => Self::Rejected,
            ProtocolFailure::NoPath => Self::NoPath,
            ProtocolFailure::InvalidSkill => Self::InvalidSkill,
            ProtocolFailure::InvalidSpell => Self::InvalidSpell,
            ProtocolFailure::InvalidArguments => Self::InvalidArguments,
            ProtocolFailure::InvalidTarget => Self::InvalidTarget,
        }
    }
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
