# Web API

> **Status:** The client registry, managed lifecycle routes, OpenAPI document,
> interactive documentation, current client state queries, a diagnostic
> main-thread command, and per-client Server-Sent Events are implemented.
> Typed game actions, WebSocket, and remote access remain planned.

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
| `GET /clients/{client}/status` | Return lifecycle, character status, and map state. |
| `GET /clients/{client}/inventory` | Return occupied inventory slots. |
| `GET /clients/{client}/equipment` | Return occupied equipment slots. |
| `GET /clients/{client}/spellbook` | Return occupied spellbook slots. |
| `GET /clients/{client}/skillbook` | Return occupied skillbook slots. |
| `GET /clients/{client}/effects` | Return active spell effects and relative duration bands. |
| `GET /clients/{client}/events` | Stream ordered changes after a current snapshot boundary. |
| `POST /clients/{client}/commands/diagnostic` | Submit a no-op command for execution on a client tick. |
| `GET /clients/{client}/commands/{command_id}` | Read a retained command state. |
| `DELETE /clients/{client}/commands/{command_id}` | Cancel a command that has not started. |
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
and `executable_fingerprint` are lowercase hexadecimal strings. Identity and
connection metadata are `null` until the corresponding information has been
observed. `client_version` is the supported Dark Ages release in dotted form,
such as `"7.41"`.

## Client state resources

After connecting, the daemon requests one complete snapshot and retains it with
the corresponding client identity. The public API presents focused resource
views of that retained observation instead of returning the entire snapshot in
one response:

```text
GameStatus { observation, lifecycle, character, map }
Inventory { observation, items }
Equipment { observation, items }
Spellbook { observation, spells }
Skillbook { observation, skills }
Effects { observation, effects }
```

Every response includes `observation` metadata with the source PID, revision,
event sequence, capture tick, latest update tick, capture duration, and world
generation. The capture fields describe the last complete memory walk;
`updated_tick_ms` advances when an incremental update is applied. Consumers can
correlate responses with the same revision. Separate HTTP requests can observe
different revisions when the daemon receives a newer update between requests.

All six routes return `404 Not Found` for an unknown client and `503 Service
Unavailable` when the target has not produced an observation, including a
capture failure reason when one is available. A collection field is null when
the client could not expose that group and an empty array when the group was
read successfully but contained no occupied slots.

An effect contains an icon and a relative `duration` band. It is not an exact
remaining time. From longest to shortest, the values are `white`, `red`,
`orange`, `yellow`, `green`, and `blue`.

Character status contains identity, appearance, progression, attributes,
vitals, weight, maximum weight, and modifiers. Map state is a separate top-level field in
`GameStatus`. Appearance is flattened into `gender`, `hair_style`, `hair_color`,
and `body_sprite`; all four are null when the local character is using a
monster-disguise image session. `is_action_restricted` reports the separate
local movement, world-drop, exchange-start, and inventory rearrangement lock;
it is not a promise that every possible action is blocked. `is_blinded` follows
the client-retained `SStatus` blind code. Item sprites exclude the client's type
flag bits, stackable item names exclude the rendered quantity suffix, and
`can_stack` retains the independent client flag. Equipment `slot` is a stable
snake-case name such as `left_ring` or `accessory1`.

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

Snapshot capture semantics and unavailable values are documented in
[Client state](state.md). The complete generated JSON schema is available in
`/openapi.json` and Swagger UI.

## Main-thread commands

The diagnostic route accepts this optional field in a JSON object:

```text
DiagnosticOptions {
    timeout_ms: u16,  // default 1,000; valid range 1 through 5,000
}
```

It returns the current command state. `200 OK` means the command reached a
terminal state during the bounded wait; `202 Accepted` means it remains queued.
The same `command_id` can be queried or cancelled through the routes above.

```text
CommandStatus {
    pid: u32,
    instance_id: string,
    command_id: u32,
    kind: "diagnostic",
    state: "accepted" | "executed" | "failed" | "cancelled" | "timed_out",
    enqueued_tick_ms: u32,
    deadline_tick_ms: u32,
    started_tick_ms: u32?,
    completed_tick_ms: u32?,
    queue_delay_ms: u32?,
    execution_us: u32?,
    main_thread_id: u32?,
    failure: "internal"?,
}
```

Command IDs are local to the reported DLL `instance_id`. Results from a prior
DLL lifetime must not be applied to a replacement instance. Terminal results
are retained for bounded queries and may be evicted under pressure.

