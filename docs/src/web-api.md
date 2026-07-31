# Web API

> **Status:** The client registry, managed lifecycle routes, OpenAPI document,
> and interactive documentation are implemented. Game-state queries, game
> actions, streaming APIs, and remote access remain planned.

`darpcd.exe` exposes standard web interfaces so applications do not need to
implement Windows injection, named-pipe IPC, or client-specific data layouts.

## Current REST surface

The Axum server binds to `127.0.0.1:2626` by default. A single
`--port <port>` option accepts values from 1 through 65535 and changes only the
port. If the address is unavailable, startup fails instead of selecting another
port. The API has no URL version prefix.

| Route | Purpose |
| --- | --- |
| `GET /health` | Report daemon availability. |
| `GET /clients` | List discovered and configured targets, identity, compatibility, and connection health. |
| `POST /clients/launch` | Launch the configured client and initialize the configured DLL. |
| `POST /clients/{pid}/load` | Load and initialize the configured DLL in a tracked client. |
| `POST /clients/{pid}/unload` | Shut down and unload the configured DLL from a tracked client. |
| `GET /openapi.json` | Return the generated OpenAPI document. |
| `GET /docs` | Open the self-hosted interactive Swagger UI. |

With the default port, Swagger is available at
`http://127.0.0.1:2626/docs`. Its vendored assets and Ayu-inspired dark theme
work without an internet connection.

## Registry models

```text
HealthState {
    status: "ok",
    version: string,
}

ClientList {
    clients: ClientState[],
}

ClientState {
    pid: u32,
    status: "connecting" | "not_loaded" | "initializing" | "connected" |
            "busy" | "disconnected" | "incompatible",
    identity: ClientIdentity?,
    connection: ConnectionMetadata?,
    reason: string?,
}

ClientIdentity {
    created_time: string,
    instance_id: string,
}

ConnectionMetadata {
    protocol_version: string,
    architecture: "x86" | "x86_64",
    dll_version: string,
    executable_fingerprint: string,
    layout_id: u32,
}
```

`created_time` is the unsigned 64-bit Windows process creation time encoded as
a decimal string so JavaScript consumers do not lose precision. `instance_id`
and `executable_fingerprint` are uppercase hexadecimal strings. Identity and
connection metadata are `null` until the corresponding information has been
observed.

The registry does not contain game-state fields yet.

## Managed lifecycle

Load and unload requests have no body. They operate only on PIDs already
discovered or explicitly configured in the daemon. The loader and DLL paths are
daemon configuration, never request data.

Launch requires a JSON object. An empty object selects normal client startup:

```json
{}
```

The complete request model is:

```text
LaunchOptions {
    allow_multiple: bool = false,
    skip_intro: bool = false,
    skip_notice: bool = false,
    server: ServerEndpoint?,
}

ServerEndpoint {
    host: string,
    port: u16 = 2610,
}
```

For example:

```json
{
  "allow_multiple": true,
  "skip_intro": true,
  "skip_notice": true,
  "server": {
    "host": "127.0.0.1",
    "port": 2610
  }
}
```

`host` must be a nonempty IPv4 address or hostname without whitespace, control
characters, or a port suffix. Supplying `server` activates the loader's
supported endpoint and fallback patch bundle. `skip_notice` activates its full
notice, early-continue, and fast-transfer patch group.

Unknown fields are rejected. In particular, the API does not accept arbitrary
client arguments or client, loader, or DLL paths. The configured game client
does not have a supported general-purpose argument surface.

Successful lifecycle operations return:

```text
LifecycleResult {
    operation: "load" | "unload" | "launch",
    pid: u32,
    changed: bool,
    darpc_loaded: bool,
}
```

`load` returns `200 OK` when the client is already connected and `202 Accepted`
after a new load. `unload` returns `200 OK`. `launch` returns `201 Created` after
the loader has initialized and resumed the new process. Registry connection is
asynchronous, so consumers should observe `GET /clients` until the returned PID
reaches `connected` or a terminal error state.

Malformed requests, unknown candidates, concurrent operations, validation
failures, unavailable configured tools, and loader timeouts use the applicable
4xx or 5xx status. Managed-operation errors have this shape:

```text
ErrorState {
    error: ErrorDetail,
}

ErrorDetail {
    code: string,
    message: string,
    pid: u32?,
}
```

The launch body is limited to 4 KiB. Other current routes reject nonempty
request bodies. Unsupported methods and paths use the normal `405 Method Not
Allowed` and `404 Not Found` responses.

## OpenAPI and interactive documentation

`utoipa` generates `/openapi.json` from the Rust HTTP models and route
descriptions. The document can be imported into Postman, Apidog, or another
OpenAPI consumer.

`utoipa-swagger-ui` serves `/docs` with vendored assets. The UI is a developer
convenience layered over the API: a rendering failure cannot affect JSON
routes, the registry, or DLL connections.

The OpenAPI `info.version` follows the daRPC release that produced the
document. It does not imply URL versioning. daRPC maintains one current API and
will add a compatibility mechanism only if real consumers require simultaneous
incompatible schemas.

The broader planned interfaces have distinct roles:

| Interface | Primary role |
| --- | --- |
| REST | Discovery, current-state queries, configuration, and discrete actions. |
| Server-Sent Events | One-way real-time event and state-update streams. |
| WebSocket | Bidirectional real-time communication where interactive request and event traffic share a connection. |

## Aggregation

One daemon may manage multiple game clients. External models must identify the
source client without exposing raw process pointers or version-specific memory
layouts. A slow client or API consumer must not block unrelated connections.

API models remain separate from both client memory layouts and binary protocol
models so each boundary can evolve deliberately.

## Remote access

The implemented listener is loopback-only. Any future remote-listening mode
must define authentication, authorization, request limits, and transport
security before it is considered safe for general use.

Server-Sent Events and WebSocket APIs must also define ordering, lag,
disconnect, and replay behavior. A connection must not silently imply that
transient events generated before subscription will be replayed.
