# Web API

`darpcd.exe` turns Windows-specific client integration into ordinary local web
interfaces. A script or application can use REST and Server-Sent Events (SSE)
without implementing DLL injection, named pipes, or Dark Ages client layouts.

REST and SSE are the supported web interfaces.

If you are building a consumer, start here for routes and actions, then use
[Live events](events.md) for the complete streaming reference. The chapters
under [Game data](state.md) explain the meaning of character and world fields.

## Starting the server

The daemon listens on `127.0.0.1:2626` by default. Use `--port <port>` to change
only the loopback port, or `--listen <ipv4[:port]>` to select an explicit IPv4
interface. Startup fails if the address is already in use instead of silently
selecting a different port.

With the default port:

- `http://127.0.0.1:2626/docs` opens the self-hosted Swagger UI.
- `http://127.0.0.1:2626/openapi.json` returns the OpenAPI document.
- `http://127.0.0.1:2626/health` reports daemon availability.

The API has no URL version prefix. daRPC maintains one current API while it is
in active development.

## Choosing a client

Every `{client}` path accepts either:

- A decimal process ID, such as `6076`
- A current character name, such as `ZiLo`, matched without case sensitivity

Process ID addressing is always available for a discovered client. Character
name addressing is available only when a connected snapshot is `in_game`.
Title, transition, disconnected, and stale clients use their process ID. If two
active clients have the same character name, the name is rejected as ambiguous
rather than choosing one.

Use `GET /clients` to discover the current path value and connection state for
every tracked client.

Observation-backed routes return `503 Service Unavailable` when no complete
snapshot is available or when the daemon rejects an event batch during
reduction. The daemon keeps the last valid snapshot internally but does not
serve it as current data. A fresh snapshot restores the routes and establishes
the boundary for new Server-Sent Events streams.

## Client registry

```console
curl "http://127.0.0.1:2626/clients"
```

The registry reports each process ID, usable name, connection status, and any
available identity or compatibility details. Common statuses include
`not_loaded`, `initializing`, `connecting`, `connected`, `busy`,
`disconnected`, and `incompatible`.

`instance_id` identifies one loaded DLL lifetime. It changes after unload and
reload. Process creation time is encoded as a decimal string so JavaScript does
not lose 64-bit precision. Instance IDs and executable fingerprints use
lowercase hexadecimal text.

## Current data routes

The current game data is split by domain so consumers can request only what
they need:

| Route | Guide |
| --- | --- |
| `GET /clients/{client}/status` | [Character status](status.md) |
| `GET /clients/{client}/items` | [Inventory](inventory.md) |
| `GET /clients/{client}/equipment` | [Equipment](equipment.md) |
| `GET /clients/{client}/skills` | [Skills](skills.md) |
| `GET /clients/{client}/spells` | [Spells](spells.md) |
| `GET /clients/{client}/effects` | [Effects](effects.md) |
| `GET /clients/{client}/objects` | [World](world.md) |
| `GET /clients/{client}/messages` | [Messages](messages.md) |
| `GET /clients/{client}/dialog` | [NPC dialogs](dialogs.md) |
| `GET /clients/{client}/message-dialogs` | [Message dialogs](message-dialogs.md) |
| `GET /clients/{client}/field-map` | [Field maps](field-maps.md) |
| `GET /clients/{client}/bulletin` | [Bulletin boards and player mail](bulletins.md) |
| `GET /clients/{client}/group` | [Groups](groups.md) |
| `GET /clients/{client}/exchange` | [Exchange](exchanges.md) |
| `GET /clients/{client}/who` | [Online players](online.md) |
| `GET /clients/{client}/legend` | [Legend](legend.md) |
| `GET /clients/{client}/players/{player}` | Read one case-insensitive visible player from retained state; see [World](world.md). |
| `POST /clients/{client}/players/{player}/inspect` | Refresh one visible player; see [World](world.md). |
| `GET /maps/{map_id}/download` | Download one locally available raw map file. |

Most routes read the daemon's retained state and do not ask the DLL to scan the
game client for every HTTP request. The Who, Legend, and player-inspection
routes are bounded requests to the game server; Legend coalesces refreshes for
one second. See
[Game data](state.md) for baseline capture, revisions, and unavailable values,
[Online players](online.md) for Who timing and filters, and [Legend](legend.md)
for self-look refresh behavior.

The complete field list and JSON schema are generated from the Rust API models
and are available in Swagger. The domain guides explain what the fields mean in
the game and how they change.

## Map file downloads

The daemon automatically uses the `Maps` directory beside the first discovered
local `Darkages.exe`. Use `--maps-path <path>` only to override that directory.
A remote consumer can request `GET /maps/{map_id}/download`; map ID `3001`, for
example, reads `lod3001.map` from the selected directory.

