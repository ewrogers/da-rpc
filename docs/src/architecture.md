# Architecture

daRPC has three primary runtime components:

| Component | Target | Responsibility |
| --- | --- | --- |
| `rpc.dll` | 32-bit Windows x86 | Integrates with one game client, reconstructs initial state, tracks game and UI changes, and hosts a named-pipe endpoint. |
| `loader.exe` | 32-bit Windows x86 | Launches a client with daRPC or injects `rpc.dll` into an already-running compatible client. |
| `rpcd.exe` | 64-bit Windows x86-64 | Discovers clients, queries and aggregates their state and events, and exposes portable web APIs. |

```text
Remote or local application
          |
          | REST / SSE / WebSocket
          v
      64-bit rpcd.exe
          |
          | Binary daRPC protocol over named pipes
          v
 32-bit injected rpc.dll <---- 32-bit loader.exe
          |
          | Client events, actions, and state
          v
   Dark Ages game client
```

## Responsibility boundaries

`rpc.dll` is the local state authority for the process into which it is
injected. It understands client memory, hooks, events, and version-specific
layouts. It continues tracking state whether or not `rpcd.exe` is connected.

`rpcd.exe` does not read client memory or independently reconstruct game state.
It queries and aggregates the state supplied by each `rpc.dll`, listens for
real-time updates, and presents stable models to API consumers.

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
