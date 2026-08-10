use super::*;
use darpc_protocol::ChantText;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChantOptions {
    /// Text sent verbatim as a spell chant.
    text: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemChantOptions {
    /// Exact case-sensitive item name, including punctuation and spacing.
    name: String,
}

macro_rules! item_chant {
    ($handler:ident, $path:literal, $build:expr) => {
        #[utoipa::path(
                                            post,
                                            path = $path,
                                            params(("client" = String, Path)),
                                            request_body = ItemChantOptions,
                                            responses(
                                                (status = 200, body = CommandStatus),
                                                (status = 202, body = CommandStatus),
                                                (status = 400, body = crate::api::ErrorState),
                                                (status = 409, body = crate::api::ErrorState)
                                            )
                                        )]
        pub(crate) async fn $handler(
            State(state): State<ApiState>,
            Path(identifier): Path<String>,
            request: Result<Json<ItemChantOptions>, JsonRejection>,
        ) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
            let Json(request) = action_request(request)?;
            submit(&state, &identifier, $build(&request.name)).await
        }
    };
}

#[utoipa::path(
    post,
    path = "/clients/{client}/chant",
    params(("client" = String, Path)),
    request_body = ChantOptions,
    responses(
        (status = 200, body = CommandStatus),
        (status = 202, body = CommandStatus),
        (status = 400, body = crate::api::ErrorState),
        (status = 409, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn chant(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<ChantOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    submit(&state, &identifier, ChantText::new(&request.text)).await
}

item_chant!(sell, "/clients/{client}/items/sell", ChantText::sell);
item_chant!(
    sell_all,
    "/clients/{client}/items/sell-all",
    ChantText::sell_all
);
item_chant!(
    deposit,
    "/clients/{client}/items/deposit",
    ChantText::deposit
);
item_chant!(
    withdraw,
    "/clients/{client}/items/withdraw",
    ChantText::withdraw
);
item_chant!(repair, "/clients/{client}/items/repair", ChantText::repair);

#[utoipa::path(
    post,
    path = "/clients/{client}/items/repair-all",
    params(("client" = String, Path)),
    responses(
        (status = 200, body = CommandStatus),
        (status = 202, body = CommandStatus),
        (status = 409, body = crate::api::ErrorState)
    )
)]
pub(crate) async fn repair_all(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    submit(&state, &identifier, Some(ChantText::repair_all())).await
}

async fn submit(
    state: &ApiState,
    identifier: &str,
    text: Option<ChantText>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let (pid, identity, _) = action_client(state, identifier)?;
    let text = text.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_chant_text",
            "text must be nonempty ASCII that fits the chant packet",
            Some(pid),
        )
    })?;
    submit_action(state, pid, identity, ProtocolKind::Chant(text)).await
}
