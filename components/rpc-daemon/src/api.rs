use crate::{
    commands::{CommandCall, ROUTER_CAPACITY},
    event::DaemonEvent,
    lifecycle::{
        LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
        LifecycleOutcome, ManagementError, ServerEndpoint as ManagedServerEndpoint,
    },
    registry::{
        ClientIdentity as RegistryClientIdentity, ClientSnapshot as RegistryClientSnapshot,
        ClientSnapshotStatus, ConnectionEvent, RegistrySnapshot, architecture, hex,
    },
    snapshot::{
        CharacterClass as SnapshotCharacterClass, CharacterGender, CharacterModifiers,
        CharacterProgression, CharacterStats, CharacterStatus, CharacterVitals,
        ClientLifecycle as SnapshotClientLifecycle, CooldownStatus, Effect, EffectDuration,
        Effects, Element, Equipment, EquipmentItem, EquipmentSlot, GameStatus, Inventory,
        InventoryItem, MapLocation, ObservationMetadata, Skill, Skillbook, Spell, SpellTargetType,
        Spellbook,
    },
    stream::{self, ClientEvent, PublishedEvent},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{Method, Request, StatusCode, header},
    middleware::{Next, from_fn},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use darpc_game_client::CLIENT_VERSION;
use darpc_protocol::{Hello, protocol_version_major, protocol_version_minor};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    sync::{
        Arc, RwLock,
        mpsc::{self, Sender, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};
use tokio::sync::{broadcast, oneshot};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

const SWAGGER_INDEX: &str = include_str!("../assets/swagger.html");
const SWAGGER_THEME: &str = include_str!("../assets/swagger-ayu.css");
const DEFAULT_SERVER_PORT: u16 = 2610;
const MAX_REQUEST_BODY: usize = 4 * 1024;

#[derive(Clone)]
pub(crate) struct ApiState {
    snapshot: Arc<RwLock<Arc<RegistrySnapshot>>>,
    lifecycle: Arc<dyn LifecycleControl>,
    events: Sender<DaemonEvent>,
    commands: SyncSender<CommandCall>,
    published_events: broadcast::Sender<PublishedEvent>,
}

impl ApiState {
    #[must_use]
    pub(crate) fn new(
        snapshot: RegistrySnapshot,
        lifecycle: Arc<dyn LifecycleControl>,
        events: Sender<DaemonEvent>,
    ) -> Self {
        let (published_events, _) = broadcast::channel(stream::EVENT_CHANNEL_CAPACITY);
        let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
        drop(command_receiver);
        Self {
            snapshot: Arc::new(RwLock::new(Arc::new(snapshot))),
            lifecycle,
            events,
            commands,
            published_events,
        }
    }

    #[must_use]
    pub(crate) fn with_command_sender(mut self, commands: SyncSender<CommandCall>) -> Self {
        self.commands = commands;
        self
    }

    pub(crate) fn publish(&self, snapshot: RegistrySnapshot) {
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = Arc::new(snapshot);
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn publish_connection_event(&self, event: &ConnectionEvent) {
        match event {
            ConnectionEvent::StateEvents {
                pid,
                identity,
                events,
            } => {
                for state_event in events {
                    let _ = self.published_events.send(PublishedEvent::State {
                        pid: *pid,
                        identity: *identity,
                        event: state_event.clone(),
                    });
                }
            }
            ConnectionEvent::Disconnected {
                pid,
                identity: Some(identity),
                reason,
            }
            | ConnectionEvent::Incompatible {
                pid,
                identity: Some(identity),
                reason,
            } => {
                let _ = self.published_events.send(PublishedEvent::Closed {
                    pid: *pid,
                    identity: *identity,
                    reason: reason.clone(),
                });
            }
            _ => {}
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<PublishedEvent> {
        self.published_events.subscribe()
    }

    pub(crate) fn snapshot(&self) -> Arc<RegistrySnapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn emit(&self, event: DaemonEvent) -> Result<(), ApiError> {
        self.events.send(event).map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "daemon_unavailable",
                "daemon state manager is unavailable",
                None,
            )
        })
    }

    pub(crate) fn route_command(
        &self,
        pid: u32,
        identity: RegistryClientIdentity,
        operation: darpc_protocol::CommandOperation,
    ) -> Result<oneshot::Receiver<crate::commands::CommandReply>, ApiError> {
        let (reply, receiver) = oneshot::channel();
        let call = CommandCall {
            pid,
            identity,
            operation,
            reply,
        };
        match self.commands.try_send(call) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "command_router_full",
                    "the bounded daemon command router is full",
                    Some(pid),
                ));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "daemon_unavailable",
                    "daemon command routing is unavailable",
                    Some(pid),
                ));
            }
        }
        self.emit(DaemonEvent::CommandsReady)?;
        Ok(receiver)
    }
}

pub(crate) fn start(port: u16, state: ApiState) -> io::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;

    thread::Builder::new()
        .name("darpcd-http".into())
        .spawn(move || {
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(error) => {
                        eprintln!("darpcd: HTTP listener failed: {error}");
                        return;
                    }
                };
                if let Err(error) = axum::serve(listener, router(state)).await {
                    eprintln!("darpcd: HTTP server failed: {error}");
                }
            });
        })
}

fn router(state: ApiState) -> Router {
    Router::<ApiState>::new()
        .route("/health", get(health))
        .route("/clients", get(clients))
        .route("/clients/{client}/status", get(client_status))
        .route("/clients/{client}/inventory", get(client_inventory))
        .route("/clients/{client}/equipment", get(client_equipment))
        .route("/clients/{client}/spellbook", get(client_spellbook))
        .route("/clients/{client}/skillbook", get(client_skillbook))
        .route("/clients/{client}/effects", get(client_effects))
        .route("/clients/{client}/events", get(client_events))
        .route(
            "/clients/{client}/commands/diagnostic",
            post(crate::commands::diagnostic),
        )
        .route(
            "/clients/{client}/commands/{command_id}",
            get(crate::commands::status).delete(crate::commands::cancel),
        )
        .route("/clients/launch", post(launch))
        .route("/clients/{client}/load", post(load))
        .route("/clients/{client}/unload", post(unload))
        .route("/docs", get(swagger_redirect))
        .route("/docs/", get(swagger_index))
        .route("/docs/ayu.css", get(swagger_theme))
        .merge(SwaggerUi::new("/docs/assets").url("/openapi.json", openapi()))
        .layer(from_fn(reject_request_body))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY))
        .with_state(state)
}

