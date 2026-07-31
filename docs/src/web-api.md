# Web API

> **Status:** The daemon registry is implemented. The HTTP server, OpenAPI
> document, and interactive documentation described here are planned.

`darpcd.exe` exposes standard web interfaces so applications do not need to
implement Windows injection, named-pipe IPC, or client-specific data layouts.

## Initial read-only surface

The initial server uses Axum and binds to a loopback address. It exposes one
current API without a URL version prefix:

| Route | Purpose |
| --- | --- |
| `GET /health` | Report daemon availability. |
| `GET /clients` | List configured targets, identities, compatibility, and connection health. |
| `GET /openapi.json` | Return the generated OpenAPI document for tools and code generators. |
| `GET /docs` | Open the self-hosted interactive Swagger UI. |

The client list represents connecting, connected, disconnected, busy, and
incompatible targets explicitly. It contains only information already owned by
the registry until snapshot messages and game-state models are implemented.

Requests and responses use bounded, dedicated HTTP models. They do not expose
registry implementation details, binary protocol messages, client layouts, or
raw process pointers. HTTP handling must not block a client worker or hold a
registry lock across network I/O.

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
