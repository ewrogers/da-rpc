# Architecture

daRPC has four primary runtime components:

| Component | Target | Responsibility |
| --- | --- | --- |
| `darpc.dll` | 32-bit Windows x86 | Integrates with one game client, reconstructs initial state, tracks game and UI changes, and hosts a named-pipe endpoint. |
| `loader.exe` | 32-bit Windows x86 | Launches a client with daRPC or injects `darpc.dll` into an already-running compatible client. |
| `darpc.exe` | 64-bit Windows x86-64 | Provides direct IPC diagnostics and a user-facing interface to the daemon API. |
| `darpcd.exe` | 64-bit Windows x86-64 | Discovers clients, queries and aggregates their state and events, and exposes portable web APIs. |

```text
Remote or local application ---- REST / SSE / WebSocket ----+
                                                            |
darpc.exe ---------------------- HTTP -----------------------+--> darpcd.exe
    |                                                               |
    | Explicit diagnostic IPC                                      | Binary IPC
    v                                                               v
                         darpc.dll <-------------------------- loader.exe
                             |
                             | Client events, actions, and state
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

`darpc.exe` normally uses the daemon HTTP API. Its explicit `ipc` command group
may connect directly to one DLL for bounded development diagnostics while the
daemon is disconnected.

`loader.exe` owns process launch and injection mechanics. Discovery may present
a process as an injection candidate, but the loader must still validate that
the target is compatible before modifying it.

## Design principles

- Keep injected code small, predictable, and resilient to failures.
- Isolate unsafe Rust and document every memory and application binary
  interface invariant.
- Use the smallest practical set of reviewed dependencies.
- Keep protocol and platform boundaries explicit and versioned.
- Do not expose raw client pointers outside the injected process.
- Keep daemon and consumer failures from terminating the game session.
- Let consumers use portable APIs without needing client internals.
