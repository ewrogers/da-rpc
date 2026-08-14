# `darpcd.exe`

The daemon is the normal starting point for dashboards, scripts, and tools that
work with one or more game clients. This chapter covers running and configuring
it. Use [Web API](web-api.md) for HTTP routes and [Live events](events.md) for
the streaming interface.

> **Status:** Automatic client discovery, the identity registry, daemon-managed
> load, unload, and launch, current client state, routed movement commands,
> REST, and Server-Sent Events are implemented.

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
- Expose default-loopback REST and Server-Sent Events APIs, an OpenAPI
  document, and Swagger UI, with an explicit IPv4 bind for trusted networks.

Additional game actions can build on this boundary later. REST provides bounded
requests and responses for those actions, while Server-Sent Events provide the
live update stream. The daemon retains observations but is not the authority for
client memory or local state.

## Command-line reference

```text
darpcd.exe [--pid <pid> ...] [--port <port> | --listen <ipv4[:port]>]
           [--auto-load]
           [--loader-path <path>] [--dll-path <path>] [--maps-path <path>]
darpcd.exe --print-openapi
```

| Flag | Meaning |
|---|---|
| `--pid <pid>` | Retain a specific process as a controlled target. Repeat the flag for multiple clients. Normal window discovery remains active. |
| `--port <port>` | Listen on this TCP port instead of `2626`. The listener remains bound to `127.0.0.1`. |
| `--listen <ipv4[:port]>` | Bind to an explicit IPv4 interface and optional port. An omitted port defaults to `2626`. This flag cannot be combined with `--port`. |
| `--auto-load` | Use the configured loader and DLL once for each discovered, supported client that is not already loaded. |
| `--loader-path <path>` | Use this `loader.exe` for managed load, unload, launch, and automatic loading. The default is `loader.exe` beside the daemon. |
| `--dll-path <path>` | Use this `darpc.dll` for managed load, launch, and automatic loading. The default is `darpc.dll` beside the daemon. |
| `--maps-path <path>` | Override the automatically discovered local client `Maps` directory used by `GET /maps/{map_id}/download`. The path must name an existing directory. |
| `--print-openapi` | Print the OpenAPI 3.1 document as JSON and exit. This standalone flag cannot be combined with server flags. |

Paths containing spaces must be quoted. Repeating a single-value flag or using
an unknown flag is an error. The daemon reports startup failures on standard
error and exits nonzero.

Start normal discovery and serve the API on the default loopback address:

```text
darpcd.exe
```

Allow a host or another virtual machine on a trusted network to reach the API:

```text
darpcd.exe --listen 0.0.0.0:2626
```

`0.0.0.0` binds every IPv4 interface. Prefer the VM's specific IPv4 address
when practical, and restrict the port with Windows Firewall. The API has no
authentication or Transport Layer Security (TLS), so every host that can reach
the listener can read state and submit actions.

Automatically load uninjected supported clients and select explicit runtime
files:

```text
darpcd.exe --auto-load --loader-path "C:\daRPC\loader.exe" --dll-path "C:\daRPC\darpc.dll"
```

Override the local client's automatically discovered map directory:

```text
darpcd.exe --maps-path "C:\Dark Ages\Maps"
```

Without the flag, the daemon adopts the `Maps` directory beside the first
discovered `Darkages.exe`, including a client discovered after daemon startup.
Until a client or override supplies a directory, map downloads return `404`.
The daemon fails at startup if an explicit override is missing or is not a
directory.

Export the same OpenAPI document that the running daemon serves at
`/openapi.json`:

```text
darpcd.exe --print-openapi > openapi.json
```

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
sequence, suppress terminal exchange alerts, and optionally select a server
endpoint. The API never accepts arbitrary process arguments or request-selected
loader and DLL paths.

The current console output reports transitions such as:

```text
HTTP API listening on http://127.0.0.1:2626
client pid=3780 status=connecting
client pid=3780 status=not_loaded
client pid=3780 status=initializing
client pid=3780 status=connected creation_time=... instance=... protocol=1.3 ...
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

The HTTP server binds to `127.0.0.1:2626` by default. `--port <port>` overrides
the port while retaining the loopback boundary. `--listen <ipv4[:port]>`
explicitly selects another IPv4 interface for trusted VM or local-network use.
The generated OpenAPI document is served at `/openapi.json`, and the vendored
Swagger UI is served at `/docs`. HTTP models remain separate from registry and
binary protocol types.

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
