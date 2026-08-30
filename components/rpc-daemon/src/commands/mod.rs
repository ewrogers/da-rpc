use crate::{
    api::{ApiError, ApiState, resolve_client},
    registry::{ClientIdentity, ClientSnapshotStatus, hex},
    resync_status::ResyncSchedulerStatus,
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use darpc_model::{
    ClientLifecycle, ClientSnapshot as GameSnapshot, CreatureKind, Direction as ModelDirection,
    InventoryItem, Skill, Spell, SpellTargetType as ModelSpellTargetType,
    WalkMode as ModelWalkMode, WorldObject, emote_code, is_client_emote_code,
};
use darpc_protocol::{
    CommandFailure as ProtocolFailure, CommandKind as ProtocolKind, CommandOperation,
    CommandResult as ProtocolResult, CommandState as ProtocolState,
    CommandStatus as ProtocolStatus, DEFAULT_COMMAND_TIMEOUT_MS,
    ExactRouteInvalidState as ProtocolExactRouteInvalidState,
    ExactRouteInvalidStateReason as ProtocolExactRouteInvalidStateReason, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS, MAX_ITEM_SLOT, MAX_SKILL_SLOT, MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT,
    MAX_WALK_ROUTE_TILES, RouteTile, SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput,
    SpellSlot, SpellTarget, WalkRoute, WalkTarget,
};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use tokio::{sync::oneshot, time::timeout};
use utoipa::ToSchema;

pub(crate) mod ability;
pub(crate) mod assail;
pub(crate) mod bulletin;
pub(crate) mod chant;
pub(crate) mod dialog;
pub(crate) mod exchange;
pub(crate) mod field_map;
pub(crate) mod group;
pub(crate) mod interaction;
pub(crate) mod legend;
pub(crate) mod look;
pub(crate) mod message;
pub(crate) mod message_dialog;
pub(crate) mod movement;
pub(crate) mod player;
pub(crate) mod raw;
pub(crate) mod resync;
pub(crate) mod stat;
pub(crate) mod who;

pub(crate) use ability::{
    CastSpellByName, CastSpellBySlot, CastSpellOptions, SkillNameOptions, SkillSlotOptions,
    SpellTargetOptions, UseSkillOptions, cast_spell, swap_skills, swap_spells, use_skill,
};
pub(crate) use chant::{ChantOptions, ItemChantOptions};
pub(crate) use dialog::{
    DialogInputOptions, DialogRevisionOptions, DialogSelectOptions, InteractOptions,
    close as close_dialog, input as dialog_input, interact, next as dialog_next,
    previous as dialog_previous, select as dialog_select,
};
pub(crate) use exchange::{
    AddExchangeItemOptions, SetExchangeGoldOptions, accept as accept_exchange,
    add_item as add_exchange_item, cancel as cancel_exchange, set_gold as set_exchange_gold,
};
pub(crate) use group::{
    GroupInviteOptions, accept as accept_group_invitation, decline as decline_group_invitation,
    invite as invite_group, toggle as toggle_group,
};
pub(crate) use interaction::{
    DropGoldOptions, DropItemOptions, EmoteOptions, GiveGoldOptions, GiveItemOptions,
    PickupItemOptions, UnequipOptions, UseItemOptions, drop_gold, drop_item, emote, give_gold,
    give_item, pickup_item, swap_items, unequip, use_item,
};
pub(crate) use legend::{LegendIcon, LegendMark, LegendSnapshot, legend};
pub(crate) use look::FarLookOptions;
pub(crate) use message::{SendMessageChannel, SendMessageOptions};
use movement::validate_destination;
pub(crate) use movement::{
    ActionDirection, Destination, RouteOptions, TurnOptions, WalkDestinationOptions,
    WalkDirectionOptions, WalkOptions, WalkRouteOptions, cancel_walk, turn, walk,
};
pub(crate) use player::{cached_player, inspect_player};
pub(crate) use who::{UserState as WhoUserState, WhoClass, WhoList, WhoPlayer, who};

pub(crate) const ROUTER_CAPACITY: usize = 64;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WORKER_CAPACITY: usize = 16;

const ROUTE_TIMEOUT: Duration = Duration::from_secs(2);
const SPELL_CAST_TIMEOUT_MS: u16 = DEFAULT_COMMAND_TIMEOUT_MS + DEFAULT_COMMAND_TIMEOUT_MS / 10;
const LOOK_TIMEOUT_MS: u16 = MAX_COMMAND_TIMEOUT_MS;
const MAX_SKILL_NAME_BYTES: usize = 128;
const MAX_SPELL_NAME_BYTES: usize = 128;
const SPELL_TARGET_DISTANCE: u32 = 14;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SlotSelector {
    /// Select an occupied slot by its one-based slot number.
    slot: Option<u8>,
    /// Select an occupied slot by its case-insensitive name.
    name: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwapSlotsOptions {
    source: SlotSelector,
    destination: SlotSelector,
}

trait NamedSlot {
    fn slot(&self) -> u8;
    fn name(&self) -> Option<&str>;
}

impl NamedSlot for InventoryItem {
    fn slot(&self) -> u8 {
        self.slot
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl NamedSlot for Skill {
    fn slot(&self) -> u8 {
        self.slot
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl NamedSlot for Spell {
    fn slot(&self) -> u8 {
        self.slot
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

fn resolve_slot_swap<T: NamedSlot>(
    pid: u32,
    entries: &[T],
    request: &SwapSlotsOptions,
    collection: &str,
    max_slot: u8,
) -> Result<(u8, u8), ApiError> {
    let source = resolve_slot_selector(pid, entries, &request.source, collection, max_slot, true)?;
    let destination = resolve_slot_selector(
        pid,
        entries,
        &request.destination,
        collection,
        max_slot,
        false,
    )?;
    if source == destination {
        return Err(selector_bad_request(
            pid,
            "source and destination must resolve to different slots",
        ));
    }
    Ok((source, destination))
}

fn resolve_slot_selector<T: NamedSlot>(
    pid: u32,
    entries: &[T],
    selector: &SlotSelector,
    collection: &str,
    max_slot: u8,
    require_occupied: bool,
) -> Result<u8, ApiError> {
    if selector.slot.is_some() == selector.name.is_some() {
        return Err(selector_bad_request(
            pid,
            "each selector must provide exactly one of slot or name",
        ));
    }
    if let Some(slot) = selector.slot {
        if slot == 0 || slot > max_slot {
            return Err(selector_bad_request(
                pid,
                format!("{collection} slot must be from 1 through {max_slot}"),
            ));
        }
        if require_occupied && !entries.iter().any(|entry| entry.slot() == slot) {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "slot_not_found",
                format!("the selected {collection} source slot is empty"),
                Some(pid),
            ));
        }
        return Ok(slot);
    }

    let requested = selector.name.as_deref().unwrap_or_default();
    if requested.is_empty() || requested.len() > MAX_SPELL_NAME_BYTES {
        return Err(selector_bad_request(
            pid,
            format!("{collection} name must contain from 1 through {MAX_SPELL_NAME_BYTES} bytes"),
        ));
    }
    let mut matches = entries.iter().filter(|entry| {
        entry
            .name()
            .is_some_and(|name| name.eq_ignore_ascii_case(requested))
    });
    let slot = matches.next().map(NamedSlot::slot).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "slot_not_found",
            format!("the selected {collection} name was not found"),
            Some(pid),
        )
    })?;
    if matches.next().is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ambiguous_slot_name",
            format!("more than one {collection} entry has that case-insensitive name"),
            Some(pid),
        ));
    }
    Ok(slot)
}

