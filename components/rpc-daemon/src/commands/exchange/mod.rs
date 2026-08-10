use super::*;
use darpc_protocol::ExchangeCommand;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddExchangeItemOptions {
    /// Select the item by its one-based inventory slot.
    slot: Option<u8>,
    /// Select the item by its case-insensitive inventory name.
    name: Option<String>,
    /// Stack quantity. Defaults to one and cannot exceed 255.
    #[serde(default = "one")]
    quantity: u32,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetExchangeGoldOptions {
    /// Gold to offer. Must be nonzero and no greater than current gold.
    amount: u32,
}

#[utoipa::path(post, path = "/clients/{client}/exchange/items", params(("client" = String, Path)), request_body = AddExchangeItemOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn add_item(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<AddExchangeItemOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let exchange = active_exchange(pid, &snapshot)?;
    if exchange.local.accepted || exchange.other.accepted {
        return Err(conflict(pid, "the exchange offer is already accepted"));
    }
    if exchange.local.items.len() >= darpc_protocol::MAX_EXCHANGE_ITEMS {
        return Err(conflict(pid, "the local exchange offer is full"));
    }
    let item =
        super::interaction::resolve_item(pid, &snapshot, request.slot, request.name.as_deref())?;
    super::interaction::validate_item_quantity(pid, item, request.quantity)?;
    let quantity = u8::try_from(request.quantity)
        .map_err(|_| bad_exchange_request(pid, "quantity cannot exceed 255"))?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Exchange(ExchangeCommand::AddItem {
            slot: super::interaction::item_slot(pid, item.slot)?,
            quantity,
        }),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/exchange/gold", params(("client" = String, Path)), request_body = SetExchangeGoldOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn set_gold(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<SetExchangeGoldOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let exchange = active_exchange(pid, &snapshot)?;
    if exchange.local.accepted || exchange.other.accepted {
        return Err(conflict(pid, "the exchange offer is already accepted"));
    }
    if exchange.local.gold != 0 {
        return Err(conflict(pid, "gold has already been set for this exchange"));
    }
    let available = snapshot
        .character
        .as_ref()
        .map(|character| character.gold)
        .ok_or_else(|| conflict(pid, "character state is unavailable"))?;
    if request.amount == 0 || request.amount > available {
        return Err(bad_exchange_request(
            pid,
            "amount must be nonzero and no greater than current gold",
        ));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Exchange(ExchangeCommand::SetGold(request.amount)),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/exchange/accept", params(("client" = String, Path)), responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn accept(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    submit_simple(&state, &identifier, ExchangeCommand::Accept).await
}

#[utoipa::path(post, path = "/clients/{client}/exchange/cancel", params(("client" = String, Path)), responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn cancel(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    submit_simple(&state, &identifier, ExchangeCommand::Cancel).await
}

async fn submit_simple(
    state: &ApiState,
    identifier: &str,
    command: ExchangeCommand,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let (pid, identity, snapshot) = action_client(state, identifier)?;
    active_exchange(pid, &snapshot)?;
    submit_action(state, pid, identity, ProtocolKind::Exchange(command)).await
}

fn active_exchange(
    pid: u32,
    snapshot: &GameSnapshot,
) -> Result<&darpc_model::ExchangeState, ApiError> {
    snapshot
        .exchange
        .as_ref()
        .ok_or_else(|| conflict(pid, "no player exchange is open"))
}

fn bad_exchange_request(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_exchange_request",
        message,
        Some(pid),
    )
}

fn conflict(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "exchange_conflict",
        message,
        Some(pid),
    )
}

const fn one() -> u32 {
    1
}
