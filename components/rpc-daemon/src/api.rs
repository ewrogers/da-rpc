use crate::registry::{
    ClientIdentity as RegistryClientIdentity, ClientSnapshot, ClientSnapshotStatus,
    RegistrySnapshot, architecture, hex,
};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    middleware::{Next, from_fn},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use darpc_protocol::{Hello, protocol_version_major, protocol_version_minor};
use serde::Serialize;
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

const SWAGGER_INDEX: &str = include_str!("../assets/swagger.html");
const SWAGGER_THEME: &str = include_str!("../assets/swagger-ayu.css");

#[derive(Clone)]
pub(crate) struct ApiState {
    snapshot: Arc<RwLock<Arc<RegistrySnapshot>>>,
}

impl ApiState {
    #[must_use]
    pub(crate) fn new(snapshot: RegistrySnapshot) -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(Arc::new(snapshot))),
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
        .route("/docs", get(swagger_redirect))
        .route("/docs/", get(swagger_index))
        .route("/docs/ayu.css", get(swagger_theme))
        .merge(SwaggerUi::new("/docs/assets").url("/openapi.json", openapi()))
        .layer(from_fn(reject_request_body))
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

#[derive(OpenApi)]
#[openapi(
    paths(health, clients),
    components(schemas(
        HealthState,
        HealthStatus,
        ClientList,
        ClientState,
        ClientStatus,
        ClientIdentity,
        ConnectionMetadata
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

#[cfg(test)]
mod tests {
    use super::{ApiState, ClientList, router};
    use crate::registry::{ConnectionEvent, Registry};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};
    use serde_json::Value;
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
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
        ApiState::new(registry.snapshot())
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
        registry.apply(&ConnectionEvent::NotLoaded { pid: 2 });
        let mut connected = hello();
        connected.process_id = 3;
        registry.apply(&ConnectionEvent::Connected {
            pid: 3,
            hello: connected,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        registry.apply(&ConnectionEvent::Busy { pid: 4 });
        registry.apply(&ConnectionEvent::Disconnected {
            pid: 5,
            identity: None,
            reason: "closed".into(),
        });
        registry.apply(&ConnectionEvent::Incompatible {
            pid: 6,
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
        let schemas = openapi["components"]["schemas"].as_object().unwrap();
        for name in [
            "HealthState",
            "HealthStatus",
            "ClientList",
            "ClientState",
            "ClientStatus",
            "ClientIdentity",
            "ConnectionMetadata",
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