The HTTP thread sends requests through a 64-entry daemon router. Each client
has its own 16-entry worker channel and existing named-pipe session, so the
daemon never opens a competing controller connection and one client cannot
consume another client's worker capacity. The DLL then enqueues into its own
64-slot pointer-free queue. Full daemon or DLL queues return `429 Too Many
Requests` immediately; unavailable connections return `503 Service
Unavailable`; an expired retained command returns `404 Not Found`.

The client tick executes at most one command per tick. IPC, HTTP, allocation,
serialization, and logging stay off the game thread. The diagnostic calls no
client function and changes no game state.

## Server-Sent Events

`GET /clients/{client}/events` requires a connected client with a current
observation. The daemon subscribes to its bounded internal broadcast channel
before reading that observation, then begins with:

```text
id: 38
event: stream.ready
data: {"type":"stream_ready","data":{"pid":6964,"instance_id":"...","revision":40,"event_sequence":38}}
```

This establishes the exact state boundary already available through the REST
resources. Every frame follows the same transport structure:

```text
id: <event_sequence>
event: <routing name>
data: {"type":"<schema discriminator>","data":{...}}
```

The SSE `event` field selects an `EventSource` listener. The JSON `type` field
selects the corresponding `ClientEvent` variant in generated clients. Routing
names use the established public spelling, while JSON discriminators always use
snake case. For example, `vitals.changed` carries `type: "vitals_changed"`.

Later state changes contain a common observation:

```text
EventObservation {
    pid: u32,
    instance_id: string,
    revision: u32,
    event_sequence: u32,
    tick_ms: u32,
}
```

`tick_ms` is the client's wrapping Windows millisecond tick. Implemented frames
are:

| SSE event | JSON `type` | Payload after `observation` |
| --- | --- | --- |
| `stream.ready` | `stream_ready` | `pid`, `instance_id`, `revision`, `event_sequence` |
| `stats.changed` | `stats_changed` | All five character attributes |
| `vitals.changed` | `vitals_changed` | Changed current or maximum health and mana fields |
| `progression.changed` | `progression_changed` | Changed level, ability, and experience fields |
| `gold.changed` | `gold_changed` | `gold` |
| `weight.changed` | `weight_changed` | `weight`, `max_weight` |
| `modifiers.changed` | `modifiers_changed` | Combat modifiers and attack and defense elements |
| `location.changed` | `location_changed` | Absolute `x`, `y`, and optional atomic `map` change |
| `blind.changed` | `blind_changed` | `is_blinded` |
| `action_restriction.changed` | `action_restriction_changed` | `is_action_restricted` |
| `effect_added` | `effect_added` | `icon`, `duration` |
| `effect_removed` | `effect_removed` | `icon` |
| `effect_changed` | `effect_changed` | `icon`, new `duration` |
| `stream.resync_required` | `stream_resync_required` | `pid`, `instance_id`, `last_event_sequence`, `dropped_events` |
| `stream.closed` | `stream_closed` | `pid`, `instance_id`, `last_event_sequence`, `reason` |

Several events may share one sequence and revision when one atomic client
packet changed several groups. Values are absolute. Consumers replace the
included fields instead of adding a delta.

Effect events identify the icon. Added and changed events also carry its new
relative duration band. Removed events carry no duration because the icon is no
longer active.

```text
id: 40
event: effect_added
data: {"type":"effect_added","data":{"observation":{"pid":6964,"instance_id":"...","revision":42,"event_sequence":40,"tick_ms":84156449},"icon":10,"duration":"yellow"}}
```

A browser subscribes to the transport event name and parses the JSON envelope:

```javascript
const events = new EventSource("/clients/Eidolon/events");
events.addEventListener("effect_added", (event) => {
  const message = JSON.parse(event.data);
  console.log(message.data.icon, message.data.duration);
});
```

`location.changed` always includes absolute x/y. Its optional `map` object is
present only when the same event atomically changes map identity, name,
dimensions, and position.

There is no replay of events created before subscription. A reconnect receives
a new `stream.ready` boundary and should read the desired REST resources. The
daemon retains 4,096 broadcast entries. If a subscriber falls behind, it
receives `stream.resync_required` with its last delivered sequence and the
dropped count, then the connection closes. A process disconnect emits
`stream.closed` and closes the stream. Fifteen-second SSE comments keep an idle
connection observable without changing state ordering.

OpenAPI declares the response as `text/event-stream`. Its `ClientEvent` `oneOf`
schema describes the JSON envelope and every payload referenced by `data`.
OpenAPI does not model the surrounding `id:` and `event:` lines, keepalive
comments, ordering, replay, or disconnect rules, so this chapter defines those
transport semantics. Swagger UI exposes the generated schemas but is not a live
SSE viewer. A browser `EventSource`, command-line SSE client, or API tool with
streaming support can consume the endpoint directly.

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
