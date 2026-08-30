use super::*;
use darpc_protocol::{
    BulletinAction, BulletinBody, BulletinCommand, BulletinComposeKind, BulletinNavigation,
    BulletinOpen, BulletinRecipient, BulletinSubject,
};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BulletinActionRequest {
    /// Use zero for open requests; otherwise use the current bulletin revision.
    revision: u32,
    action: BulletinActionOptions,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum BulletinActionOptions {
    OpenServerList,
    OpenWorldBoard {
        x: u16,
        y: u16,
    },
    OpenSection {
        section_id: u16,
    },
    SelectSection {
        section_id: u16,
    },
    OpenEntry {
        entry_id: i16,
    },
    SelectEntry {
        entry_id: i16,
    },
    LoadOlder,
    Scroll {
        position: i32,
    },
    Back,
    Forward,
    PreviousEntry,
    NextEntry,
    BeginBoardPost,
    BeginPlayerMail,
    BeginReply,
    UpdateBoardPost {
        subject: String,
        body: String,
    },
    UpdatePlayerMail {
        recipient: String,
        subject: String,
        body: String,
    },
    SubmitCompose,
    DeleteEntry {
        entry_id: i16,
    },
    HighlightEntry {
        entry_id: i16,
    },
    Close,
}

/// Performs one revision-guarded action against the native bulletin session.
#[utoipa::path(
    post,
    path = "/clients/{client}/bulletin/actions",
    params(("client" = String, Path)),
    request_body = BulletinActionRequest,
    responses(
        (status = 200, body = CommandStatus),
        (status = 202, body = CommandStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState),
        (status = 429, body = crate::api::ErrorState),
        (status = 503, body = crate::api::ErrorState),
        (status = 504, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn action(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<BulletinActionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = super::fresh_snapshot(&state, &identifier).await?;
    if snapshot.lifecycle != ClientLifecycle::InGame {
        return Err(conflict(
            pid,
            "client_not_in_game",
            "the client is not currently in game",
        ));
    }
    let opening = matches!(
        request.action,
        BulletinActionOptions::OpenServerList | BulletinActionOptions::OpenWorldBoard { .. }
    );
    if opening {
        if request.revision != 0 {
            return Err(bad_request(
                pid,
                "invalid_bulletin_revision",
                "bulletin open requests require revision zero",
            ));
        }
    } else {
        let bulletin = snapshot.active_bulletin.as_ref().ok_or_else(|| {
            conflict(
                pid,
                "bulletin_unavailable",
                "no native bulletin session is active",
            )
        })?;
        if bulletin.revision != request.revision {
            return Err(conflict(
                pid,
                "stale_bulletin",
                "the bulletin state changed after the supplied revision",
            ));
        }
    }
    let action = protocol_action(pid, request.action)?;
    submit_action(
        &state,
        pid,
        identity,
        ProtocolKind::Bulletin(BulletinCommand {
            revision: request.revision,
            action,
        }),
    )
    .await
}

fn protocol_action(pid: u32, action: BulletinActionOptions) -> Result<BulletinAction, ApiError> {
    Ok(match action {
        BulletinActionOptions::OpenServerList => BulletinAction::Open(BulletinOpen::ServerList),
        BulletinActionOptions::OpenWorldBoard { x, y } => {
            BulletinAction::Open(BulletinOpen::WorldTile { x, y })
        }
        BulletinActionOptions::OpenSection { section_id } => {
            BulletinAction::OpenSection { section_id }
        }
        BulletinActionOptions::SelectSection { section_id } => {
            BulletinAction::SelectSection { section_id }
        }
        BulletinActionOptions::OpenEntry { entry_id } => BulletinAction::OpenEntry { entry_id },
        BulletinActionOptions::SelectEntry { entry_id } => BulletinAction::SelectEntry { entry_id },
        BulletinActionOptions::LoadOlder => BulletinAction::LoadOlder,
        BulletinActionOptions::Scroll { position } => BulletinAction::Scroll { position },
        BulletinActionOptions::Back => BulletinAction::Navigate(BulletinNavigation::Back),
        BulletinActionOptions::Forward => BulletinAction::Navigate(BulletinNavigation::Forward),
        BulletinActionOptions::PreviousEntry => {
            BulletinAction::Navigate(BulletinNavigation::PreviousEntry)
        }
        BulletinActionOptions::NextEntry => BulletinAction::Navigate(BulletinNavigation::NextEntry),
        BulletinActionOptions::BeginBoardPost => {
            BulletinAction::BeginCompose(BulletinComposeKind::BoardPost)
        }
        BulletinActionOptions::BeginPlayerMail => {
            BulletinAction::BeginCompose(BulletinComposeKind::PlayerMail)
        }
        BulletinActionOptions::BeginReply => {
            BulletinAction::BeginCompose(BulletinComposeKind::Reply)
        }
        BulletinActionOptions::UpdateBoardPost { subject, body } => {
            BulletinAction::UpdateBoardPost {
                subject: subject_value(pid, &subject)?,
                body: body_value(pid, &body)?,
            }
        }
        BulletinActionOptions::UpdatePlayerMail {
            recipient,
            subject,
            body,
        } => BulletinAction::UpdatePlayerMail {
            recipient: BulletinRecipient::new(&recipient).ok_or_else(|| {
                bad_request(
                    pid,
                    "invalid_bulletin_recipient",
                    "recipient must be ASCII, NUL-free, and at most 15 bytes",
                )
            })?,
            subject: subject_value(pid, &subject)?,
            body: body_value(pid, &body)?,
        },
        BulletinActionOptions::SubmitCompose => BulletinAction::SubmitCompose,
        BulletinActionOptions::DeleteEntry { entry_id } => BulletinAction::DeleteEntry { entry_id },
        BulletinActionOptions::HighlightEntry { entry_id } => {
            BulletinAction::HighlightEntry { entry_id }
        }
        BulletinActionOptions::Close => BulletinAction::Close,
    })
}

fn subject_value(pid: u32, value: &str) -> Result<BulletinSubject, ApiError> {
    BulletinSubject::new(value).ok_or_else(|| {
        bad_request(
            pid,
            "invalid_bulletin_subject",
            "subject must be ASCII, NUL-free, and at most 60 bytes",
        )
    })
}

fn body_value(pid: u32, value: &str) -> Result<BulletinBody, ApiError> {
    BulletinBody::new(value).ok_or_else(|| {
        bad_request(
            pid,
            "invalid_bulletin_body",
            "body must be ASCII, NUL-free, and at most 3000 bytes",
        )
    })
}

fn bad_request(pid: u32, code: &'static str, message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, code, message, Some(pid))
}

fn conflict(pid: u32, code: &'static str, message: &str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, code, message, Some(pid))
}
