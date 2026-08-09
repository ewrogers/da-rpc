use super::*;
use darpc_model::{CreatureKind, DialogInteraction, WorldObject};
use darpc_protocol::{DialogAction, DialogCommand, DialogText};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InteractOptions {
    /// Visible Mundane name or object ID.
    #[schema(example = "Beggar")]
    target: ObjectTarget,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
enum ObjectTarget {
    Name(String),
    Id(u32),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DialogSelectOptions {
    /// Revision from the current dialog state.
    #[schema(example = 7)]
    revision: u32,
    /// Zero-based displayed row index.
    #[schema(example = 0)]
    index: u16,
    /// Item quantity. Defaults to one.
    #[schema(example = 1)]
    #[serde(default = "one")]
    quantity: u8,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DialogInputOptions {
    /// Revision from the current dialog state.
    #[schema(example = 7)]
    revision: u32,
    /// Nonempty ASCII answer within the current prompt's byte limit.
    #[schema(example = "ZiLo")]
    input: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DialogRevisionOptions {
    /// Revision from the current dialog state.
    #[schema(example = 7)]
    revision: u32,
}

/// Starts a conversation with a visible Mundane through the native client.
#[utoipa::path(post, path = "/clients/{client}/interact", params(("client" = String, Path)), request_body = InteractOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn interact(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<InteractOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let id = resolve_target(pid, &snapshot, &request.target)?;
    submit_action(&state, pid, identity, ProtocolKind::Interact(id)).await
}

/// Selects a zero-based row from the current dialog.
#[utoipa::path(post, path = "/clients/{client}/dialog/select", params(("client" = String, Path)), request_body = DialogSelectOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn select(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DialogSelectOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let dialog = dialog(pid, &snapshot, request.revision, false)?;
    if request.quantity == 0
        || !valid_selection(&dialog.interaction, request.index, request.quantity)
    {
        return Err(bad_request(
            pid,
            "index or quantity is invalid for the current dialog",
        ));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Dialog(DialogCommand {
            revision: request.revision,
            action: DialogAction::Select {
                index: request.index,
                quantity: request.quantity,
            },
        }),
    )
    .await
}

/// Submits text to the current dialog prompt.
#[utoipa::path(post, path = "/clients/{client}/dialog/input", params(("client" = String, Path)), request_body = DialogInputOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn input(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DialogInputOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let dialog = dialog(pid, &snapshot, request.revision, false)?;
    let DialogInteraction::Input(input) = &dialog.interaction else {
        return Err(bad_request(
            pid,
            "the current dialog does not accept text input",
        ));
    };
    if request.input.is_empty()
        || request.input.len() > usize::from(input.maximum_bytes)
        || request.input.as_bytes().contains(&0)
    {
        return Err(bad_request(
            pid,
            "input is empty or exceeds the current dialog limit",
        ));
    }
    let input = DialogText::new(&request.input)
        .ok_or_else(|| bad_request(pid, "input is invalid for a client dialog"))?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Dialog(DialogCommand {
            revision: request.revision,
            action: DialogAction::Input(input),
        }),
    )
    .await
}

/// Navigates to the previous pursuit page when available.
#[utoipa::path(post, path = "/clients/{client}/dialog/previous", params(("client" = String, Path)), request_body = DialogRevisionOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn previous(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DialogRevisionOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    navigate(state, identifier, request, true).await
}

/// Navigates to the next pursuit page when available.
#[utoipa::path(post, path = "/clients/{client}/dialog/next", params(("client" = String, Path)), request_body = DialogRevisionOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn next(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DialogRevisionOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    navigate(state, identifier, request, false).await
}

/// Closes the current dialog through its native client action.
#[utoipa::path(post, path = "/clients/{client}/dialog/close", params(("client" = String, Path)), request_body = DialogRevisionOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn close(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<DialogRevisionOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let dialog = dialog(pid, &snapshot, request.revision, true)?;
    if !dialog.navigation.close {
        return Err(bad_request(pid, "the current dialog cannot be closed"));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Dialog(DialogCommand {
            revision: request.revision,
            action: DialogAction::Close,
        }),
    )
    .await
}

async fn navigate(
    state: ApiState,
    identifier: String,
    request: Result<Json<DialogRevisionOptions>, JsonRejection>,
    previous: bool,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let dialog = dialog(pid, &snapshot, request.revision, false)?;
    let allowed = if previous {
        dialog.navigation.previous
    } else {
        dialog.navigation.next
    };
    if !allowed {
        return Err(bad_request(pid, "the requested navigation is unavailable"));
    }
    let action = if previous {
        DialogAction::Previous
    } else {
        DialogAction::Next
    };
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Dialog(DialogCommand {
            revision: request.revision,
            action,
        }),
    )
    .await
}

fn dialog(
    pid: u32,
    snapshot: &GameSnapshot,
    revision: u32,
    allow_pending: bool,
) -> Result<&darpc_model::DialogState, ApiError> {
    let dialog = snapshot
        .dialog
        .as_ref()
        .ok_or_else(|| conflict(pid, "dialog_unavailable", "no NPC dialog is currently open"))?;
    if dialog.revision != revision {
        return Err(conflict(
            pid,
            "stale_dialog",
            "the dialog changed after the supplied revision",
        ));
    }
    if dialog.response_pending && !allow_pending {
        return Err(conflict(
            pid,
            "dialog_pending",
            "the current dialog is waiting for the server",
        ));
    }
    Ok(dialog)
}

fn valid_selection(interaction: &DialogInteraction, index: u16, quantity: u8) -> bool {
    match interaction {
        DialogInteraction::Choices(values) => values.iter().any(|value| value.index == index),
        DialogInteraction::Items(values) => values
            .iter()
            .find(|value| value.index == index)
            .is_some_and(|value| {
                value
                    .available_quantity
                    .is_none_or(|available| quantity <= available)
            }),
        DialogInteraction::Inventory(values)
        | DialogInteraction::Spells(values)
        | DialogInteraction::Skills(values) => values.iter().any(|value| value.index == index),
        _ => false,
    }
}

fn resolve_target(
    pid: u32,
    snapshot: &GameSnapshot,
    target: &ObjectTarget,
) -> Result<NonZeroU32, ApiError> {
    let objects = snapshot.objects.as_deref().unwrap_or_default();
    let object = match target {
        ObjectTarget::Id(id) => objects.iter().find(|value| value.id() == *id && matches!(value, WorldObject::Creature { kind: CreatureKind::Npc, .. })),
        ObjectTarget::Name(name) => objects.iter().find(|value| matches!(value, WorldObject::Creature { kind: CreatureKind::Npc, name: Some(actual), .. } if actual.eq_ignore_ascii_case(name))),
    }.ok_or_else(|| bad_request(pid, "the target mundane is not visible"))?;
    NonZeroU32::new(object.id())
        .ok_or_else(|| bad_request(pid, "the target has an invalid object ID"))
}

fn bad_request(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_dialog_action",
        message,
        Some(pid),
    )
}
fn conflict(pid: u32, code: &'static str, message: &str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, code, message, Some(pid))
}
const fn one() -> u8 {
    1
}
