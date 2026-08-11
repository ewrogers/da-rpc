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
    InventoryItem, Skill, Spell, SpellTargetType as ModelSpellTargetType, WorldObject, emote_code,
    is_client_emote_code,
};
use darpc_protocol::{
    CommandFailure as ProtocolFailure, CommandKind as ProtocolKind, CommandOperation,
    CommandResult as ProtocolResult, CommandState as ProtocolState,
    CommandStatus as ProtocolStatus, DEFAULT_COMMAND_TIMEOUT_MS, MAX_COMMAND_TIMEOUT_MS,
    MAX_COMMAND_WAIT_MS, MAX_ITEM_SLOT, MAX_SKILL_SLOT, MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT,
    SkillSlot, SlotSwap, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, WalkTarget,
};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use tokio::{sync::oneshot, time::timeout};
use utoipa::ToSchema;

pub(crate) mod ability;
pub(crate) mod assail;
pub(crate) mod chant;
pub(crate) mod dialog;
pub(crate) mod exchange;
pub(crate) mod group;
pub(crate) mod interaction;
pub(crate) mod legend;
pub(crate) mod movement;
pub(crate) mod player;
pub(crate) mod raw;
pub(crate) mod resync;
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
use movement::validate_destination;
pub(crate) use movement::{
    ActionDirection, Destination, TurnOptions, WalkDestinationOptions, WalkDirectionOptions,
    WalkOptions, turn, walk,
};
pub(crate) use player::{cached_player, inspect_player};
pub(crate) use who::{UserState as WhoUserState, WhoClass, WhoList, WhoPlayer, who};

pub(crate) const ROUTER_CAPACITY: usize = 64;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WORKER_CAPACITY: usize = 16;

const ROUTE_TIMEOUT: Duration = Duration::from_secs(2);
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
    pub(crate) operation: CommandOperation,
    pub(crate) reply: oneshot::Sender<CommandReply>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
// Successful replies retain the complete protocol result until the HTTP task receives it.
#[allow(clippy::large_enum_variant)]
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
    }
}

pub(super) async fn submit_action(
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
