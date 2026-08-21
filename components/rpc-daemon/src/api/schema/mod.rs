use super::*;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(super) struct HealthState {
    pub(super) status: HealthStatus,
    pub(super) version: &'static str,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum HealthStatus {
    Ok,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(super) struct ClientList {
    pub(super) clients: Vec<ClientState>,
}

impl From<&RegistrySnapshot> for ClientList {
    fn from(snapshot: &RegistrySnapshot) -> Self {
        Self {
            clients: snapshot.clients.iter().map(ClientState::from).collect(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(super) struct ClientState {
    /// Configured operating-system process identifier.
    pub(super) pid: u32,
    /// Current endpoint identifier: in-game character name or process ID.
    pub(super) name: String,
    /// Current daemon connection state for this target.
    pub(super) status: ClientStatus,
    /// Stable process and DLL identity, once observed.
    pub(super) identity: Option<ClientIdentity>,
    /// Last accepted connection metadata, when available.
    pub(super) connection: Option<ConnectionMetadata>,
    /// Human-readable detail for disconnected or incompatible targets.
    pub(super) reason: Option<String>,
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
pub(super) enum ClientStatus {
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
pub(super) struct ClientIdentity {
    /// Unsigned 64-bit Windows process creation time encoded in decimal.
    pub(super) created_time: String,
    /// Per-load DLL instance identifier encoded as 32 lowercase hexadecimal digits.
    pub(super) instance_id: String,
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
pub(super) struct ConnectionMetadata {
    /// Negotiated daRPC binary protocol version.
    pub(super) protocol_version: String,
    /// Architecture reported by the injected DLL.
    pub(super) architecture: String,
    /// Semantic version of the injected DLL.
    pub(super) dll_version: String,
    /// Supported executable fingerprint encoded as lowercase hexadecimal.
    pub(super) executable_fingerprint: String,
    /// Supported Dark Ages client version.
    pub(super) client_version: String,
}

impl ConnectionMetadata {
    pub(super) fn new(hello: Hello, selected_version: u16) -> Self {
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
pub(super) struct LaunchOptions {
    /// Full path to the supported Dark Ages client executable.
    pub(super) client_path: String,
    /// Allow another Dark Ages client process to start.
    #[serde(default)]
    pub(super) allow_multiple: bool,
    /// Reveal up to 255 ground items while either Alt key is held.
    #[serde(default)]
    pub(super) show_items_with_alt: bool,
    /// Replace completed and cancelled exchange alerts with floating messages.
    #[serde(default)]
    pub(super) skip_exchange_alerts: bool,
    /// Skip the introductory video.
    #[serde(default)]
    pub(super) skip_intro: bool,
    /// Skip the title-screen notice and its associated delays.
    #[serde(default)]
    pub(super) skip_notice: bool,
    /// Override the game server as `host` or `host:port`; the port defaults to 2610.
    #[serde(default)]
    pub(super) server: Option<String>,
}

impl TryFrom<LaunchOptions> for ManagedLaunchOptions {
    type Error = ApiError;

    fn try_from(options: LaunchOptions) -> Result<Self, Self::Error> {
        let client_path = validate_client_path(options.client_path)?;
        let server = options.server.map(validate_server).transpose()?;
        Ok(Self {
            client_path,
            allow_multiple: options.allow_multiple,
            show_items_with_alt: options.show_items_with_alt,
            skip_exchange_alerts: options.skip_exchange_alerts,
            skip_intro: options.skip_intro,
            skip_notice: options.skip_notice,
            server,
        })
    }
}

pub(super) fn validate_client_path(client_path: String) -> Result<std::path::PathBuf, ApiError> {
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

pub(super) fn validate_server(server: String) -> Result<ManagedServerEndpoint, ApiError> {
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

pub(super) fn invalid_server(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_server", message, None)
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(super) struct LoadResult {
    pub(super) operation: LifecycleAction,
    pub(super) pid: u32,
    pub(super) was_loaded: bool,
}

impl LoadResult {
    pub(super) fn unchanged(pid: u32) -> Self {
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
pub(super) struct UnloadResult {
    pub(super) operation: LifecycleAction,
    pub(super) pid: u32,
    pub(super) was_unloaded: bool,
}

impl UnloadResult {
    pub(super) fn unchanged(pid: u32) -> Self {
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
pub(super) struct LifecycleResult {
    pub(super) operation: LifecycleAction,
    pub(super) pid: u32,
    pub(super) changed: bool,
    pub(super) darpc_loaded: bool,
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
pub(super) enum LifecycleAction {
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
    pub(super) status: StatusCode,
    pub(super) body: ErrorState,
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
    pub(super) error: ErrorDetail,
}

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct ErrorDetail {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) pid: Option<u32>,
}
