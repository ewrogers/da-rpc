# Architecture

daRPC has four primary runtime components:

| Component | Target | Responsibility |
| --- | --- | --- |
| `darpc.dll` | 32-bit Windows x86 | Integrates with one game client, reconstructs initial state, tracks game and UI changes, and hosts a named-pipe endpoint. |
| `loader.exe` | 32-bit Windows x86 | Launches a client with daRPC or injects `darpc.dll` into an already-running compatible client. |
| `darpc.exe` | 64-bit Windows x86-64 | Talks directly to one injected DLL and presents typed binary protocol results as text or JSON. |
| `darpcd.exe` | 64-bit Windows x86-64 | Discovers clients, queries and aggregates their state and events, and exposes portable web APIs. |

```text
Remote or local application ---- REST / SSE / WebSocket ----> darpcd.exe
                                                                  |
                                                                  | Binary IPC
darpc.exe ---------------------- Binary IPC ------------------+    |
                                                             v    v
                                                           darpc.dll
                                                               ^
                                                               | Load and initialize
                                                          loader.exe
                                                               |
                                                               v
                                                     Dark Ages game client
```

## Responsibility boundaries

`darpc.dll` is the local state authority for the process into which it is
injected. It understands client memory, hooks, events, and version-specific
layouts. It continues tracking state whether or not `darpcd.exe` is connected.

`darpcd.exe` does not read client memory or independently reconstruct game state.
It queries and aggregates the state supplied by each `darpc.dll`, listens for
real-time updates, and presents stable models to API consumers.
Character and user interface state remain client-scoped. A future shared-world
projection may merge compatible map and entity observations, but it must retain
their source and freshness instead of presenting partial client visibility as
authoritative global state.

`darpc.exe` connects directly to one DLL and remains usable without the daemon.
It presents typed binary protocol operations as human-readable text or stable
JSON. Because the DLL currently accepts one controller, the direct CLI reports
a busy endpoint while `darpcd.exe` owns that pipe. It does not call or fall back
to the daemon HTTP API.

`loader.exe` owns process launch and injection mechanics. Discovery may present
a process as an injection candidate, but the loader must still validate that
the target is compatible before modifying it.

The `darpc-hook` support crate owns the reusable, in-process x86 detour
boundary. It has no game addresses or state knowledge. `darpc.dll` supplies
validated client-specific targets and detours for each live hook.
The separate `hook-harness.exe` exercises that boundary against owned code.
See [Hook safety](hooks.md) for its transaction and shutdown contracts.

## Web boundary

The daemon web boundary uses Axum. It binds to `127.0.0.1:2626` by default and
exposes health, client discovery, focused state resources, per-client
Server-Sent Events, lifecycle operations, `/openapi.json`, and `/docs`. A
`--port <port>` option changes only the port; remote interfaces remain
unavailable. HTTP response models remain separate from registry records,
binary protocol messages, and client layouts.

`utoipa` generates the OpenAPI document from the same Rust models and route
descriptions used by the server. A vendored Swagger UI presents that document
at `/docs` without a content delivery network or other runtime internet
dependency. Consumers may instead import `/openapi.json` into their preferred
OpenAPI tooling.

The synchronous connection workers send events to the daemon's registry loop.
After each changed event, that loop publishes a new immutable registry snapshot
for the HTTP thread. A handler clones the published snapshot reference before
building its response, so it never holds the live registry or a lock across
network I/O. Ordered state events also enter a bounded broadcast channel for
matching SSE subscribers. HTTP failures and slow subscribers therefore cannot
stop client health checks or mutate registry state.

The HTTP routes do not carry a version prefix. daRPC maintains one current API
while it is evolving. The OpenAPI `info.version` identifies the documented
daRPC release, not a parallel compatibility surface. Route or media-type
versioning should be introduced only if supported consumers create a concrete
need for simultaneous incompatible APIs.

## Design principles

- Keep injected code small, predictable, and resilient to failures.
- Isolate unsafe Rust and document every memory and application binary
  interface invariant.
- Use the smallest practical set of reviewed dependencies.
- Keep platform boundaries explicit, and keep the binary protocol versioned.
- Do not expose raw client pointers outside the injected process.
- Keep daemon and consumer failures from terminating the game session.
- Let consumers use portable APIs without needing client internals.
