use super::*;
use darpc_model::WorldObject;
use darpc_protocol::{GroupCommand, GroupInvitationAction, GroupText};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GroupToggleOptions {
    /// Reopen group invitations after leaving or disbanding a current group.
    #[serde(default = "default_leave_open")]
    #[schema(default = true)]
    leave_open: bool,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GroupInviteOptions {
    /// Player name or visible player object ID.
    #[schema(example = "ZiLo")]
    target: GroupTarget,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
enum GroupTarget {
    Name(String),
    Id(u32),
}

/// Toggles whether invitations are open, or leaves the current group and reopens them.
#[utoipa::path(post, path = "/clients/{client}/group/toggle", params(("client" = String, Path)), request_body = Option<GroupToggleOptions>, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 404, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn toggle(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Option<Json<GroupToggleOptions>>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let request = request.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;
    let leave_open = request
        .map(|Json(request)| request.leave_open)
        .unwrap_or_else(default_leave_open);
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let grouped = snapshot
        .group
        .as_ref()
        .is_some_and(|group| !group.members.is_empty());
    let first = submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Group(GroupCommand::Toggle),
    )
    .await?;
    if !grouped || !leave_open || first.1.0.state != CommandState::Executed {
        return Ok(first);
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Group(GroupCommand::Toggle),
    )
    .await
}

/// Invites a player using the normal client group request.
#[utoipa::path(post, path = "/clients/{client}/group/invite", params(("client" = String, Path)), request_body = GroupInviteOptions, responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 404, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn invite(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<GroupInviteOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let name = resolve_player_name(pid, &snapshot, &request.target)?;
    let target = GroupText::new(&name)
        .ok_or_else(|| bad_request(pid, "the player name cannot be sent as a group request"))?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Group(GroupCommand::Invite(target)),
    )
    .await
}

/// Accepts a currently retained group invitation.
#[utoipa::path(post, path = "/clients/{client}/group/invitations/{invitation_id}/accept", params(("client" = String, Path), ("invitation_id" = u32, Path)), responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 404, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn accept(
    State(state): State<ApiState>,
    Path((identifier, invitation_id)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    respond(
        state,
        identifier,
        invitation_id,
        GroupInvitationAction::Accept,
    )
    .await
}

/// Declines and closes a currently retained group invitation.
#[utoipa::path(post, path = "/clients/{client}/group/invitations/{invitation_id}/decline", params(("client" = String, Path), ("invitation_id" = u32, Path)), responses((status = 200, body = CommandStatus), (status = 202, body = CommandStatus), (status = 400, body = crate::api::ErrorState), (status = 404, body = crate::api::ErrorState), (status = 409, body = crate::api::ErrorState)))]
pub(crate) async fn decline(
    State(state): State<ApiState>,
    Path((identifier, invitation_id)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    respond(
        state,
        identifier,
        invitation_id,
        GroupInvitationAction::Decline,
    )
    .await
}

async fn respond(
    state: ApiState,
    identifier: String,
    invitation_id: u32,
    action: GroupInvitationAction,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    if invitation_id == 0 {
        return Err(bad_request(0, "invitation_id must be greater than zero"));
    }
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let invitation_exists = snapshot.group.as_ref().is_some_and(|group| {
        group
            .invitations
            .iter()
            .any(|invitation| invitation.id == invitation_id)
    });
    if !invitation_exists {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "group_invitation_not_found",
            "the group invitation is no longer open",
            Some(pid),
        ));
    }
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Group(GroupCommand::Respond {
            invitation_id,
            action,
        }),
    )
    .await
}

fn resolve_player_name(
    pid: u32,
    snapshot: &GameSnapshot,
    target: &GroupTarget,
) -> Result<String, ApiError> {
    let wanted = match target {
        GroupTarget::Name(name) => {
            if snapshot
                .character
                .as_ref()
                .and_then(|character| character.name.as_deref())
                .is_some_and(|character_name| character_name.eq_ignore_ascii_case(name))
            {
                return Err(bad_request(pid, "a character cannot invite itself"));
            }
            return Ok(name.clone());
        }
        GroupTarget::Id(wanted) => wanted,
    };

    let objects = snapshot.objects.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "objects_unavailable",
            "visible player state is unavailable",
            Some(pid),
        )
    })?;
    let player = objects
        .iter()
        .find(|object| matches!(object, WorldObject::Player { id, .. } if id == wanted));
    let Some(WorldObject::Player { id, name, .. }) = player else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "player_not_found",
            "the requested player is not visible",
            Some(pid),
        ));
    };
    if snapshot
        .character
        .as_ref()
        .and_then(|character| character.id)
        == Some(*id)
    {
        return Err(bad_request(pid, "a character cannot invite itself"));
    }
    name.clone().filter(|name| !name.is_empty()).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "player_name_unavailable",
            "the visible player does not have a known name",
            Some(pid),
        )
    })
}

fn bad_request(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_group_request",
        message,
        (pid != 0).then_some(pid),
    )
}

const fn default_leave_open() -> bool {
    true
}
