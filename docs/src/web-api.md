# Web API

> **Status:** The client registry, managed lifecycle routes, OpenAPI document,
> interactive documentation, and current client snapshot query are implemented.
> Game actions, streaming APIs, and remote access remain planned.

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
| `GET /clients/{client}/snapshot` | Return the latest complete snapshot observed from one connected client. |
| `POST /clients/launch` | Launch the configured client and initialize the configured DLL. |
| `POST /clients/{client}/load` | Load and initialize the configured DLL in a tracked client. |
| `POST /clients/{client}/unload` | Shut down and unload the configured DLL from a tracked client. |
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
    name: string,
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
    client_version: string,
}
```

Every `{client}` path accepts a decimal PID or a case-insensitive current
character name. PID addressing is always available. Name addressing is
available only for a connected snapshot whose lifecycle is `in_game`; title,
transition, disconnected, and stale observations fall back to the decimal PID.
`ClientState.name` exposes the currently eligible path value. A duplicate
active name is rejected as ambiguous rather than selecting an arbitrary
process.

`created_time` is the unsigned 64-bit Windows process creation time encoded as
a decimal string so JavaScript consumers do not lose precision. `instance_id`
and `executable_fingerprint` are uppercase hexadecimal strings. Identity and
connection metadata are `null` until the corresponding information has been
observed. `client_version` is the supported Dark Ages release in dotted form,
such as `"7.41"`.

## Client snapshots

After connecting, the daemon requests a complete snapshot and retains it with
the corresponding client identity. `GET /clients/{client}/snapshot` returns that
latest observation. It returns `404 Not Found` for an unknown PID and `503
Service Unavailable` when the target has not produced a snapshot, including a
capture failure reason when one is available.

The response identifies the source process and contains capture metadata,
lifecycle, and an optional character. Character state includes identity,
appearance, progression, attributes, vitals, modifiers, map location,
inventory, equipment, spellbook, and skillbook. Appearance is flattened into
`gender`, `hair_style`, `hair_color`, and `body_sprite`; all four are null when
the local character is using a monster-disguise image session. `action_locked`
reports the separate local movement, world-drop, exchange-start, and inventory
rearrangement lock; it is not a promise that every possible action is blocked.
`is_blinded` follows the client-retained `SStatus` blind code.
Occupied slots are arrays; an absent array means the client could not expose
that group, while an empty array means the group was read successfully and
contained no occupied slots. Item sprites exclude the client's type flag bits,
stackable item names exclude the rendered quantity suffix, and `can_stack`
retains the independent client flag. Equipment `slot` is a stable snake-case
name such as `left_ring` or `accessory1`.

Text-input spells expose their ASCII-only prompt. Other spell target modes
have a null prompt. Element and target-type names are exposed without duplicate
numeric identifier fields.

The `disconnected` lifecycle means that the active client is displaying its
reconnect dialog. Character state remains present when the underlying world
structures are still valid, so consumers should use the lifecycle rather than
the presence of a character to decide whether the session is connected.

Known enum values are represented with readable snake-case names. Duplicate
numeric identifiers for character class and gender are not exposed. Raw memory
addresses and version-specific layout details are never exposed.

Snapshot capture semantics, unavailable values, and collection behavior are
documented in [Client state](state.md). The complete generated JSON schema is
available in `/openapi.json` and Swagger UI.

## Managed lifecycle

Load and unload requests have no body. They operate only on PIDs already
discovered or explicitly configured in the daemon. The loader and DLL paths are
daemon configuration, never request data.

Launch requires a JSON object containing the full path to the supported client
executable. The smallest request is:

```json
{
  "client_path": "C:\\Games\\Dark Ages\\Darkages.exe"
}
```

The complete request model is:

```text
LaunchOptions {
    client_path: string,
    allow_multiple: bool = false,
    skip_intro: bool = false,
    skip_notice: bool = false,
    server: string?,
}
```

For example:

```json
{
  "client_path": "D:\\Games\\Dark Ages\\Darkages.exe",
  "allow_multiple": true,
  "skip_intro": true,
  "skip_notice": true,
  "server": "127.0.0.1:2610"
}
```

`client_path` must be a fully qualified Windows drive or Universal Naming
Convention (UNC) path. `loader.exe` validates the selected file as the supported
client before creating a process and uses its parent directory as the client
working directory.

`server` accepts `host` or `host:port`. The host must be a nonempty IPv4 address
or hostname without whitespace or control characters. The port defaults to
2610 and, when present, must be from 1 through 65535. IPv6 literals are not
supported. Supplying `server` activates the loader's supported endpoint and
fallback patch bundle. `skip_notice` activates its full notice,
early-continue, and fast-transfer patch group.

Unknown fields are rejected. In particular, the API does not accept arbitrary
client arguments or request-selected loader and DLL paths. The game client does
not have a supported general-purpose argument surface.

Successful load operations return:

```text
LoadResult {
    operation: "load",
    pid: u32,
    was_loaded: bool,
}
```

Successful unload operations return:

```text
UnloadResult {
    operation: "unload",
    pid: u32,
    was_unloaded: bool,
}
```

`was_loaded` and `was_unloaded` report whether that request performed the
corresponding state transition. They are `false` when the DLL was already in
the requested state.

Successful launch operations return:

```text
LifecycleResult {
    operation: "launch",
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