fn selector_bad_request(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_swap_selector",
        message,
        Some(pid),
    )
}

pub(crate) struct CommandCall {
    pub(crate) pid: u32,
    pub(crate) identity: ClientIdentity,
    pub(crate) operation: ClientOperation,
    pub(crate) reply: oneshot::Sender<CommandReply>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
// Command calls already carried the full operation before snapshot requests shared this route.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ClientOperation {
    Command(CommandOperation),
    Diagnostics(darpc_protocol::DiagnosticsOperation),
    Snapshot(SnapshotFreshness),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SnapshotFreshness {
    /// Reuse the worker's bounded recent snapshot when available.
    Recent,
    /// Require a new client capture for revision-sensitive validation.
    Fresh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
// Successful replies retain the complete protocol result until the HTTP task receives it.
#[allow(clippy::large_enum_variant)]
pub(crate) enum CommandReply {
    Result(ProtocolResult),
    Diagnostics(darpc_protocol::DiagnosticsResponse),
    Snapshot(Box<GameSnapshot>),
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
    /// Resync correlation ID, equal to command_id and present only for resync commands.
    #[serde(skip_serializing_if = "Option::is_none")]
    resync_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resync: Option<ResyncSchedulerStatus>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<ExactRouteInvalidStateDiagnostics>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ExactRouteInvalidStateDiagnostics {
    reason: ExactRouteInvalidStateReason,
    route_map_id: u32,
    packet_map_id: Option<u32>,
    native_map_id: Option<u32>,
    packet_position: Option<DiagnosticTilePosition>,
    native_position: Option<DiagnosticTilePosition>,
    staged_position: Option<DiagnosticTilePosition>,
    transition_active: Option<bool>,
    route_mode: Option<DiagnosticWalkMode>,
    current_destination: Option<DiagnosticTilePosition>,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExactRouteInvalidStateReason {
    MapTransitionPending,
    NativeMapUnavailable,
    NativeMapMismatch,
    NativeTransitionUnavailable,
    ConfirmedMapMismatch,
    ConfirmedPositionMismatch,
    MapDimensionsUnavailable,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct DiagnosticTilePosition {
    x: i32,
    y: i32,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticWalkMode {
    NativeRoute,
    ExactRoute,
    Direct,
    Pursuit,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandKind {
    Diagnostic,
    Turn,
    Walk,
    UseSkill,
    CastSpell,
    UseItem,
    DropItem,
    DropGold,
    PickupItem,
    Unequip,
    Emote,
    Interact,
    Dialog,
    GiveItem,
    GiveGold,
    SwapItems,
    SwapSpells,
    SwapSkills,
    Group,
    Who,
    Exchange,
    Chant,
    Legend,
    InspectPlayer,
    Raw,
    Assail,
    Resync,
    Message,
    AddStat,
    SelectFieldMapDestination,
    DismissMessageDialog,
    Look,
    Bulletin,
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
    InsufficientMana,
    Resist,
    NotAllowed,
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
            Ok(command_status(state, pid, identity, status))
        }
        CommandReply::Result(ProtocolResult::ExactRouteInvalidState {
            status,
            diagnostics,
        }) => Ok(CommandStatus::with_exact_route_diagnostics(
            pid,
            identity,
            status,
            diagnostics,
        )),
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
        CommandReply::Result(ProtocolResult::Who { .. }) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_command_result",
            "the command returned a Who list to a non-Who endpoint",
            Some(pid),
        )),
        CommandReply::Result(ProtocolResult::Legend { .. }) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_command_result",
            "the command returned legend marks to a non-legend endpoint",
            Some(pid),
        )),
        CommandReply::Result(ProtocolResult::Player { .. }) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_command_result",
            "the command returned a player profile to a non-inspection endpoint",
            Some(pid),
        )),
        CommandReply::Snapshot(_) | CommandReply::Diagnostics(_) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_snapshot_result",
            "the command route returned a state snapshot",
            Some(pid),
        )),
    }
}