A successful response is the exact file bytes with media type
`application/octet-stream` and an attachment filename. HTTP already transports
arbitrary bytes without corruption, so the endpoint does not add base64
encoding or JSON overhead. Missing files and the period before a client map
directory is discovered return `404` with the normal structured
`map_not_found` error. Files larger
than 4 MiB are rejected with `413`; the daemon does not read an unbounded file
into memory. Numeric 16-bit map IDs are the only accepted path input, so a
request cannot select another file in or outside the configured directory.

## Action routes

| Route | Purpose |
| --- | --- |
| `POST /clients/{client}/turn` | Face a cardinal direction. |
| `POST /clients/{client}/look` | Look at the tile directly ahead and emit the result asynchronously. |
| `POST /clients/{client}/far-look` | Look at one tile on the current map and emit the result asynchronously. |
| `POST /clients/{client}/walk` | Take one step, pathfind to a tile, or install an exact route. |
| `DELETE /clients/{client}/walk` | Cancel the active route. |
| `POST /clients/{client}/resync` | [Request the same server refresh as the F5 key.](#resynchronizing-a-client) |
| `POST /clients/{client}/skills/use` | Use a skill by slot or name. |
| `POST /clients/{client}/skills/swap` | Swap skills using slot-or-name selectors. |
| `POST /clients/{client}/spells/cast` | Cast a spell by slot or name. |
| `POST /clients/{client}/spells/swap` | Swap spells using slot-or-name selectors. |
| `POST /clients/{client}/items/use` | Use an inventory item by slot or name. |
| `POST /clients/{client}/items/drop` | Drop an item at a ground tile. |
| `POST /clients/{client}/items/give` | Give an item to a visible human, monster, or NPC. |
| `POST /clients/{client}/items/swap` | Swap inventory slots using slot-or-name selectors. |
| `POST /clients/{client}/items/pickup` | Pick up the top ground item at a tile. |
| `POST /clients/{client}/chant` | Send verbatim text as a spell chant. |
| `POST /clients/{client}/messages/send` | Send say, shout, guild, group, or whisper chat. |
| `POST /messages/send` | Send a daemon-only internal payload to one named client or all connected clients. |
| `POST /clients/{client}/items/sell` | Ask an NPC to buy one named item. |
| `POST /clients/{client}/items/sell-all` | Ask an NPC to buy all matching named items. |
| `POST /clients/{client}/items/deposit` | Deposit a named item with an NPC. |
| `POST /clients/{client}/items/withdraw` | Withdraw a named item from an NPC. |
| `POST /clients/{client}/items/repair` | Repair one named item through an NPC. |
| `POST /clients/{client}/items/repair-all` | Ask an NPC to repair all items. |
| `POST /clients/{client}/gold/drop` | Drop gold at a ground tile. |
| `POST /clients/{client}/gold/give` | Give gold to a visible human, monster, or NPC. |
| `POST /clients/{client}/equipment/unequip` | Unequip one readable equipment slot. |
| `POST /clients/{client}/emote` | Perform an emote by confirmed name or client code. |
| `POST /clients/{client}/raw/send` | Send a bounded custom client packet or dispatch a synthetic server packet. |
| `POST /clients/{client}/assail` | Submit the client's native basic-attack packet. |
| `POST /clients/{client}/stats/{stat}` | Spend one available point on `strength`/`str`, `dexterity`/`dex`, `intelligence`/`int`, `wisdom`/`wis`, or `constitution`/`con`. |
| `POST /clients/{client}/interact` | Start a conversation with a visible Mundane. |
| `POST /clients/{client}/dialog/select` | Select a row in the current NPC dialog. |
| `POST /clients/{client}/dialog/input` | Answer the current text prompt. |
| `POST /clients/{client}/dialog/previous` | Move to the previous pursuit page. |
| `POST /clients/{client}/dialog/next` | Move to the next pursuit page. |
| `POST /clients/{client}/dialog/close` | Close the current NPC dialog. |
| `POST /clients/{client}/message-dialogs/dismiss` | Dismiss one current [message dialog](message-dialogs.md). |
| `POST /clients/{client}/field-map/select` | Select one destination from the active [field map](field-maps.md). |
| `POST /clients/{client}/bulletin/actions` | Open, navigate, scroll, compose, or mutate [bulletin boards and player mail](bulletins.md). |
| `POST /clients/{client}/group/toggle` | Toggle invitations, or leave the current group. |
| `POST /clients/{client}/group/invite` | Invite a visible player. |
| `POST /clients/{client}/group/invitations/{id}/accept` | Accept a pending invitation. |
| `POST /clients/{client}/group/invitations/{id}/decline` | Decline a pending invitation. |
| `POST /clients/{client}/exchange/items` | Add an inventory item to the current exchange. |
| `POST /clients/{client}/exchange/gold` | Set the local exchange gold once. |
| `POST /clients/{client}/exchange/accept` | Accept the current exchange. |
| `POST /clients/{client}/exchange/cancel` | Cancel the current exchange. |
| `POST /clients/{client}/players/{player}/inspect` | Refresh one case-insensitive visible player profile. The cache-only `GET` route is listed under current data routes. |
| `POST /clients/{client}/commands/diagnostic` | Run a no-op main-thread command for testing. |
| `GET /clients/{client}/diagnostics/hooks` | Query the current hook timing mode and counters. |
| `PUT /clients/{client}/diagnostics` | Set `mode` to `disabled` or `hook_timing`, optionally with `reset: true`. |
| `GET /clients/{client}/commands/{command_id}` | Read retained command status. |
| `DELETE /clients/{client}/commands/{command_id}` | Cancel a command that has not started. |

Movement request bodies, route injection, cancellation, and stop reasons are
documented in [Movement](movement.md). Emote names and codes are documented in
[Emotes](emotes.md).
Look request bodies, response correlation, popup suppression, and result events
are documented in [Looking at tiles](looks.md).
Item, gold, pickup, chant, and NPC item-action bodies are documented in
[Inventory](inventory.md).
Outbound chat and internal inter-client message fields are documented in
[Messages](messages.md).
Equipment, skill, and spell arguments are documented in their respective
chapters. NPC interaction, revision checks, and dialog responses are documented
in [NPC dialogs](dialogs.md). Stat-point spending is documented with
[Character status](status.md#spending-stat-points). Group state, invitations, and roster confirmation
are documented in [Groups](groups.md). Player offers, constraints, and exchange
completion are documented in [Exchange](exchanges.md).
Raw packet syntax and crash risks are documented in [Raw packets](raw.md).

Runtime hook diagnostics require DLL component 1.5.2 or later. The mode is
disabled by default. A successful query returns the stage budget, call count,
total, average, maximum, over-budget count, and last duration in microseconds.
Reset clears counters but the request's `mode` remains authoritative for the
resulting runtime state.

### Resynchronizing a client

`POST /clients/{client}/resync` takes no request body. It schedules the same
opcode-only refresh as pressing F5 in the game client. Use it when the client
appears out of sync with the server, such as after movement is rejected or the
character appears stuck against a wall.

The response describes the one active refresh:

```text
{
    pid: u32,
    instance_id: string,
    resync_id: u32,
    coalesced: bool,
    resync: {
        phase: idle | waiting_to_send | awaiting_response,
        active_resync_id: u32?,
        pending_count: u32,
    },
}
```

`coalesced: true` means another F5 or HTTP request already owns the returned
`resync_id`; daRPC did not send a second packet. `pending_count` is always zero
in 1.7.0. The HTTP response does not mean the server redraw is finished. Follow
`client.resync` and `client.resync_completed` on the event stream.

See [Refresh and resynchronization](resync.md) for movement safety, the
one-second fallback, object reconciliation, error codes, and the complete
consumer sequence.

Basic attacks require no request body:

```sh
curl --request POST "http://127.0.0.1:2626/clients/ZiLo/assail"
```

The action submits client packet `0x13` on the game thread. Observe
`player.animated` and `sound.played` on the client's event stream for the
server-confirmed animation and sound cues.

### Native command results

Native actions are queued for the client main thread. A response contains a
`command_id`, command kind, current state, timing information, and an optional
failure reason.

Failed exact-route replacements can also contain `diagnostics`. It reports the
packet-confirmed and native committed positions, an active staged destination,
map IDs, transition state, route mode, and current destination. The field is
absent from other command results.

Command states are:

```text
accepted, executed, failed, cancelled, timed_out
```

`200 OK` means the command reached a final state during the request's bounded
wait. `202 Accepted` means it is still queued and can be checked later. A full
queue returns `429 Too Many Requests`; an unavailable client returns
`503 Service Unavailable`.

An executed state means the client accepted and ran the local native call. A
later game event is the better proof of the resulting state or server
submission. daRPC does not automatically retry actions.

Command IDs belong to one DLL `instance_id`. Do not apply a retained result to
a different DLL lifetime. A command can be cancelled or expire before it
starts. Once native execution has begun, it completes normally.

The DLL executes at most one queued command per client tick. Web handling,
named-pipe input/output, allocation, serialization, and logging stay off the
game thread. See [Runtime hooks](hooks.md#main-thread-affinity) for the reason.

## Server-Sent Events

```console
curl --no-buffer --header "Accept: text/event-stream" \
  "http://127.0.0.1:2626/clients/ZiLo/events"
```

SSE is a one-way live stream. Use it for changing vitals, inventory updates,
walking, spell activity, nearby objects, messages, and action observations.

The endpoint requires a connected client with current state. The first event
is a `stream.ready` boundary:

```text
id: 38
event: stream.ready
data: {"type":"stream_ready","data":{"pid":6076,"instance_id":"...","revision":42,"event_sequence":38}}
```

After `stream.ready`, read the REST resources needed by the consumer, then
apply later events in their delivered order. The daemon begins listening before
it reads the ready boundary, so a change cannot slip between those two steps.

### Listening from the command line

```console
curl --no-buffer --header "Accept: text/event-stream" \
  "http://127.0.0.1:2626/clients/ZiLo/events"
```

Swagger UI shows the SSE response schemas but is not a live stream viewer.
Browser applications can use `EventSource`; other clients need streaming
response support.

The [Live events](events.md) chapter contains the complete event catalog,
payload structures, ordering rules, collection batching, browser examples, and
recovery procedure. Read it before relying on a long-running stream.

## Managed client lifecycle

The daemon can launch a client or load and unload the DLL:

| Route | Purpose |
| --- | --- |
| `POST /clients/launch` | Launch the client and initialize the DLL. |
| `POST /clients/{client}/load` | Load the DLL into a discovered client. |
| `POST /clients/{client}/unload` | Shut down and unload the DLL. |

Load and unload have no request body. The daemon's `--loader-path` and
`--dll-path` settings choose the trusted tools.

The smallest launch request is:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"client_path":"C:\\Games\\Dark Ages\\Darkages.exe"}' \
  "http://127.0.0.1:2626/clients/launch"
```

Available launch options are:

```text
{
    client_path,
    allow_multiple: false,
    show_items_with_alt: false,
    skip_exchange_alerts: false,
    skip_intro: false,
    skip_notice: false,
    server: null,
}
```

`client_path` must be a fully qualified Windows drive or Universal Naming
Convention (UNC) path. Its parent directory becomes the client's working
directory. `server` accepts `host` or `host:port`, with port 2610 as the
default. `skip_exchange_alerts` replaces the one-button result shown after a
completed or cancelled player exchange with the same text in the floating
game-message bar without changing the exchange itself.
`show_items_with_alt` applies the launch-only ground-item patch and
reveals up to 255 items while either Alt key is held.
The API does not accept arbitrary game arguments or request-selected loader and
DLL paths.

Load reports whether it actually changed an unloaded client. Unload reports
whether it actually changed a loaded client. Launch returns the new process ID
after the loader initializes and resumes it. Daemon discovery is asynchronous,
so poll `/clients` until the process becomes connected or reports an error.

## Errors

Validation errors use the applicable HTTP 4xx response. Environmental and
client availability failures use a suitable 5xx response. Managed lifecycle
errors use this general shape:

```text
{
  "error": {
    "code": "...",
    "message": "...",
    "pid": 6076
  }
}
```

Unknown fields are rejected. Current request-size limits and exact response
models are included in OpenAPI.

## OpenAPI and Swagger UI

The OpenAPI document is generated from the Rust HTTP models. It can be imported
into Postman, Apidog, client generators, or another OpenAPI consumer.

- `/docs` hosts the interactive Swagger UI.
- `/openapi.json` serves the canonical OpenAPI 3.1 JSON for the running binary.
- Release bundles include the same document as `openapi.json` for offline use.
- `darpcd.exe --print-openapi` prints the document and exits without starting
  the server.

Swagger UI uses vendored assets and an Ayu-inspired dark theme, so it works
without an internet connection. A Swagger rendering problem cannot affect the
registry, JSON routes, or DLL connections.

The OpenAPI document is the interface contract; Swagger UI is only one viewer.
This separation allows the documentation frontend to change later without
changing API routes or generated clients.

OpenAPI describes the JSON event envelopes but cannot fully express the
surrounding SSE lines, stream ordering, lag, and reconnect behavior. This
chapter is the source of truth for those transport rules.

## Network access

The listener defaults to `127.0.0.1:2626`. `--listen <ipv4[:port]>` can bind a
specific IPv4 interface or `0.0.0.0` for access from a host, another virtual
machine, or a trusted local network. The option changes the entire API
listener, not only Swagger UI.

The API has no authentication, authorization, or TLS. Any host that can reach a
non-loopback listener can read game state and submit actions. Prefer a specific
interface over `0.0.0.0`, restrict the selected port with Windows Firewall, and
do not expose the listener to an untrusted network or the public internet. A
generally available remote mode still requires authentication, authorization,
request limits, and transport security.

WebSockets are intentionally unsupported. REST maps commands and state reads to
bounded requests with ordinary HTTP status and error handling. SSE provides a
persistent server-to-consumer event stream with straightforward reconnection.
This split is adequate for real-time bot interaction and avoids duplicating
validation, backpressure, ordering, and connection lifecycle behavior across a
second bidirectional API. The decision can be revisited if a measured use case
cannot be expressed cleanly with REST and SSE.
