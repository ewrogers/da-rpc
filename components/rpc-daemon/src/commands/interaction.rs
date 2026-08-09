use super::*;
use darpc_model::{EquipmentSlot, InventoryItem};
use darpc_protocol::{GoldTransfer, ItemSlot, ItemTransfer, TilePosition, TransferTarget};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UseItemOptions {
    slot: Option<u8>,
    name: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DropItemOptions {
    /// Select the item by its one-based inventory slot.
    slot: Option<u8>,
    /// Select the item by its case-insensitive inventory name.
    name: Option<String>,
    #[serde(default = "one")]
    quantity: u32,
    destination: Option<Destination>,
    target: Option<ObjectTarget>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DropGoldOptions {
    amount: u32,
    destination: Option<Destination>,
    target: Option<ObjectTarget>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PickupItemOptions {
    position: Destination,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnequipOptions {
    slot: EquipmentSlotName,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmoteOptions {
    /// Select any emote exposed by the client UI by its numeric request code.
    code: Option<u8>,
    /// Select a confirmed emote by its case-insensitive player-facing name.
    #[schema(example = "wave")]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
enum ObjectTarget {
    Name(String),
    Id(u32),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum EquipmentSlotName {
    Weapon,
    Armor,
    Shield,
    Helmet,
    Earrings,
    Necklace,
    LeftRing,
    RightRing,
    LeftGauntlet,
    RightGauntlet,
    Belt,
    Greaves,
    Boots,
    Accessory1,
    Overcoat,
    OverHelm,
    Accessory2,
    Accessory3,
}

#[utoipa::path(post, path = "/clients/{client}/items/use", params(("client" = String, Path)), request_body = UseItemOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn use_item(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<UseItemOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let slot = resolve_item(pid, &snapshot, request.slot, request.name.as_deref())?.slot;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::UseItem(item_slot(pid, slot)?),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/items/drop", params(("client" = String, Path)), request_body = DropItemOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn drop_item(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DropItemOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let item = resolve_item(pid, &snapshot, request.slot, request.name.as_deref())?;
    validate_item_quantity(pid, item, request.quantity)?;
    let target =
        resolve_transfer_target(pid, &snapshot, request.destination, request.target.as_ref())?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::DropItem(ItemTransfer {
            slot: item_slot(pid, item.slot)?,
            quantity: request.quantity,
            target,
        }),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/gold/drop", params(("client" = String, Path)), request_body = DropGoldOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn drop_gold(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DropGoldOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    if request.amount == 0 {
        return Err(bad_request(pid, "amount must be greater than zero"));
    }
    let gold = snapshot.character.as_ref().map_or(0, |value| value.gold);
    if request.amount > gold {
        return Err(bad_request(
            pid,
            "amount exceeds the character's current gold",
        ));
    }
    let target =
        resolve_transfer_target(pid, &snapshot, request.destination, request.target.as_ref())?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::DropGold(GoldTransfer {
            amount: request.amount,
            target,
        }),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/items/pickup", params(("client" = String, Path)), request_body = PickupItemOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn pickup_item(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<PickupItemOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    validate_destination(pid, &snapshot, request.position)?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::PickupItem(TilePosition {
            x: request.position.x,
            y: request.position.y,
        }),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/equipment/unequip", params(("client" = String, Path)), request_body = UnequipOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn unequip(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<UnequipOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Unequip(request.slot.into()),
    )
    .await
}

#[utoipa::path(post, path = "/clients/{client}/emote", params(("client" = String, Path)), request_body = EmoteOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn emote(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<EmoteOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, _) = action_client(&state, &identifier)?;
    if request.code.is_some() == request.name.is_some() {
        return Err(bad_request(pid, "provide exactly one of code or name"));
    }
    let code = if let Some(code) = request.code {
        code
    } else {
        emote_code(request.name.as_deref().unwrap_or_default())
            .ok_or_else(|| bad_request(pid, "the emote name is not recognized"))?
    };
    if !is_client_emote_code(code) {
        return Err(bad_request(
            pid,
            "code is not an emote exposed by the client UI",
        ));
    }
    submit_action(&state, pid, identity, ProtocolKind::Emote(code)).await
}

fn resolve_item<'a>(
    pid: u32,
    snapshot: &'a GameSnapshot,
    slot: Option<u8>,
    name: Option<&str>,
) -> Result<&'a InventoryItem, ApiError> {
    if slot.is_some() == name.is_some() {
        return Err(bad_request(pid, "provide exactly one of slot or name"));
    }
    let inventory = snapshot
        .character
        .as_ref()
        .and_then(|character| character.inventory.as_deref())
        .ok_or_else(|| conflict(pid, "inventory is unavailable"))?;
    inventory
        .iter()
        .find(|item| {
            slot == Some(item.slot)
                || name.is_some_and(|name| {
                    item.name
                        .as_deref()
                        .is_some_and(|item_name| item_name.eq_ignore_ascii_case(name))
                })
        })
        .ok_or_else(|| bad_request(pid, "the item was not found in the current inventory"))
}