async fn swagger_redirect() -> Redirect {
    Redirect::to("/docs/")
}

async fn swagger_index() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(SWAGGER_INDEX))
}

async fn swagger_theme() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        SWAGGER_THEME,
    )
}

async fn reject_request_body(request: Request<Body>, next: Next) -> Response {
    if request.method() == Method::POST
        && (request.uri().path() == "/clients/launch"
            || request.uri().path().ends_with("/commands/diagnostic"))
    {
        return next.run(request).await;
    }
    if request.headers().contains_key(header::TRANSFER_ENCODING) {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if let Some(length) = request.headers().get(header::CONTENT_LENGTH) {
        let Some(length) = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if length != 0 {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
    }
    next.run(request).await
}

fn openapi() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    document.info.title = "daRPC API".into();
    document
}

#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "The daemon HTTP server is available", body = HealthState))
)]
async fn health() -> Json<HealthState> {
    Json(HealthState {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[utoipa::path(
    get,
    path = "/clients",
    responses((status = 200, description = "Configured client targets and their current connection state", body = ClientList))
)]
async fn clients(State(state): State<ApiState>) -> Json<ClientList> {
    let snapshot = state.snapshot();
    Json(ClientList::from(snapshot.as_ref()))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/status",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest character, map, and lifecycle status", body = GameStatus),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_status(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<GameStatus>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(GameStatus::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/inventory",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest inventory observation", body = Inventory),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_inventory(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Inventory>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Inventory::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/equipment",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest equipment observation", body = Equipment),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_equipment(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Equipment>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Equipment::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/spellbook",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest spellbook observation", body = Spellbook),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_spellbook(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Spellbook>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Spellbook::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/skillbook",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest skillbook observation", body = Skillbook),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_skillbook(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Skillbook>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Skillbook::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/effects",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The latest spell effect observation", body = Effects),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "No client observation is currently available", body = ErrorState)
    )
)]
async fn client_effects(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Effects>, ApiError> {
    let registry = state.snapshot();
    let (pid, snapshot) = resolve_game_snapshot(&registry, &identifier)?;
    Ok(Json(Effects::from_model(pid, snapshot)))
}

#[utoipa::path(
    get,
    path = "/clients/{client}/events",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "Server-Sent Events stream beginning with stream.ready. Each frame has an event name, sequence ID, and a ClientEvent JSON envelope in data.", body = ClientEvent, content_type = "text/event-stream"),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 503, description = "The client is not connected with a current observation", body = ErrorState)
    )
)]
async fn client_events(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<Response, ApiError> {
    let receiver = state.subscribe();
    let registry = state.snapshot();
    let client = resolve_client(&registry, &identifier)?;
    if client.status != ClientSnapshotStatus::Connected {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the client is not currently connected",
            Some(client.pid),
        ));
    }
    let identity = client.identity.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the connected client identity is unavailable",
            Some(client.pid),
        )
    })?;
    let snapshot = client.game_snapshot.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_stream_unavailable",
            "the client has not published an observation yet",
            Some(client.pid),
        )
    })?;
    Ok(stream::response(
        client.pid,
        identity,
        snapshot.revision,
        snapshot.event_sequence,
        receiver,
    )
    .into_response())
}

#[utoipa::path(
    post,
    path = "/clients/{client}/load",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The DLL was already loaded", body = LoadResult),
        (status = 202, description = "The DLL was loaded and the daemon is connecting", body = LoadResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "The client is busy or another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
async fn load(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<LoadResult>), ApiError> {
    let (pid, status) = tracked_status(&state, &identifier)?;
    match status {
        ClientSnapshotStatus::Connected => {
            return Ok((StatusCode::OK, Json(LoadResult::unchanged(pid))));
        }
        ClientSnapshotStatus::Busy | ClientSnapshotStatus::Incompatible => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "client_busy",
                "the client DLL is already owned by another or incompatible controller",
                Some(pid),
            ));
        }
        ClientSnapshotStatus::Initializing => {
            return Err(operation_in_progress(pid));
        }
        ClientSnapshotStatus::Connecting
        | ClientSnapshotStatus::NotLoaded
        | ClientSnapshotStatus::Disconnected => {}
    }

    state.emit(DaemonEvent::Status(ConnectionEvent::Initializing { pid }))?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.load(pid)).await?;
    state.emit(DaemonEvent::Status(ConnectionEvent::Connecting { pid }))?;
    Ok((StatusCode::ACCEPTED, Json(LoadResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/{client}/unload",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    responses(
        (status = 200, description = "The DLL is unloaded", body = UnloadResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "Another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
async fn unload(
    Path(identifier): Path<String>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<UnloadResult>), ApiError> {
    let (pid, status) = tracked_status(&state, &identifier)?;
    match status {
        ClientSnapshotStatus::NotLoaded => {
            return Ok((StatusCode::OK, Json(UnloadResult::unchanged(pid))));
        }
        ClientSnapshotStatus::Initializing => return Err(operation_in_progress(pid)),
        ClientSnapshotStatus::Connecting
        | ClientSnapshotStatus::Connected
        | ClientSnapshotStatus::Busy
        | ClientSnapshotStatus::Disconnected
        | ClientSnapshotStatus::Incompatible => {}
    }

    state.emit(DaemonEvent::Status(ConnectionEvent::Initializing { pid }))?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.unload(pid)).await?;
    state.emit(DaemonEvent::Status(ConnectionEvent::NotLoaded { pid }))?;
    Ok((StatusCode::OK, Json(UnloadResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/launch",
    request_body(content = LaunchOptions, description = "Supported Dark Ages executable path and launch options", content_type = "application/json"),
    responses(
        (status = 201, description = "The configured client was launched with the DLL initialized", body = LifecycleResult),
        (status = 400, description = "The launch options were invalid", body = ErrorState),
        (status = 413, description = "The request body exceeded 4 KiB", body = ErrorState),
        (status = 422, description = "Loader validation rejected the configured client", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
async fn launch(
    State(state): State<ApiState>,
    request: Result<Json<LaunchOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<LifecycleResult>), ApiError> {
    let Json(request) = request.map_err(|rejection| {
        ApiError::new(
            rejection.status(),
            "invalid_request",
            rejection.body_text(),
            None,
        )
    })?;
    let options = ManagedLaunchOptions::try_from(request)?;
    let lifecycle = Arc::clone(&state.lifecycle);
    let outcome = run_lifecycle(move || lifecycle.launch(&options)).await?;
    state.emit(DaemonEvent::Track(outcome.pid))?;
    Ok((StatusCode::CREATED, Json(LifecycleResult::from(outcome))))
}

pub(crate) fn resolve_client<'a>(
    registry: &'a RegistrySnapshot,
    identifier: &str,
) -> Result<&'a RegistryClientSnapshot, ApiError> {
    if identifier.bytes().all(|byte| byte.is_ascii_digit()) {
        let pid = identifier.parse::<u32>().map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_client",
                "numeric client identifiers must be valid nonzero process IDs",
                None,
            )
        })?;
        if pid == 0 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_client",
                "numeric client identifiers must be valid nonzero process IDs",
                Some(pid),
            ));
        }
        return registry
            .clients
            .iter()
            .find(|client| client.pid == pid)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "client_not_found",
                    format!("process {pid} is not a discovered or configured client"),
                    Some(pid),
                )
            });
    }

    let mut matches = registry.clients.iter().filter(|client| {
        current_character_name(client).is_some_and(|name| name.eq_ignore_ascii_case(identifier))
    });
    let Some(client) = matches.next() else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "client_not_found",
            format!("no connected in-game client is named {identifier:?}"),
            None,
        ));
    };
    if matches.next().is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ambiguous_client",
            format!("more than one connected in-game client is named {identifier:?}"),
            None,
        ));
    }
    Ok(client)
}

