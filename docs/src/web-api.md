# Web API

> **Status:** The read-only HTTP server, OpenAPI document, and interactive
> documentation described in the initial surface are implemented. State
> snapshots, actions, streaming APIs, and remote access remain planned.

`darpcd.exe` exposes standard web interfaces so applications do not need to
implement Windows injection, named-pipe IPC, or client-specific data layouts.

## Initial read-only surface

The server uses Axum and binds to `127.0.0.1:2626` by default. A single
`--port <port>` option accepts values from 1 through 65535 and changes only the
port. Port zero, repeated options, and malformed values are rejected. If the
selected address is unavailable, daemon startup fails instead of choosing a
different port. It exposes one current API without a URL version prefix:

| Route | Purpose |
| --- | --- |
| `GET /health` | Report daemon availability. |
| `GET /clients` | List configured targets, identities, compatibility, and connection health. |
| `GET /openapi.json` | Return the generated OpenAPI document for tools and code generators. |
| `GET /docs` | Open the self-hosted interactive Swagger UI. |

With the default port, the interactive documentation is available at
`http://127.0.0.1:2626/docs`.

The JSON response shapes are:

```text
HealthResponse {
    status: "ok",
    version: string,
}

ClientsResponse {
    clients: Client[],
}

Client {
    pid: u32,
    status: "connecting" | "not_loaded" | "connected" | "busy" |
            "disconnected" | "incompatible",
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
a decimal string. The string representation preserves its exact identity value
for JavaScript consumers. `instance_id` and `executable_fingerprint` are
uppercase hexadecimal strings. Identity and connection metadata are `null`
until the corresponding information has been observed.

The client list represents connecting, connected, disconnected, busy, and
incompatible targets explicitly. It contains only information already owned by
the registry until snapshot messages and game-state models are implemented.

Requests and responses use bounded, dedicated HTTP models. They do not expose
registry implementation details, binary protocol messages, client layouts, or
raw process pointers. HTTP handling must not block a client worker or hold a
registry lock across network I/O.

The current routes accept no request body. A nonzero content length or transfer
encoding receives `413 Payload Too Large`. Unsupported methods and unknown
paths receive the normal `405 Method Not Allowed` and `404 Not Found`
responses. The daemon never falls back to another port when binding fails.

## OpenAPI and interactive documentation

`utoipa` generates the OpenAPI document from the Rust HTTP models and route
descriptions. The specification served by `/openapi.json` is the contract used
by the interactive documentation and can be imported into Postman, Apidog, or
another OpenAPI consumer.

`utoipa-swagger-ui` serves `/docs` with vendored assets. The UI therefore works
without a content delivery network or runtime internet access. It is a
developer convenience layered over the API: failure to render the UI must not
affect the JSON routes, registry, or DLL connections.

The OpenAPI `info.version` follows the daRPC release that produced the
document. It does not imply URL versioning. daRPC maintains a single current
HTTP API and will add an explicit compatibility mechanism only if real
consumers require simultaneous incompatible schemas.

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

API models should remain separate from both client memory layouts and binary
protocol models. This allows each boundary to evolve deliberately.

## Remote access

The daemon can support applications on another operating system because the
external boundary uses web protocols. Network listeners should default to the
least exposed practical interface. Any remote-listening mode must define
authentication, authorization, request limits, and transport-security
expectations before it is considered safe for general use.

SSE and WebSocket APIs must also define ordering, lag, disconnect, and replay
behavior. A connection must not silently imply that transient events generated
before subscription will be replayed.