pub(crate) async fn recent_snapshot(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientIdentity, Box<GameSnapshot>), ApiError> {
    snapshot(state, identifier, SnapshotFreshness::Recent).await
}

pub(crate) async fn fresh_snapshot(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientIdentity, Box<GameSnapshot>), ApiError> {
    snapshot(state, identifier, SnapshotFreshness::Fresh).await
}

async fn snapshot(
    state: &ApiState,
    identifier: &str,
    freshness: SnapshotFreshness,
) -> Result<(u32, ClientIdentity, Box<GameSnapshot>), ApiError> {
    let (pid, identity) = connected_client(state, identifier)?;
    let receiver = state.route_snapshot(pid, identity, freshness)?;
    let reply = timeout(ROUTE_TIMEOUT, receiver)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "snapshot_route_timeout",
                "the live client snapshot did not respond within two seconds",
                Some(pid),
            )
        })?
        .map_err(|_| unavailable(pid))?;
    match reply {
        CommandReply::Snapshot(snapshot) => Ok((pid, identity, snapshot)),
        CommandReply::Busy => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "snapshot_queue_full",
            "the bounded client request queue is full",
            Some(pid),
        )),
        CommandReply::Unavailable => Err(unavailable(pid)),
        CommandReply::Result(_) | CommandReply::Diagnostics(_) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected_command_result",
            "the snapshot route returned a command result",
            Some(pid),
        )),
    }
}

