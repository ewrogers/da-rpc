# `darpcd.exe`

> **Status:** Automatic client discovery, the identity registry, daemon-managed
> load, unload, and launch, and the HTTP API are implemented. Game-state
> snapshots, actions, and streaming APIs remain planned.

`darpcd.exe` is a 64-bit x86-64 Windows daemon that makes injected clients easy
to use from local applications.

Its current responsibilities are to:

- Discover supported game client windows and their deterministic daRPC pipes.
- Track uninjected processes as loader candidates.
- Connect and reconnect to available `darpc.dll` instances.
- Invoke the configured `loader.exe` for explicit lifecycle operations.
- Aggregate client identity and connection health.
- Expose a loopback REST API, OpenAPI document, and Swagger UI.

Game-state aggregation, real-time events, and routed game actions build on this
boundary later. The daemon is not the authority for client memory or local
state.

## Discovery and registry

Start the daemon without a PID to discover clients from their verified
`Darkages` top-level window class:

```text
darpcd.exe
darpcd.exe --port 3626
```

Repeat `--pid <pid>` to retain additional controlled targets or processes that
do not expose the normal game window. Explicit and discovered targets use the
same independent connection workers:

```text
darpcd.exe --pid 3780 --pid 6648
```

Each worker retries a missing or busy pipe, performs the shared controller
handshake, and sends a bounded periodic `Ping` to detect a broken connection.
An accepted release connection must report the supported x86 architecture,
executable fingerprint, and layout ID. Registry identity combines the PID, raw
process creation time, and DLL instance ID. A reused PID or reloaded DLL
therefore replaces the prior record instead of inheriting it.

An incompatible peer remains visible as a target status but is not accepted as
a client and is never reinjected automatically. A discovered target is removed
after its game window disappears. An explicit PID remains configured until the
daemon exits.

## Managed lifecycle

The loader and DLL paths default to `loader.exe` and `darpc.dll` beside
`darpcd.exe`. Override those server-side paths when the artifacts live
elsewhere:

```text
darpcd.exe --loader-path <loader.exe> --dll-path <darpc.dll>
```

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

The registry contains identity, compatibility, and connection health only.
Once state messages exist, every new daemon connection will obtain a fresh
snapshot and then follow updates from an ordered boundary.

## Web interface

The HTTP server binds to `127.0.0.1:2626` by default. A single
`--port <port>` option overrides the port while retaining the loopback-only
boundary. The generated OpenAPI document is served at `/openapi.json`, and the
vendored Swagger UI is served at `/docs`. HTTP models remain separate from
registry and binary protocol types.

See the [Web API](web-api.md) chapter for routes, request models, responses, and
failure behavior.

## Failure isolation

A daemon restart must not end a game session. The pipe closes when the daemon
stops, `darpc.dll` immediately returns to listening, and a replacement daemon
can reconnect without reinjection. One worker failure changes only that
target's status and cannot terminate another worker or the daemon.

Lifecycle work runs outside the asynchronous HTTP executor. Connections and
requests are bounded so one slow API consumer or game client cannot starve the
others.