fn resolve_game_snapshot<'a>(
    registry: &'a RegistrySnapshot,
    identifier: &str,
) -> Result<(u32, &'a darpc_model::ClientSnapshot), ApiError> {
    let client = resolve_client(registry, identifier)?;
    let pid = client.pid;
    let snapshot = client.game_snapshot.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "observation_unavailable",
            client
                .snapshot_reason
                .as_deref()
                .unwrap_or("the client has not published an observation yet"),
            Some(pid),
        )
    })?;
    Ok((pid, snapshot))
}

fn current_character_name(client: &RegistryClientSnapshot) -> Option<&str> {
    if client.status != ClientSnapshotStatus::Connected {
        return None;
    }
    let snapshot = client.game_snapshot.as_ref()?;
    if snapshot.lifecycle != darpc_model::ClientLifecycle::InGame {
        return None;
    }
    snapshot
        .character
        .as_ref()?
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
}

fn tracked_status(
    state: &ApiState,
    identifier: &str,
) -> Result<(u32, ClientSnapshotStatus), ApiError> {
    let registry = state.snapshot();
    let client = resolve_client(&registry, identifier)?;
    Ok((client.pid, client.status))
}

async fn run_lifecycle(
    operation: impl FnOnce() -> Result<LifecycleOutcome, ManagementError> + Send + 'static,
) -> Result<LifecycleOutcome, ApiError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "management_worker_failed",
                format!("management worker failed: {error}"),
                None,
            )
        })?
        .map_err(ApiError::from)
}