pub(super) async fn submit_action(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    kind: ProtocolKind,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let timeout_ms = match kind {
        // The client dispatcher can pause just beyond one second while a prior
        // spell advances through its native cast sequence. Give casts 10%
        // tolerance instead of dropping them before they reach the main thread.
        ProtocolKind::CastSpell(_) => SPELL_CAST_TIMEOUT_MS,
        ProtocolKind::Look(_) => LOOK_TIMEOUT_MS,
        _ => DEFAULT_COMMAND_TIMEOUT_MS,
    };
    let status = route(
        state,
        pid,
        identity,
        CommandOperation::Submit {
            kind,
            timeout_ms,
            wait_ms: MAX_COMMAND_WAIT_MS,
        },
    )
    .await?;
    if matches!(kind, ProtocolKind::Resync)
        && status.state == CommandState::Failed
        && status.failure == Some(CommandFailure::Rejected)
    {
        let mut error = ApiError::new(
            StatusCode::CONFLICT,
            "resync_busy",
            "a client refresh is already in progress",
            Some(pid),
        );
        if let Some(resync) = status.resync {
            error = error.with_resync(resync);
        }
        return Err(error);
    }
    let response_status = if status.state == CommandState::Accepted {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((response_status, Json(status)))
}

pub(super) fn action_request<T>(
    request: Result<Json<T>, JsonRejection>,
) -> Result<Json<T>, ApiError> {
    request.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })
}