fn validate_item_quantity(pid: u32, item: &InventoryItem, quantity: u32) -> Result<(), ApiError> {
    if quantity == 0 || quantity > item.quantity || (!item.can_stack && quantity != 1) {
        return Err(bad_request(
            pid,
            "quantity is invalid for the selected item",
        ));
    }
    Ok(())
}

fn resolve_transfer_target(
    pid: u32,
    snapshot: &GameSnapshot,
    destination: Option<Destination>,
    target: Option<&ObjectTarget>,
) -> Result<TransferTarget, ApiError> {
    match (destination, target) {
        (Some(destination), None) => {
            validate_destination(pid, snapshot, destination)?;
            Ok(TransferTarget::Tile(TilePosition {
                x: destination.x,
                y: destination.y,
            }))
        }
        (None, Some(target)) => resolve_object_target(pid, snapshot, target),
        _ => Err(bad_request(
            pid,
            "provide exactly one of destination or target",
        )),
    }
}

fn resolve_object_target(
    pid: u32,
    snapshot: &GameSnapshot,
    target: &ObjectTarget,
) -> Result<TransferTarget, ApiError> {
    let self_id = snapshot
        .character
        .as_ref()
        .and_then(|character| character.id);
    let objects = snapshot.objects.as_deref().unwrap_or_default();
    let object = match target {
        ObjectTarget::Id(id) => objects.iter().find(|object| {
            object.id() == *id
                && matches!(
                    object,
                    WorldObject::Player { .. } | WorldObject::Creature { .. }
                )
        }),
        ObjectTarget::Name(name) => objects
            .iter()
            .find(|object| matches_name(object, name, true))
            .or_else(|| {
                objects
                    .iter()
                    .find(|object| matches_name(object, name, false))
            }),
    }
    .filter(|object| Some(object.id()) != self_id)
    .ok_or_else(|| bad_request(pid, "the target human or creature is not visible"))?;
    NonZeroU32::new(object.id())
        .map(TransferTarget::Object)
        .ok_or_else(|| bad_request(pid, "the target object ID is invalid"))
}

fn matches_name(object: &WorldObject, name: &str, human: bool) -> bool {
    match object {
        WorldObject::Player {
            name: candidate, ..
        } if human => candidate
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name)),
        WorldObject::Creature {
            name: candidate, ..
        } if !human => candidate
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name)),
        _ => false,
    }
}

fn item_slot(pid: u32, slot: u8) -> Result<ItemSlot, ApiError> {
    ItemSlot::new(slot).ok_or_else(|| bad_request(pid, "item slot is outside 1 through 59"))
}

fn bad_request(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_action",
        message,
        Some(pid),
    )
}

fn conflict(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "client_state_unavailable",
        message,
        Some(pid),
    )
}

const fn one() -> u32 {
    1
}

impl From<EquipmentSlotName> for EquipmentSlot {
    fn from(slot: EquipmentSlotName) -> Self {
        match slot {
            EquipmentSlotName::Weapon => Self::Weapon,
            EquipmentSlotName::Armor => Self::Armor,
            EquipmentSlotName::Shield => Self::Shield,
            EquipmentSlotName::Helmet => Self::Helmet,
            EquipmentSlotName::Earrings => Self::Earrings,
            EquipmentSlotName::Necklace => Self::Necklace,
            EquipmentSlotName::LeftRing => Self::LeftRing,
            EquipmentSlotName::RightRing => Self::RightRing,
            EquipmentSlotName::LeftGauntlet => Self::LeftGauntlet,
            EquipmentSlotName::RightGauntlet => Self::RightGauntlet,
            EquipmentSlotName::Belt => Self::Belt,
            EquipmentSlotName::Greaves => Self::Greaves,
            EquipmentSlotName::Boots => Self::Boots,
            EquipmentSlotName::Accessory1 => Self::Accessory1,
            EquipmentSlotName::Overcoat => Self::Overcoat,
            EquipmentSlotName::OverHelm => Self::OverHelm,
            EquipmentSlotName::Accessory2 => Self::Accessory2,
            EquipmentSlotName::Accessory3 => Self::Accessory3,
        }
    }
}
