use crate::{
    event::DaemonEvent,
    management::{
        LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
        LifecycleOutcome, ManagementError, ServerEndpoint as ManagedServerEndpoint,
    },
    registry::{
        ClientIdentity as RegistryClientIdentity, ClientSnapshot, ClientSnapshotStatus,
        ConnectionEvent, RegistrySnapshot, architecture, hex,
    },
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
use darpc_protocol::{Hello, protocol_version_major, protocol_version_minor};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    sync::{Arc, RwLock, mpsc::Sender},
    thread::{self, JoinHandle},
};
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
}

impl ApiState {
    #[must_use]
    pub(crate) fn new(
        snapshot: RegistrySnapshot,
        lifecycle: Arc<dyn LifecycleControl>,
        events: Sender<DaemonEvent>,
    ) -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(Arc::new(snapshot))),
            lifecycle,
            events,
        }
    }

    pub(crate) fn publish(&self, snapshot: RegistrySnapshot) {
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = Arc::new(snapshot);
    }

    fn snapshot(&self) -> Arc<RegistrySnapshot> {
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
}

pub(crate) fn start(port: u16, state: ApiState) -> io::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
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
        .route("/clients/launch", post(launch))
        .route("/clients/{pid}/load", post(load))
        .route("/clients/{pid}/unload", post(unload))
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
    if request.method() == Method::POST && request.uri().path() == "/clients/launch" {
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
    post,
    path = "/clients/{pid}/load",
    params(("pid" = u32, Path, description = "Discovered client process identifier")),
    responses(
        (status = 200, description = "The DLL was already loaded", body = LifecycleResult),
        (status = 202, description = "The DLL was loaded and the daemon is connecting", body = LifecycleResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "The client is busy or another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
async fn load(
    Path(pid): Path<u32>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<LifecycleResult>), ApiError> {
    let status = tracked_status(&state, pid)?;
    match status {
        ClientSnapshotStatus::Connected => {
            return Ok((
                StatusCode::OK,
                Json(LifecycleResult::unchanged(
                    LifecycleOperation::Load,
                    pid,
                    true,
                )),
            ));
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
    Ok((StatusCode::ACCEPTED, Json(LifecycleResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/{pid}/unload",
    params(("pid" = u32, Path, description = "Discovered client process identifier")),
    responses(
        (status = 200, description = "The DLL is unloaded", body = LifecycleResult),
        (status = 400, description = "The process identifier was invalid", body = ErrorState),
        (status = 404, description = "The process is not a discovered or configured client", body = ErrorState),
        (status = 409, description = "Another lifecycle operation is active", body = ErrorState),
        (status = 422, description = "Loader validation rejected the process", body = ErrorState),
        (status = 503, description = "The configured loader is unavailable", body = ErrorState),
        (status = 504, description = "The loader operation timed out", body = ErrorState)
    )
)]
async fn unload(
    Path(pid): Path<u32>,
    State(state): State<ApiState>,
) -> Result<(StatusCode, Json<LifecycleResult>), ApiError> {
    match tracked_status(&state, pid)? {
        ClientSnapshotStatus::NotLoaded => {
            return Ok((
                StatusCode::OK,
                Json(LifecycleResult::unchanged(
                    LifecycleOperation::Unload,
                    pid,
                    false,
                )),
            ));
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
    Ok((StatusCode::OK, Json(LifecycleResult::from(outcome))))
}

#[utoipa::path(
    post,
    path = "/clients/launch",
    request_body(content = LaunchOptions, description = "Supported Dark Ages launch options", content_type = "application/json"),
    responses(
        (status = 201, description = "The configured client was launched with the DLL initialized", body = LifecycleResult),
        (status = 400, description = "The launch options were invalid", body = ErrorState),
        (status = 413, description = "The request body exceeded 4 KiB", body = ErrorState),
        (status = 422, description = "Loader validation rejected the configured client", body = ErrorState),
        (status = 503, description = "The client or loader is not configured or available", body = ErrorState),
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

fn tracked_status(state: &ApiState, pid: u32) -> Result<ClientSnapshotStatus, ApiError> {
    if pid == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_pid",
            "process ID must be greater than zero",
            None,
        ));
    }
    state
        .snapshot()
        .clients
        .iter()
        .find(|client| client.pid == pid)
        .map(|client| client.status)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "client_not_found",
                format!("process {pid} is not a discovered or configured client"),
                Some(pid),
            )
        })
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
    paths(health, clients, load, unload, launch),
    components(schemas(
        HealthState,
        HealthStatus,
        ClientList,
        ClientState,
        ClientStatus,
        ClientIdentity,
        ConnectionMetadata,
        LaunchOptions,
        ServerEndpoint,
        LifecycleResult,
        LifecycleAction,
        ErrorState,
        ErrorDetail
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
    /// Current daemon connection state for this target.
    status: ClientStatus,
    /// Stable process and DLL identity, once observed.
    identity: Option<ClientIdentity>,
    /// Last accepted connection metadata, when available.
    connection: Option<ConnectionMetadata>,
    /// Human-readable detail for disconnected or incompatible targets.
    reason: Option<String>,
}

impl From<&ClientSnapshot> for ClientState {
    fn from(client: &ClientSnapshot) -> Self {
        Self {
            pid: client.pid,
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
    /// Per-load DLL instance identifier encoded as 32 uppercase hexadecimal digits.
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
    /// Supported executable fingerprint encoded as uppercase hexadecimal.
    executable_fingerprint: String,
    /// Supported client layout identifier.
    layout_id: u32,
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
            layout_id: hello.layout_id,
        }
    }
}

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
struct LaunchOptions {
    /// Allow another Dark Ages client process to start.
    allow_multiple: bool,
    /// Skip the introductory video.
    skip_intro: bool,
    /// Skip the title-screen notice and its associated delays.
    skip_notice: bool,
    /// Override the game server through the supported endpoint patch bundle.
    server: Option<ServerEndpoint>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ServerEndpoint {
    /// IPv4 address or hostname accepted by the supported game client.
    host: String,
    /// Server port. Defaults to 2610 when omitted.
    #[schema(minimum = 1, maximum = 65535)]
    port: Option<u16>,
}

impl TryFrom<LaunchOptions> for ManagedLaunchOptions {
    type Error = ApiError;

    fn try_from(options: LaunchOptions) -> Result<Self, Self::Error> {
        let server = options.server.map(validate_server).transpose()?;
        Ok(Self {
            allow_multiple: options.allow_multiple,
            skip_intro: options.skip_intro,
            skip_notice: options.skip_notice,
            server,
        })
    }
}

fn validate_server(server: ServerEndpoint) -> Result<ManagedServerEndpoint, ApiError> {
    let host = server.host;
    if host.is_empty()
        || host.trim() != host
        || host.len() > 255
        || host.chars().any(|character| {
            character.is_control() || character.is_whitespace() || character == ':'
        })
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_server",
            "server host must be a nonempty IPv4 address or hostname without whitespace or a port",
            None,
        ));
    }
    let port = server.port.unwrap_or(DEFAULT_SERVER_PORT);
    if port == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_server",
            "server port must be an integer from 1 through 65535",
            None,
        ));
    }
    Ok(ManagedServerEndpoint { host, port })
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct LifecycleResult {
    operation: LifecycleAction,
    pid: u32,
    changed: bool,
    darpc_loaded: bool,
}

impl LifecycleResult {
    fn unchanged(operation: LifecycleOperation, pid: u32, darpc_loaded: bool) -> Self {
        Self {
            operation: LifecycleAction::from(operation),
            pid,
            changed: false,
            darpc_loaded,
        }
    }
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
struct ApiError {
    status: StatusCode,
    body: ErrorState,
}

impl ApiError {
    fn new(
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
            "loader_unavailable" | "launch_not_configured" => StatusCode::SERVICE_UNAVAILABLE,
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
struct ErrorState {
    error: ErrorDetail,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
struct ErrorDetail {
    code: String,
    message: String,
    pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::{ApiState, ClientList, LaunchOptions, router};
    use crate::{
        event::DaemonEvent,
        management::{
            LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
            LifecycleOutcome, ManagementError,
        },
        registry::{ConnectionEvent, Registry},
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};
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
            layout_id: 741,
        }
    }

    fn state() -> ApiState {
        let mut registry = Registry::new();
        registry.apply(&ConnectionEvent::Connected {
            pid: 42,
            hello: hello(),
            selected_version: SUPPORTED_VERSIONS.max,
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
    fn serves_health_and_client_snapshots() {
        assert_eq!(json("/health")["status"], "ok");

        let clients = json("/clients");
        assert_eq!(clients["clients"][0]["pid"], 42);
        assert_eq!(clients["clients"][0]["status"], "connected");
        assert_eq!(
            clients["clients"][0]["identity"]["created_time"],
            "134299999186432946"
        );
        assert_eq!(
            clients["clients"][0]["connection"]["protocol_version"],
            "1.0"
        );

        let state = state();
        let mut registry = Registry::new();
        registry.apply(&ConnectionEvent::NotLoaded { pid: 7 });
        state.publish(registry.snapshot());
        assert_eq!(state.snapshot().clients[0].pid, 7);
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
        assert!(openapi["paths"]["/clients/launch"].is_object());
        assert!(openapi["paths"]["/clients/{pid}/load"].is_object());
        assert!(openapi["paths"]["/clients/{pid}/unload"].is_object());
        let schemas = openapi["components"]["schemas"].as_object().unwrap();
        for name in [
            "HealthState",
            "HealthStatus",
            "ClientList",
            "ClientState",
            "ClientStatus",
            "ClientIdentity",
            "ConnectionMetadata",
            "LaunchOptions",
            "ServerEndpoint",
            "LifecycleResult",
            "LifecycleAction",
            "ErrorState",
            "ErrorDetail",
        ] {
            assert!(schemas.contains_key(name), "OpenAPI omitted {name}");
        }

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
            r#"{"allow_multiple":true,"server":{"host":"127.0.0.1"}}"#,
        );
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(matches!(receiver.recv().unwrap(), DaemonEvent::Track(77)));
    }

    #[test]
    fn rejects_arbitrary_launch_arguments() {
        let (state, _receiver) = state_with_status(ConnectionEvent::NotLoaded { pid: 42 });
        let response = post_json(state, "/clients/launch", r#"{"arguments":["unsafe"]}"#);
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn defaults_the_server_port_without_exposing_paths() {
        let request: LaunchOptions =
            serde_json::from_str(r#"{"server":{"host":"da0.kru.com"}}"#).unwrap();
        let options = ManagedLaunchOptions::try_from(request).unwrap();
        let server = options.server.unwrap();
        assert_eq!(server.host, "da0.kru.com");
        assert_eq!(server.port, 2610);

        for field in ["client_path", "loader_path", "dll_path"] {
            let body = format!(r#"{{"{field}":"unsafe"}}"#);
            assert!(serde_json::from_str::<LaunchOptions>(&body).is_err());
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