pub(super) fn action_client(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientIdentity, GameSnapshot), ApiError> {
    let registry = state.snapshot();
    let client = resolve_client(&registry, identifier)?;
    let identity = client
        .identity
        .filter(|_| client.status == ClientSnapshotStatus::Connected)
        .ok_or_else(|| unavailable(client.pid))?;
    if let Some(reason) = client.snapshot_reason.as_deref() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "client_state_unavailable",
            reason,
            Some(client.pid),
        ));
    }
    let snapshot = client.game_snapshot.as_ref().ok_or_else(|| {
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
    Ok((client.pid, identity, snapshot.as_ref().clone()))
}

pub(crate) fn connected_client(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientIdentity), ApiError> {
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

pub(crate) fn unavailable(pid: u32) -> ApiError {
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
        let resync_id = matches!(status.kind, ProtocolKind::Resync).then_some(status.command_id);
        Self {
            pid,
            instance_id: hex(&identity.dll_instance_id),
            command_id: status.command_id,
            resync_id,
            resync: None,
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
            diagnostics: None,
        }
    }

    fn with_exact_route_diagnostics(
        pid: u32,
        identity: ClientIdentity,
        status: ProtocolStatus,
        diagnostics: ProtocolExactRouteInvalidState,
    ) -> Self {
        Self {
            diagnostics: Some(diagnostics.into()),
            ..Self::new(pid, identity, status)
        }
    }
}

fn command_status(
    state: &ApiState,
    pid: u32,
    identity: ClientIdentity,
    status: ProtocolStatus,
) -> CommandStatus {
    let is_resync = matches!(status.kind, ProtocolKind::Resync);
    let resync_id = status.command_id;
    let accepted = matches!(
        status.state,
        ProtocolState::Accepted | ProtocolState::Executed
    );
    let mut result = CommandStatus::new(pid, identity, status);
    if is_resync {
        result.resync = Some(if accepted {
            state.accept_resync(identity, resync_id)
        } else {
            state.resync_status(identity)
        });
    }
    result
}

impl From<ProtocolExactRouteInvalidState> for ExactRouteInvalidStateDiagnostics {
    fn from(value: ProtocolExactRouteInvalidState) -> Self {
        Self {
            reason: value.reason.into(),
            route_map_id: value.route_map_id,
            packet_map_id: value.packet_map_id,
            native_map_id: value.native_map_id,
            packet_position: value.packet_position.map(Into::into),
            native_position: value.native_position.map(Into::into),
            staged_position: value.staged_position.map(Into::into),
            transition_active: value.transition_active,
            route_mode: value.route_mode.map(Into::into),
            current_destination: value.current_destination.map(Into::into),
        }
    }
}

impl From<ProtocolExactRouteInvalidStateReason> for ExactRouteInvalidStateReason {
    fn from(value: ProtocolExactRouteInvalidStateReason) -> Self {
        match value {
            ProtocolExactRouteInvalidStateReason::MapTransitionPending => {
                Self::MapTransitionPending
            }
            ProtocolExactRouteInvalidStateReason::NativeMapUnavailable => {
                Self::NativeMapUnavailable
            }
            ProtocolExactRouteInvalidStateReason::NativeMapMismatch => Self::NativeMapMismatch,
            ProtocolExactRouteInvalidStateReason::NativeTransitionUnavailable => {
                Self::NativeTransitionUnavailable
            }
            ProtocolExactRouteInvalidStateReason::ConfirmedMapMismatch => {
                Self::ConfirmedMapMismatch
            }
            ProtocolExactRouteInvalidStateReason::ConfirmedPositionMismatch => {
                Self::ConfirmedPositionMismatch
            }
            ProtocolExactRouteInvalidStateReason::MapDimensionsUnavailable => {
                Self::MapDimensionsUnavailable
            }
        }
    }
}

impl From<darpc_model::TilePosition> for DiagnosticTilePosition {
    fn from(value: darpc_model::TilePosition) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<ModelWalkMode> for DiagnosticWalkMode {
    fn from(value: ModelWalkMode) -> Self {
        match value {
            ModelWalkMode::NativeRoute => Self::NativeRoute,
            ModelWalkMode::ExactRoute => Self::ExactRoute,
            ModelWalkMode::Direct => Self::Direct,
            ModelWalkMode::Pursuit => Self::Pursuit,
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
            ProtocolKind::UseItem(_) => Self::UseItem,
            ProtocolKind::DropItem(_) => Self::DropItem,
            ProtocolKind::DropGold(_) => Self::DropGold,
            ProtocolKind::PickupItem(_) => Self::PickupItem,
            ProtocolKind::Unequip(_) => Self::Unequip,
            ProtocolKind::Emote(_) => Self::Emote,
            ProtocolKind::Interact(_) => Self::Interact,
            ProtocolKind::Dialog(_) => Self::Dialog,
            ProtocolKind::GiveItem(_) => Self::GiveItem,
            ProtocolKind::GiveGold(_) => Self::GiveGold,
            ProtocolKind::SwapSlots(SlotSwap::Inventory { .. }) => Self::SwapItems,
            ProtocolKind::SwapSlots(SlotSwap::Spellbook { .. }) => Self::SwapSpells,
            ProtocolKind::SwapSlots(SlotSwap::Skillbook { .. }) => Self::SwapSkills,
            ProtocolKind::Group(_) => Self::Group,
            ProtocolKind::Who => Self::Who,
            ProtocolKind::Exchange(_) => Self::Exchange,
            ProtocolKind::Chant(_) => Self::Chant,
            ProtocolKind::Legend => Self::Legend,
            ProtocolKind::Raw(_) => Self::Raw,
            ProtocolKind::Assail => Self::Assail,
            ProtocolKind::InspectPlayer(_) => Self::InspectPlayer,
            ProtocolKind::Resync => Self::Resync,
            ProtocolKind::Message(_) => Self::Message,
            ProtocolKind::AddStat(_) => Self::AddStat,
            ProtocolKind::SelectFieldMapDestination(_) => Self::SelectFieldMapDestination,
            ProtocolKind::DismissMessageDialog(_) => Self::DismissMessageDialog,
            ProtocolKind::Look(_) => Self::Look,
            ProtocolKind::Bulletin(_) => Self::Bulletin,
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
            ProtocolFailure::InsufficientMana => Self::InsufficientMana,
            ProtocolFailure::Resist => Self::Resist,
            ProtocolFailure::NotAllowed => Self::NotAllowed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_result_failures_have_stable_json_names() {
        for (failure, name) in [
            (ProtocolFailure::InsufficientMana, "insufficient_mana"),
            (ProtocolFailure::Resist, "resist"),
            (ProtocolFailure::InvalidTarget, "invalid_target"),
            (ProtocolFailure::NotAllowed, "not_allowed"),
        ] {
            let failure = CommandFailure::from(failure);
            assert_eq!(
                serde_json::to_value(failure).unwrap(),
                serde_json::json!(name)
            );
        }
    }

    #[test]
    fn exact_route_invalid_state_diagnostics_are_exposed_in_command_json() {
        let status = ProtocolStatus {
            command_id: 7,
            kind: ProtocolKind::Diagnostic,
            state: ProtocolState::Failed,
            enqueued_tick_ms: 10,
            deadline_tick_ms: 1_010,
            started_tick_ms: Some(11),
            completed_tick_ms: Some(12),
            execution_us: Some(15),
            main_thread_id: Some(42),
            failure: Some(ProtocolFailure::InvalidState),
        };
        let diagnostics = ProtocolExactRouteInvalidState {
            reason: ProtocolExactRouteInvalidStateReason::ConfirmedPositionMismatch,
            route_map_id: 500,
            packet_map_id: Some(500),
            native_map_id: Some(500),
            packet_position: Some(darpc_model::TilePosition { x: 10, y: 20 }),
            native_position: Some(darpc_model::TilePosition { x: 10, y: 20 }),
            staged_position: Some(darpc_model::TilePosition { x: 11, y: 20 }),
            transition_active: Some(true),
            route_mode: Some(ModelWalkMode::ExactRoute),
            current_destination: Some(darpc_model::TilePosition { x: 30, y: 20 }),
        };
        let command = CommandStatus::with_exact_route_diagnostics(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            status,
            diagnostics,
        );

        let json = serde_json::to_value(command).unwrap();
        assert_eq!(json["failure"], "invalid_state");
        assert_eq!(json["diagnostics"]["reason"], "confirmed_position_mismatch");
        assert_eq!(json["diagnostics"]["staged_position"]["x"], 11);
        assert_eq!(json["diagnostics"]["route_mode"], "exact_route");
        assert_eq!(json["diagnostics"]["current_destination"]["x"], 30);
    }
}
