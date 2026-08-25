# Executable components

The Windows release contains three executable programs. Choose the narrowest
one that fits the task:

| Program | Architecture | Purpose |
|---|---:|---|
| [`loader.exe`](loader.md) | 32-bit x86 | Inspect, attach to, detach from, or launch a supported game client. |
| [`darpc.exe`](cli.md) | 64-bit x86-64 | Run one command against one attached client without starting the web service. |
| [`darpcd.exe`](rpcd.md) | 64-bit x86-64 | Discover and aggregate clients, then expose the REST API, Server-Sent Events, OpenAPI document, and Swagger UI. |

`darpc.exe` and `darpcd.exe` are alternative controllers for the same client.
Only one controller can own a client's named-pipe connection at a time. Use
`darpc.exe` for scripts and terminal workflows. Use `darpcd.exe` for long-lived
API clients, multiple game clients, event subscriptions, and browser-based API
exploration.

The fourth runtime component, `darpc.dll`, is injected into the 32-bit client
and is not run from a command prompt. See the [`darpc.dll` architecture
chapter](rpc-dll.md) for its responsibilities and safety boundaries.

The following chapters provide the complete command-line syntax, flags,
examples, output formats, and operational notes for every executable.