fn operation_in_progress(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "operation_in_progress",
        "another lifecycle operation is already active for this client",
        Some(pid),
    )
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        clients,
        client_status,
        client_inventory,
        client_equipment,
        client_spellbook,
        client_skillbook,
        client_effects,
        client_events,
        load,
        unload,
        launch,
        crate::commands::diagnostic,
        crate::commands::status,
        crate::commands::cancel
    ),
    components(schemas(
        HealthState,
        HealthStatus,
        ClientList,
        ClientState,
        ClientStatus,
        ClientIdentity,
        ConnectionMetadata,
        ObservationMetadata,
        GameStatus,
        SnapshotClientLifecycle,
        CharacterStatus,
        CharacterGender,
        SnapshotCharacterClass,
        CharacterProgression,
        CharacterStats,
        CharacterVitals,
        CharacterModifiers,
        Element,
        MapLocation,
        Inventory,
        InventoryItem,
        Equipment,
        EquipmentItem,
        EquipmentSlot,
        Spellbook,
        Spell,
        Skillbook,
        Skill,
        CooldownStatus,
        SpellTargetType,
        Effects,
        Effect,
        EffectDuration,
        LaunchOptions,
        LoadResult,
        UnloadResult,
        LifecycleResult,
        LifecycleAction,
        ErrorState,
        ErrorDetail,
        ClientEvent,
        crate::commands::DiagnosticOptions,
        crate::commands::CommandStatus,
        crate::commands::CommandKind,
        crate::commands::CommandState,
        crate::commands::CommandFailure
    ))
)]
struct ApiDoc;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct HealthState {
    status: HealthStatus,
    version: &'static str,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum HealthStatus {
    Ok,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct ClientList {
    clients: Vec<ClientState>,
}

impl From<&RegistrySnapshot> for ClientList {
    fn from(snapshot: &RegistrySnapshot) -> Self {
        Self {
            clients: snapshot.clients.iter().map(ClientState::from).collect(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct ClientState {
    /// Configured operating-system process identifier.
    pid: u32,
    /// Current endpoint identifier: in-game character name or process ID.
    name: String,
    /// Current daemon connection state for this target.
    status: ClientStatus,
    /// Stable process and DLL identity, once observed.
    identity: Option<ClientIdentity>,
    /// Last accepted connection metadata, when available.
    connection: Option<ConnectionMetadata>,
    /// Human-readable detail for disconnected or incompatible targets.
    reason: Option<String>,
}

impl From<&RegistryClientSnapshot> for ClientState {
    fn from(client: &RegistryClientSnapshot) -> Self {
        Self {
            pid: client.pid,
            name: current_character_name(client)
                .map_or_else(|| client.pid.to_string(), str::to_owned),
            status: ClientStatus::from(client.status),
            identity: client.identity.map(ClientIdentity::from),
            connection: client
                .hello
                .zip(client.selected_version)
                .map(|(hello, selected_version)| ConnectionMetadata::new(hello, selected_version)),
            reason: client.reason.clone(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum ClientStatus {
    Connecting,
    Initializing,
    NotLoaded,
    Connected,
    Busy,
    Disconnected,
    Incompatible,
}

impl From<ClientSnapshotStatus> for ClientStatus {
    fn from(status: ClientSnapshotStatus) -> Self {
        match status {
            ClientSnapshotStatus::Connecting => Self::Connecting,
            ClientSnapshotStatus::Initializing => Self::Initializing,
            ClientSnapshotStatus::NotLoaded => Self::NotLoaded,
            ClientSnapshotStatus::Connected => Self::Connected,
            ClientSnapshotStatus::Busy => Self::Busy,
            ClientSnapshotStatus::Disconnected => Self::Disconnected,
            ClientSnapshotStatus::Incompatible => Self::Incompatible,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct ClientIdentity {
    /// Unsigned 64-bit Windows process creation time encoded in decimal.
    created_time: String,
    /// Per-load DLL instance identifier encoded as 32 lowercase hexadecimal digits.
    instance_id: String,
}

impl From<RegistryClientIdentity> for ClientIdentity {
    fn from(identity: RegistryClientIdentity) -> Self {
        Self {
            created_time: identity.process_creation_time.to_string(),
            instance_id: hex(&identity.dll_instance_id),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct ConnectionMetadata {
    /// Negotiated daRPC binary protocol version.
    protocol_version: String,
    /// Architecture reported by the injected DLL.
    architecture: String,
    /// Semantic version of the injected DLL.
    dll_version: String,
    /// Supported executable fingerprint encoded as lowercase hexadecimal.
    executable_fingerprint: String,
    /// Supported Dark Ages client version.
    client_version: String,
}

impl ConnectionMetadata {
    fn new(hello: Hello, selected_version: u16) -> Self {
        Self {
            protocol_version: format!(
                "{}.{}",
                protocol_version_major(selected_version),
                protocol_version_minor(selected_version)
            ),
            architecture: architecture(hello.architecture).into(),
            dll_version: format!(
                "{}.{}.{}",
                hello.dll_version.major, hello.dll_version.minor, hello.dll_version.patch
            ),
            executable_fingerprint: hex(&hello.executable_fingerprint),
            client_version: CLIENT_VERSION.into(),
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct LaunchOptions {
    /// Full path to the supported Dark Ages client executable.
    client_path: String,
    /// Allow another Dark Ages client process to start.
    #[serde(default)]
    allow_multiple: bool,
    /// Skip the introductory video.
    #[serde(default)]
    skip_intro: bool,
    /// Skip the title-screen notice and its associated delays.
    #[serde(default)]
    skip_notice: bool,
    /// Override the game server as `host` or `host:port`; the port defaults to 2610.
    #[serde(default)]
    server: Option<String>,
}

impl TryFrom<LaunchOptions> for ManagedLaunchOptions {
    type Error = ApiError;

    fn try_from(options: LaunchOptions) -> Result<Self, Self::Error> {
        let client_path = validate_client_path(options.client_path)?;
        let server = options.server.map(validate_server).transpose()?;
        Ok(Self {
            client_path,
            allow_multiple: options.allow_multiple,
            skip_intro: options.skip_intro,
            skip_notice: options.skip_notice,
            server,
        })
    }
}

fn validate_client_path(client_path: String) -> Result<std::path::PathBuf, ApiError> {
    let bytes = client_path.as_bytes();
    let drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let unc_absolute = client_path.starts_with("\\\\") || client_path.starts_with("//");
    if client_path.contains('\0') || (!drive_absolute && !unc_absolute) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_client_path",
            "client_path must be a fully qualified Windows executable path",
            None,
        ));
    }
    Ok(client_path.into())
}

fn validate_server(server: String) -> Result<ManagedServerEndpoint, ApiError> {
    if server.is_empty() || server.trim() != server {
        return Err(invalid_server(
            "server must be a nonempty host or host:port without surrounding whitespace",
        ));
    }

    if server.matches(':').count() > 1 {
        return Err(invalid_server(
            "server must be an IPv4 address or hostname with an optional port",
        ));
    }

    let (host, port) = match server.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() || port.is_empty() {
                return Err(invalid_server("server host and port must not be empty"));
            }
            let port = port.parse::<u16>().map_err(|_| {
                invalid_server("server port must be an integer from 1 through 65535")
            })?;
            (host, port)
        }
        None => (server.as_str(), DEFAULT_SERVER_PORT),
    };

    if port == 0 {
        return Err(invalid_server(
            "server port must be an integer from 1 through 65535",
        ));
    }
    if host.len() > 255
        || host
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(invalid_server(
            "server host must be at most 255 characters without whitespace or control characters",
        ));
    }

    Ok(ManagedServerEndpoint {
        host: host.to_owned(),
        port,
    })
}

fn invalid_server(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_server", message, None)
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct LoadResult {
    operation: LifecycleAction,
    pid: u32,
    was_loaded: bool,
}

impl LoadResult {
    fn unchanged(pid: u32) -> Self {
        Self {
            operation: LifecycleAction::Load,
            pid,
            was_loaded: false,
        }
    }
}

impl From<LifecycleOutcome> for LoadResult {
    fn from(outcome: LifecycleOutcome) -> Self {
        Self {
            operation: LifecycleAction::Load,
            pid: outcome.pid,
            was_loaded: outcome.changed,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct UnloadResult {
    operation: LifecycleAction,
    pid: u32,
    was_unloaded: bool,
}

impl UnloadResult {
    fn unchanged(pid: u32) -> Self {
        Self {
            operation: LifecycleAction::Unload,
            pid,
            was_unloaded: false,
        }
    }
}

impl From<LifecycleOutcome> for UnloadResult {
    fn from(outcome: LifecycleOutcome) -> Self {
        Self {
            operation: LifecycleAction::Unload,
            pid: outcome.pid,
            was_unloaded: outcome.changed,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct LifecycleResult {
    operation: LifecycleAction,
    pid: u32,
    changed: bool,
    darpc_loaded: bool,
}

impl From<LifecycleOutcome> for LifecycleResult {
    fn from(outcome: LifecycleOutcome) -> Self {
        Self {
            operation: LifecycleAction::from(outcome.operation),
            pid: outcome.pid,
            changed: outcome.changed,
            darpc_loaded: outcome.darpc_loaded,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum LifecycleAction {
    Load,
    Unload,
    Launch,
}

impl From<LifecycleOperation> for LifecycleAction {
    fn from(operation: LifecycleOperation) -> Self {
        match operation {
            LifecycleOperation::Load => Self::Load,
            LifecycleOperation::Unload => Self::Unload,
            LifecycleOperation::Launch => Self::Launch,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    body: ErrorState,
}

impl ApiError {
    pub(crate) fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        pid: Option<u32>,
    ) -> Self {
        Self {
            status,
            body: ErrorState {
                error: ErrorDetail {
                    code: code.into(),
                    message: message.into(),
                    pid,
                },
            },
        }
    }
}

impl From<ManagementError> for ApiError {
    fn from(error: ManagementError) -> Self {
        let status = match error.code.as_str() {
            "invalid_arguments" => StatusCode::BAD_REQUEST,
            "process_missing" | "process_exited" => StatusCode::NOT_FOUND,
            "access_denied" => StatusCode::FORBIDDEN,
            "already_loaded" => StatusCode::CONFLICT,
            "invalid_dll" | "wrong_architecture" | "unsupported_client" => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            "timeout" => StatusCode::GATEWAY_TIMEOUT,
            "loader_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
            "invalid_loader_response" => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.code, error.message, error.pid)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ErrorState {
    error: ErrorDetail,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ErrorDetail {
    code: String,
    message: String,
    pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::{ApiState, ClientList, LaunchOptions, resolve_client, router};
    use crate::{
        commands::{CommandReply, ROUTER_CAPACITY},
        event::DaemonEvent,
        lifecycle::{
            LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
            LifecycleOutcome, ManagementError,
        },
        registry::{ClientIdentity as RegistryClientIdentity, ConnectionEvent, Registry},
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use darpc_model::{
        CharacterAppearance, CharacterClass, CharacterProgression,
        CharacterSnapshot as ModelCharacterSnapshot, CharacterStats, CharacterVitals,
        ClientLifecycle, ClientSnapshot as ModelClientSnapshot,
        CooldownStatus as ModelCooldownStatus, Effect as ModelEffect,
        EffectDuration as ModelEffectDuration, EquipmentItem as ModelEquipmentItem,
        EquipmentSlot as ModelEquipmentSlot, Gender, InventoryItem as ModelInventoryItem,
        MapLocation, Skill as ModelSkill, Spell as ModelSpell,
        SpellTargetType as ModelSpellTargetType,
    };
    use darpc_protocol::{
        Architecture, CommandKind, CommandOperation, CommandResult, CommandState, CommandStatus,
        ComponentVersion, Hello, SUPPORTED_VERSIONS,
    };
    use serde_json::Value;
    use std::{
        net::{Ipv4Addr, SocketAddrV4, TcpListener},
        sync::{Arc, mpsc},
    };
    use tower::ServiceExt as _;

    fn hello() -> Hello {
        Hello {
            protocol_versions: SUPPORTED_VERSIONS,
            dll_instance_id: [0xAB; 16],
            process_id: 42,
            process_creation_time: 134_299_999_186_432_946,
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            executable_fingerprint: [0xCD; 32],
            client_version: 741,
        }
    }

    fn game_snapshot() -> ModelClientSnapshot {
        ModelClientSnapshot {
            revision: 3,
            event_sequence: 2,
            captured_tick_ms: 500,
            updated_tick_ms: 510,
            capture_duration_us: 75,
            world_generation: 1,
            lifecycle: ClientLifecycle::InGame,
            character: Some(ModelCharacterSnapshot {
                id: Some(1234),
                name: Some("SiLo".into()),
                appearance: Some(CharacterAppearance {
                    gender: Gender::Male,
                    hair_style: 17,
                    hair_color: 6,
                    body_sprite: 1,
                }),
                class: CharacterClass::Wizard,
                is_action_restricted: false,
                is_blinded: true,
                gold: 99,
                weight: 25,
                max_weight: 60,
                progression: CharacterProgression {
                    level: 50,
                    ability_level: 2,
                    experience: 1_000,
                    ability_points: Some(10),
                    experience_to_next_level: Some(20),
                    ability_to_next_level: Some(30),
                },
                stats: CharacterStats {
                    strength: 3,
                    intelligence: 7,
                    wisdom: 6,
                    constitution: 5,
                    dexterity: 4,
                },
                vitals: CharacterVitals {
                    health: 80,
                    max_health: 100,
                    mana: 60,
                    max_mana: 70,
                },
                modifiers: None,
                location: Some(MapLocation {
                    id: 3001,
                    name: None,
                    x: Some(11),
                    y: Some(22),
                    width: 100,
                    height: 80,
                }),
                inventory: Some(vec![ModelInventoryItem {
                    slot: 1,
                    sprite: 0x0123,
                    dye_color: 7,
                    name: Some("Dark Belt".into()),
                    quantity: 3,
                    can_stack: true,
                    durability: 41,
                    max_durability: 50,
                }]),
                equipment: Some(vec![ModelEquipmentItem {
                    slot: ModelEquipmentSlot::Armor,
                    sprite: 0x1234,
                    dye_color: 2,
                    name: Some("Hy-Brasyl Armor".into()),
                    durability: 900,
                    max_durability: 1_000,
                }]),
                spellbook: Some(vec![ModelSpell {
                    slot: 7,
                    icon: 0x0456,
                    name: Some("Fas Spiorad".into()),
                    level: 3,
                    max_level: 5,
                    lines: 4,
                    target_type: ModelSpellTargetType::TextInput,
                    prompt: Some("Who?".into()),
                    cooldown: ModelCooldownStatus {
                        active: true,
                        remaining_ms: None,
                    },
                }]),
                skillbook: Some(vec![ModelSkill {
                    slot: 4,
                    icon: 0x0123,
                    name: Some("Assail".into()),
                    level: 10,
                    max_level: 100,
                    cooldown: ModelCooldownStatus {
                        active: true,
                        remaining_ms: Some(750),
                    },
                }]),
                effects: Some(vec![ModelEffect {
                    icon: 300,
                    duration: ModelEffectDuration::White,
                }]),
            }),
        }
    }

    fn state() -> ApiState {
        let mut registry = Registry::new();
        let hello = hello();
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Snapshot {
            pid: 42,
            identity: RegistryClientIdentity::from_hello(hello),
            snapshot: Box::new(game_snapshot()),
        });
        let (events, _receiver) = mpsc::channel();
        ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
    }

    struct FakeLifecycle;

    impl LifecycleControl for FakeLifecycle {
        fn load(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
            Ok(LifecycleOutcome {
                operation: LifecycleOperation::Load,
                pid,
                changed: true,
                darpc_loaded: true,
            })
        }

        fn unload(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
            Ok(LifecycleOutcome {
                operation: LifecycleOperation::Unload,
                pid,
                changed: true,
                darpc_loaded: false,
            })
        }

        fn launch(
            &self,
            _options: &ManagedLaunchOptions,
        ) -> Result<LifecycleOutcome, ManagementError> {
            Ok(LifecycleOutcome {
                operation: LifecycleOperation::Launch,
                pid: 77,
                changed: true,
                darpc_loaded: true,
            })
        }
    }

    fn response(path: &str) -> axum::response::Response {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                router(state())
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
            })
    }

    fn json(path: &str) -> Value {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let response = router(state())
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
                serde_json::from_slice(&bytes).unwrap()
            })
    }

    fn text(path: &str) -> String {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let response = router(state())
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
                String::from_utf8(bytes.to_vec()).unwrap()
            })
    }

    fn state_with_status(event: ConnectionEvent) -> (ApiState, mpsc::Receiver<DaemonEvent>) {
        let mut registry = Registry::new();
        registry.apply(&event);
        let (events, receiver) = mpsc::channel();
        (
            ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events),
            receiver,
        )
    }

    fn post_json(state: ApiState, path: &str, body: &str) -> axum::response::Response {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                router(state)
                    .oneshot(
                        Request::post(path)
                            .header("content-type", "application/json")
                            .body(Body::from(body.to_owned()))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            })
    }

    #[test]
    fn routes_a_diagnostic_through_the_bounded_command_path() {
        let mut registry = Registry::new();
        let hello = hello();
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        let (events, event_receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
        let state = ApiState::new(registry.snapshot(), Arc::new(FakeLifecycle), events)
            .with_command_sender(commands);
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                event_receiver.recv().unwrap(),
                DaemonEvent::CommandsReady
            ));
            let call = command_receiver.recv().unwrap();
            assert_eq!(call.pid, 42);
            assert_eq!(call.identity, RegistryClientIdentity::from_hello(hello));
            assert!(matches!(
                call.operation,
                CommandOperation::Submit {
                    kind: CommandKind::Diagnostic,
                    timeout_ms: 1_000,
                    wait_ms: 1_000,
                }
            ));
            call.reply
                .send(CommandReply::Result(CommandResult::Status(CommandStatus {
                    command_id: 9,
                    kind: CommandKind::Diagnostic,
                    state: CommandState::Executed,
                    enqueued_tick_ms: 100,
                    deadline_tick_ms: 1_100,
                    started_tick_ms: Some(104),
                    completed_tick_ms: Some(104),
                    execution_us: Some(2),
                    main_thread_id: Some(77),
                    failure: None,
                })))
                .unwrap();
        });

        let response = post_json(state, "/clients/42/commands/diagnostic", "{}");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response);
        assert_eq!(body["pid"], 42);
        assert_eq!(body["instance_id"], "abababababababababababababababab");
        assert_eq!(body["command_id"], 9);
        assert_eq!(body["state"], "executed");
        assert_eq!(body["queue_delay_ms"], 4);
        assert_eq!(body["execution_us"], 2);
        assert_eq!(body["main_thread_id"], 77);
        worker.join().unwrap();
    }

    fn response_json(response: axum::response::Response) -> Value {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
                serde_json::from_slice(&bytes).unwrap()
            })
    }

    #[test]
    fn serves_health_and_client_resources() {
        assert_eq!(json("/health")["status"], "ok");

        let clients = json("/clients");
        assert_eq!(clients["clients"][0]["pid"], 42);
        assert_eq!(clients["clients"][0]["name"], "SiLo");
        assert_eq!(clients["clients"][0]["status"], "connected");
        assert_eq!(
            clients["clients"][0]["identity"]["created_time"],
            "134299999186432946"
        );
        assert_eq!(
            clients["clients"][0]["connection"]["protocol_version"],
            "1.0"
        );
        assert_eq!(
            clients["clients"][0]["connection"]["client_version"],
            "7.41"
        );
        assert!(
            clients["clients"][0]["connection"]
                .get("layout_id")
                .is_none()
        );

        let status = json("/clients/silo/status");
        assert_eq!(status["observation"]["pid"], 42);
        assert_eq!(status["observation"]["revision"], 3);
        assert_eq!(status["observation"]["event_sequence"], 2);
        assert_eq!(status["observation"]["updated_tick_ms"], 510);
        assert_eq!(status["lifecycle"], "in_game");
        assert_eq!(status["character"]["name"], "SiLo");
        assert_eq!(status["character"]["gender"], "male");
        assert_eq!(status["character"]["hair_style"], 17);
        assert_eq!(status["character"]["hair_color"], 6);
        assert_eq!(status["character"]["body_sprite"], 1);
        assert_eq!(status["character"]["is_action_restricted"], false);
        assert_eq!(status["character"]["is_blinded"], true);
        assert!(status["character"].get("gender_id").is_none());
        assert!(status["character"].get("class_id").is_none());
        assert!(status["character"].get("inventory").is_none());
        assert!(status["character"].get("equipment").is_none());
        assert!(status["character"].get("spellbook").is_none());
        assert!(status["character"].get("skillbook").is_none());
        assert_eq!(status["character"]["progression"]["level"], 50);
        assert_eq!(status["map"]["x"], 11);

        let inventory = json("/clients/silo/inventory");
        assert_eq!(inventory["observation"]["revision"], 3);
        assert_eq!(inventory["items"][0]["quantity"], 3);
        assert_eq!(inventory["items"][0]["can_stack"], true);
        assert_eq!(inventory["items"][0]["name"], "Dark Belt");
        assert_eq!(inventory["items"][0]["sprite"], 0x0123);

        let equipment = json("/clients/silo/equipment");
        assert_eq!(equipment["observation"]["revision"], 3);
        assert_eq!(equipment["items"][0]["slot"], "armor");

        let spellbook = json("/clients/silo/spellbook");
        assert_eq!(spellbook["observation"]["revision"], 3);
        assert_eq!(spellbook["spells"][0]["target_type"], "text_input");
        assert_eq!(spellbook["spells"][0]["prompt"], "Who?");
        assert!(spellbook["spells"][0].get("target_type_id").is_none());

        let skillbook = json("/clients/silo/skillbook");
        assert_eq!(skillbook["observation"]["revision"], 3);
        assert_eq!(skillbook["skills"][0]["max_level"], 100);

        let effects = json("/clients/silo/effects");
        assert_eq!(effects["observation"]["revision"], 3);
        assert_eq!(effects["effects"][0]["icon"], 300);
        assert_eq!(effects["effects"][0]["duration"], "white");

        let events = response("/clients/silo/events");
        assert_eq!(events.status(), StatusCode::OK);
        assert_eq!(
            events.headers()[axum::http::header::CONTENT_TYPE],
            "text/event-stream"
        );

        assert_eq!(
            response("/clients/silo/snapshot").status(),
            StatusCode::NOT_FOUND
        );

        let state = state();
        let mut registry = Registry::new();
        registry.apply(&ConnectionEvent::NotLoaded { pid: 7 });
        state.publish(registry.snapshot());
        assert_eq!(state.snapshot().clients[0].pid, 7);
    }

    #[test]
    fn disconnected_snapshots_fall_back_to_the_process_id() {
        let mut registry = Registry::new();
        let hello = hello();
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        let mut snapshot = game_snapshot();
        snapshot.lifecycle = ClientLifecycle::Disconnected;
        registry.apply(&ConnectionEvent::Snapshot {
            pid: 42,
            identity: RegistryClientIdentity::from_hello(hello),
            snapshot: Box::new(snapshot),
        });

        let snapshot = registry.snapshot();
        let clients = serde_json::to_value(ClientList::from(&snapshot)).unwrap();
        assert_eq!(clients["clients"][0]["name"], "42");
        assert_eq!(resolve_client(&snapshot, "42").unwrap().pid, 42);
        assert_eq!(
            resolve_client(&snapshot, "silo").unwrap_err().status,
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn duplicate_active_character_names_are_ambiguous() {
        let mut registry = Registry::new();
        for (pid, instance) in [(42, 0xAB), (43, 0xAC)] {
            let mut hello = hello();
            hello.process_id = pid;
            hello.process_creation_time += u64::from(pid);
            hello.dll_instance_id = [instance; 16];
            registry.apply(&ConnectionEvent::Connected {
                pid,
                hello,
                selected_version: SUPPORTED_VERSIONS.max,
            });
            registry.apply(&ConnectionEvent::Snapshot {
                pid,
                identity: RegistryClientIdentity::from_hello(hello),
                snapshot: Box::new(game_snapshot()),
            });
        }

        assert_eq!(
            resolve_client(&registry.snapshot(), "SILO")
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn serializes_every_client_status() {
        let mut registry = Registry::new();
        registry.apply(&ConnectionEvent::Connecting { pid: 1 });
        registry.apply(&ConnectionEvent::Initializing { pid: 2 });
        registry.apply(&ConnectionEvent::NotLoaded { pid: 3 });
        let mut connected = hello();
        connected.process_id = 4;
        registry.apply(&ConnectionEvent::Connected {
            pid: 4,
            hello: connected,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Busy { pid: 5 });
        registry.apply(&ConnectionEvent::Disconnected {
            pid: 6,
            identity: None,
            reason: "closed".into(),
        });
        registry.apply(&ConnectionEvent::Incompatible {
            pid: 7,
            identity: None,
            reason: "unsupported".into(),
        });

        let value = serde_json::to_value(ClientList::from(&registry.snapshot())).unwrap();
        let statuses = value["clients"]
            .as_array()
            .unwrap()
            .iter()
            .map(|client| client["status"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            statuses,
            [
                "connecting",
                "initializing",
                "not_loaded",
                "connected",
                "busy",
                "disconnected",
                "incompatible",
            ]
        );
    }

    #[test]
    fn serves_the_openapi_contract_and_vendored_swagger_ui() {
        let openapi = json("/openapi.json");
        assert_eq!(openapi["openapi"], "3.1.0");
        assert_eq!(openapi["info"]["title"], "daRPC API");
        assert!(openapi["paths"]["/health"].is_object());
        assert!(openapi["paths"]["/clients"].is_object());
        for path in [
            "/clients/{client}/status",
            "/clients/{client}/inventory",
            "/clients/{client}/equipment",
            "/clients/{client}/spellbook",
            "/clients/{client}/skillbook",
            "/clients/{client}/effects",
            "/clients/{client}/events",
            "/clients/{client}/commands/diagnostic",
            "/clients/{client}/commands/{command_id}",
        ] {
            assert!(openapi["paths"][path].is_object(), "OpenAPI omitted {path}");
        }
        assert!(openapi["paths"]["/clients/{client}/snapshot"].is_null());
        assert!(openapi["paths"]["/clients/launch"].is_object());
        assert!(openapi["paths"]["/clients/{client}/load"].is_object());
        assert!(openapi["paths"]["/clients/{client}/unload"].is_object());
        let schemas = openapi["components"]["schemas"].as_object().unwrap();
        for name in [
            "HealthState",
            "HealthStatus",
            "ClientList",
            "ClientState",
            "ClientStatus",
            "ClientIdentity",
            "ConnectionMetadata",
            "ObservationMetadata",
            "GameStatus",
            "ClientLifecycle",
            "CharacterStatus",
            "CharacterGender",
            "CharacterClass",
            "CharacterProgression",
            "CharacterStats",
            "CharacterVitals",
            "CharacterModifiers",
            "Element",
            "MapLocation",
            "Inventory",
            "InventoryItem",
            "Equipment",
            "EquipmentItem",
            "EquipmentSlot",
            "Spellbook",
            "Spell",
            "Skillbook",
            "Skill",
            "CooldownStatus",
            "SpellTargetType",
            "Effects",
            "Effect",
            "EffectDuration",
            "LaunchOptions",
            "LoadResult",
            "LifecycleResult",
            "LifecycleAction",
            "UnloadResult",
            "ErrorState",
            "ErrorDetail",
            "ClientEvent",
            "StreamReady",
            "EventObservation",
            "EffectAdded",
            "EffectRemoved",
            "EffectChanged",
            "StreamResyncRequired",
            "StreamClosed",
            "DiagnosticOptions",
            "CommandStatus",
            "CommandKind",
            "CommandState",
            "CommandFailure",
        ] {
            assert!(schemas.contains_key(name), "OpenAPI omitted {name}");
        }
        let event_response =
            &openapi["paths"]["/clients/{client}/events"]["get"]["responses"]["200"];
        assert_eq!(
            event_response["content"]["text/event-stream"]["schema"]["$ref"],
            "#/components/schemas/ClientEvent"
        );
        let event_variants = schemas["ClientEvent"]["oneOf"].as_array().unwrap();
        for event_type in [
            "stream_ready",
            "effect_added",
            "effect_removed",
            "effect_changed",
        ] {
            assert!(event_variants.iter().any(|variant| {
                variant["properties"]["type"]["enum"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == event_type))
            }));
        }
        assert!(
            schemas["LaunchOptions"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "client_path")
        );
        assert!(schemas["LoadResult"]["properties"]["was_loaded"].is_object());
        assert!(schemas["LoadResult"]["properties"]["changed"].is_null());
        assert!(schemas["UnloadResult"]["properties"]["was_unloaded"].is_object());
        assert!(schemas["UnloadResult"]["properties"]["changed"].is_null());
        assert!(
            schemas["CharacterStatus"]["properties"]
                .get("gender_id")
                .is_none()
        );
        assert!(
            schemas["CharacterStatus"]["properties"]
                .get("class_id")
                .is_none()
        );
        for collection in [
            "inventory",
            "equipment",
            "spellbook",
            "skillbook",
            "effects",
        ] {
            assert!(
                schemas["CharacterStatus"]["properties"]
                    .get(collection)
                    .is_none(),
                "CharacterStatus still exposes {collection}"
            );
        }
        assert!(
            schemas["CharacterModifiers"]["properties"]
                .get("attack_element_id")
                .is_none()
        );
        assert!(
            schemas["CharacterModifiers"]["properties"]
                .get("defense_element_id")
                .is_none()
        );
        assert!(
            schemas["Spell"]["properties"]
                .get("target_type_id")
                .is_none()
        );
        assert!(
            schemas["ClientLifecycle"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "disconnected")
        );

        let docs = response("/docs/");
        assert_eq!(docs.status(), StatusCode::OK);
        assert!(text("/docs/").contains("/docs/ayu.css"));
        let asset = response("/docs/assets/swagger-ui-bundle.js");
        assert_eq!(asset.status(), StatusCode::OK);
        let theme = text("/docs/ayu.css");
        assert!(theme.contains("--ayu-bg: #0b0e14"));
        assert!(theme.contains("--ayu-orange: #ffb454"));
        assert!(theme.contains(".swagger-ui .info .title small pre.version"));
        assert!(theme.contains(".swagger-ui button.model-box-control"));
        assert!(theme.contains(".swagger-ui .json-schema-2020-12-accordion"));
        assert!(theme.contains(".swagger-ui .opblock-summary-control:focus"));
        assert!(theme.contains(".swagger-ui .opblock .opblock-section-header h4"));
    }

    #[test]
    fn delegates_typed_lifecycle_operations() {
        let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
        let response = post_json(state, "/clients/42/load", "");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let result = response_json(response);
        assert_eq!(result["was_loaded"], true);
        assert!(result.get("changed").is_none());
        assert!(matches!(
            receiver.recv().unwrap(),
            DaemonEvent::Status(ConnectionEvent::Initializing { pid: 42 })
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            DaemonEvent::Status(ConnectionEvent::Connecting { pid: 42 })
        ));

        let (state, receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
        let response = post_json(
            state,
            "/clients/launch",
            r#"{"client_path":"C:\\Darkages.exe","allow_multiple":true,"server":"127.0.0.1"}"#,
        );
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(matches!(receiver.recv().unwrap(), DaemonEvent::Track(77)));
    }

    #[test]
    fn reports_no_transition_when_the_dll_is_already_in_the_requested_state() {
        let result = response_json(post_json(state(), "/clients/42/load", ""));
        assert_eq!(result["was_loaded"], false);
        assert!(result.get("changed").is_none());

        let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
        let result = response_json(post_json(state, "/clients/42/unload", ""));
        assert_eq!(result["was_unloaded"], false);
        assert!(result.get("changed").is_none());
    }

    #[test]
    fn rejects_arbitrary_launch_arguments() {
        let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
        let response = post_json(
            state,
            "/clients/launch",
            r#"{"client_path":"C:\\Darkages.exe","arguments":["unsafe"]}"#,
        );
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn accepts_a_full_client_path_and_defaults_the_server_port() {
        let request: LaunchOptions = serde_json::from_str(
            r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"da0.kru.com"}"#,
        )
        .unwrap();
        let options = ManagedLaunchOptions::try_from(request).unwrap();
        assert_eq!(
            options.client_path,
            std::path::PathBuf::from(r"D:\Games\Dark Ages\Darkages.exe")
        );
        let server = options.server.unwrap();
        assert_eq!(server.host, "da0.kru.com");
        assert_eq!(server.port, 2610);

        let request: LaunchOptions = serde_json::from_str(
            r#"{"client_path":"D:\\Games\\Dark Ages\\Darkages.exe","server":"127.0.0.1:3000"}"#,
        )
        .unwrap();
        let server = ManagedLaunchOptions::try_from(request)
            .unwrap()
            .server
            .unwrap();
        assert_eq!(server.host, "127.0.0.1");
        assert_eq!(server.port, 3000);

        assert!(serde_json::from_str::<LaunchOptions>("{}").is_err());
        assert!(
            serde_json::from_str::<LaunchOptions>(
                r#"{"client_path":"C:\\Darkages.exe","server":{"host":"da0.kru.com"}}"#
            )
            .is_err()
        );
        for field in ["loader_path", "dll_path"] {
            let body = format!(r#"{{"{field}":"unsafe"}}"#);
            assert!(serde_json::from_str::<LaunchOptions>(&body).is_err());
        }
    }

    #[test]
    fn rejects_relative_client_paths() {
        let request: LaunchOptions =
            serde_json::from_str(r#"{"client_path":"Darkages.exe"}"#).unwrap();
        let error = ManagedLaunchOptions::try_from(request).unwrap_err();
        assert_eq!(error.body.error.code, "invalid_client_path");
    }

    #[test]
    fn rejects_invalid_server_strings() {
        for server in ["", ":2610", "host:", "host:0", "host:nope", "::1"] {
            let request = LaunchOptions {
                client_path: r"C:\Darkages.exe".into(),
                allow_multiple: false,
                skip_intro: false,
                skip_notice: false,
                server: Some(server.into()),
            };
            let error = ManagedLaunchOptions::try_from(request).unwrap_err();
            assert_eq!(error.body.error.code, "invalid_server");
        }
    }

    #[test]
    fn rejects_request_bodies() {
        let response = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                router(state())
                    .oneshot(
                        Request::get("/health")
                            .header("content-length", "1")
                            .body(Body::from("x"))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            });
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn refuses_an_occupied_port() {
        let held = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        let result = super::start(port, state());
        assert!(result.is_err());
    }
}
