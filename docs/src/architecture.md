# Architecture

This chapter is a light tour of the system. It is for readers who want to know
how daRPC works without reading protocol fields, memory layouts, or x86 code.
Application authors can use the [Web API](web-api.md) and
[Live events](events.md) without depending on these internal boundaries.

daRPC is split into four programs so the game-specific work stays close to the
client while tools can use ordinary command-line and web interfaces.

| Component | What it does |
| --- | --- |
| `darpc.dll` | Lives inside one game client, tracks its state, observes events, and runs native actions. |
| `loader.exe` | Launches a supported client or attaches and detaches the DLL. |
| `darpc.exe` | Talks directly to one DLL and prints human-readable text or JSON. |
| `darpcd.exe` | Discovers several clients and exposes REST, SSE, OpenAPI, and Swagger UI. |

```text
                         direct CLI
                    +---------------------- darpc.exe
                    |
Darkages.exe <-> darpc.dll <-> named pipe <-> darpcd.exe <-> REST / SSE
     ^                                                       OpenAPI / Swagger
     |
 loader.exe
```

The DLL and loader are 32-bit because the Dark Ages client is 32-bit. The
direct CLI and daemon are ordinary 64-bit Windows programs.

## One DLL per client

Each injected DLL is responsible for its own game client. It understands the
supported client layout, captures current state, and keeps that state updated
as the game handles new events.

The DLL remains useful without the daemon. `darpc.exe` can connect directly for
one-client scripts or diagnostics. The DLL also continues tracking local state
when the daemon disconnects and accepts a replacement connection later.

Only one controller owns a DLL's named pipe at a time. If the daemon is
connected, a direct CLI request reports that the endpoint is busy rather than
silently sending the request through the daemon.

## The daemon is the meeting point

`darpcd.exe` discovers game clients and keeps a current public view of each
one. It never reads game memory itself. It uses the typed state and events sent
by each DLL, then presents player-friendly API models.

This boundary keeps Windows injection and client details out of dashboards,
scripts, and other consumers. It also keeps a slow web request or event
subscriber from blocking unrelated game clients.

REST reads current state and submits individual actions. Server-Sent Events
(SSE) carry one-way live changes. Together they provide real-time interaction
while keeping commands bounded and the event stream independently reconnectable.
WebSockets are intentionally unsupported because another bidirectional transport
would duplicate validation, flow control, ordering, and connection lifecycle
behavior without a demonstrated requirement.

## Reading game state

A new daemon connection begins with a complete client baseline. The DLL then
sends smaller ordered updates as relevant game events occur.

```text
client main thread -> bounded copy -> DLL state -> named pipe -> daemon -> REST / SSE
```

REST resources are views of the daemon's retained state. Reading inventory or
status does not trigger a new memory walk. See [Game data](state.md) for the
baseline, revisions, missing values, and reconnect behavior.

## Running native actions

Actions travel in the opposite direction:

```text
REST or direct CLI -> named pipe -> bounded queue -> client tick -> native method
```

The pipe worker validates and queues pointer-free command data. A normal game
tick executes at most one queued command on the client main thread. This is
where the client expects its movement, skill, and spell methods to run.

Using native methods keeps client timing, interface state, and local validation
in the normal path. Native pathfinding owns its route, so player input can
cancel or replace it naturally.

## Hooks and main-thread affinity

Small runtime hooks provide safe moments to copy changing state and drain
native commands. They do not perform web requests, named-pipe input/output,
logging, or large conversions.

The [Runtime hooks](hooks.md) chapter explains every installed hook, why it
exists, how work is moved off the main thread, and how daRPC removes the hooks
before unloading.

## Client views are not a global world

Several clients can observe the same map, but each has its own view distance
and last-seen time. daRPC keeps those observations separate. It does not claim
that one client's visible-object list is the full map population.

A future shared-world projection can merge compatible observations, but it
must preserve their source and freshness.

## Design goals

- Keep injected work small and bounded.
- Preserve the client's original behavior unless a feature explicitly changes it.
- Validate the exact supported client before reading state or installing hooks.
- Keep raw pointers and client layouts inside the injected process.
- Let the DLL, daemon, and consumers disconnect without closing the game.
- Give tool authors portable interfaces and game-friendly data names.
