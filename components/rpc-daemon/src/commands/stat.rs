use super::{action_client, submit_action};
use crate::api::{ApiError, ApiState};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use darpc_protocol::{CharacterStat, CommandKind as ProtocolKind};

#[utoipa::path(
    post,
    path = "/clients/{client}/stats/{stat}",
    summary = "Spend a character stat point",
    description = "Spends one available stat point on strength (str), dexterity (dex), intelligence (int), wisdom (wis), or constitution (con).",
    params(("client" = String, Path), ("stat" = String, Path)),
    responses(
        (status = 200, body = super::CommandStatus),
        (status = 202, body = super::CommandStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState),
        (status = 429, body = crate::api::ErrorState),
        (status = 503, body = crate::api::ErrorState),
        (status = 504, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn add(
    State(state): State<ApiState>,
    Path((identifier, stat)): Path<(String, String)>,
) -> Result<(StatusCode, Json<super::CommandStatus>), ApiError> {
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let stat = parse_stat(&stat).ok_or_else(|| {
        bad_request(
            pid,
            "stat must be strength/str, dexterity/dex, intelligence/int, wisdom/wis, or constitution/con",
        )
    })?;
    let stat_points = snapshot
        .character
        .as_ref()
        .map_or(0, |character| character.stats.stat_points);
    if stat_points == 0 {
        return Err(bad_request(pid, "character has no stat points"));
    }
    if !state.reserve_stat_spend(pid) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "stat_spend_too_fast",
            "wait 500 milliseconds before spending another stat point",
            Some(pid),
        ));
    }
    submit_action(&state, pid, identity, ProtocolKind::AddStat(stat)).await
}

fn parse_stat(stat: &str) -> Option<CharacterStat> {
    match stat {
        "strength" | "str" => Some(CharacterStat::Strength),
        "dexterity" | "dex" => Some(CharacterStat::Dexterity),
        "intelligence" | "int" => Some(CharacterStat::Intelligence),
        "wisdom" | "wis" => Some(CharacterStat::Wisdom),
        "constitution" | "con" => Some(CharacterStat::Constitution),
        _ => None,
    }
}

fn bad_request(pid: u32, message: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_action",
        message,
        Some(pid),
    )
}
