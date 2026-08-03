# `darpcd.exe`

> **Status:** Automatic client discovery, the identity registry, daemon-managed
> load, unload, and launch, current client state, routed movement commands,
> REST, and Server-Sent Events are implemented. WebSocket APIs remain planned.

`darpcd.exe` is a 64-bit x86-64 Windows daemon that makes injected clients easy
to use from local applications.

Its current responsibilities are to:

- Discover supported game client windows and their deterministic daRPC pipes.
- Track uninjected processes as loader candidates.
- Connect and reconnect to available `darpc.dll` instances.
- Invoke the configured `loader.exe` for explicit lifecycle operations.
- Optionally load the configured DLL once into each uninjected client.
- Aggregate client identity, connection health, snapshots, and ordered state
  updates from each connected client.
- Route bounded commands through each client's existing pipe session.
- Expose loopback REST and Server-Sent Events APIs, an OpenAPI document, and
  Swagger UI.

Additional game actions and bidirectional WebSocket traffic can build on this
boundary later. The daemon retains observations but is not the authority for
client memory or local state.

## Discovery and registry

Start the daemon without a PID to discover clients from their verified
`Darkages` top-level window class:

```text
darpcd.exe
darpcd.exe --port 3626
darpcd.exe --auto-load
```

Repeat `--pid <pid>` to retain additional controlled targets or processes that
do not expose the normal game window. Explicit and discovered targets use the
same independent connection workers:

```text
darpcd.exe --pid 3780 --pid 6648
```

Each worker retries a missing or busy pipe, performs the shared controller
handshake, requests a fresh snapshot, long-polls bounded state-event batches,
and sends a periodic `Ping` to detect a broken connection.
An accepted release connection must report the supported x86 architecture,
executable fingerprint, and client version. Registry identity combines the PID, raw
process creation time, and DLL instance ID. A reused PID or reloaded DLL
therefore replaces the prior record instead of inheriting it.

Each worker also owns a bounded command receiver. HTTP requests carry the
expected process and DLL identity, and the worker rejects a request after a
replacement or disconnect. It processes at most one routed command between
normal event polls, assigns protocol request IDs on the owning session, and
never opens a competing pipe connection. A full worker queue affects only that
client.

An incompatible peer remains visible as a target status but is not accepted as
a client and is never reinjected automatically. A discovered target is removed
after its game window disappears. An explicit PID remains configured until the
daemon exits.

`--auto-load` applies the configured loader and DLL to every `not_loaded`
target once per tracked process. This includes targets present at daemon startup
and targets discovered later. Connecting, connected, busy, initializing, and
incompatible targets are not injected. Each target is handled independently,
so one validation or loader failure does not stop discovery or other clients.

Automatic loading records the attempt before starting the loader. It therefore
does not retry on every discovery pass, and an explicit unload remains unloaded
for the rest of that tracked process lifetime. Removing and rediscovering the
process, or restarting the daemon with `--auto-load`, makes it eligible again.

## Managed lifecycle

The loader and DLL paths default to `loader.exe` and `darpc.dll` beside
`darpcd.exe`. Override those server-side paths when the artifacts live
elsewhere:

```text
darpcd.exe --loader-path <loader.exe> --dll-path <darpc.dll>
```

These paths are also used by `--auto-load`; the flag never accepts a different
DLL or bypasses normal loader validation.

Each launch request supplies the full executable path for the intended
installation. The daemon assumes no client base directory; `loader.exe` uses
the executable's parent directory as the launched process working directory.

The HTTP API can load the configured DLL into a discovered PID, unload it, or
launch the requested executable suspended and initialize the DLL before the
client resumes. `loader.exe` repeats architecture, DLL, and executable
validation for every operation. A window match or request path is only a
candidate signal.

Launch requests expose the client executable path and only the supported
startup choices: allow multiple clients, skip the intro, skip the notice
sequence, and optionally select a server endpoint. The API never accepts
arbitrary process arguments or request-selected loader and DLL paths.

The current console output reports transitions such as:

```text
HTTP API listening on http://127.0.0.1:2626
client pid=3780 status=connecting
client pid=3780 status=not_loaded
client pid=3780 status=initializing
client pid=3780 status=connected creation_time=... instance=... protocol=1.0 ...
client pid=3780 status=disconnected instance=... reason="..."
client pid=3780 status=busy
client pid=3780 status=incompatible instance=... reason="..."
client pid=3780 status=removed
```

After the handshake, each worker requests a fresh snapshot and stores it with
that client's identity and connection metadata. Reconnecting after a daemon
restart therefore reconstructs daemon state without reinjecting the DLL. The
snapshot carries the event boundary it already represents. Consecutive absolute
updates reduce into the retained state and appear in REST without another
memory walk. A reported overflow or sequence or revision gap causes an
immediate fresh snapshot.

Active spell effects are retained as a focused collection resource. Ordered
add, remove, and relative-duration changes update that resource and the
per-client event stream from the same event boundary.

## Web interface

The HTTP server binds to `127.0.0.1:2626` by default. A single
`--port <port>` option overrides the port while retaining the loopback-only
boundary. The generated OpenAPI document is served at `/openapi.json`, and the
vendored Swagger UI is served at `/docs`. HTTP models remain separate from
registry and binary protocol types.

Each connected client also exposes
`GET /clients/{client}/events`. The daemon subscribes before reading the
current registry snapshot, emits a `stream.ready` boundary, and then emits only
later state changes for that exact process and DLL identity. The internal
broadcast channel holds 4,096 events. A lagging subscriber receives
`stream.resync_required` and closes; it cannot block the game hook, client
worker, or another subscriber.

The same web boundary can submit, query, and cancel the no-op diagnostic
command. A separate bounded daemon router wakes the registry loop, which sends
the request to the matching per-client worker. The returned status includes
the DLL instance ID, client tick timing, execution duration, and game
main-thread ID.

See the [Web API](web-api.md) chapter for routes, request models, responses, and
failure behavior.

## Failure isolation

A daemon restart must not end a game session. The pipe closes when the daemon
stops, `darpc.dll` immediately returns to listening, and a replacement daemon
can reconnect without reinjection. One worker failure changes only that
target's status and cannot terminate another worker or the daemon.

Lifecycle work runs outside the asynchronous HTTP executor. Connections,
requests, DLL event storage, and daemon stream fanout are bounded so one slow
API consumer or game client cannot starve the others.
