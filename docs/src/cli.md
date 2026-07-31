# `darpc.exe` command-line interface

> **Status:** The direct `ipc` commands documented below are implemented.

`darpc.exe` is a direct, single-client command-line interface to an injected
`darpc.dll`. It connects to the process-specific named pipe, exchanges typed
binary protocol messages, and presents responses as human-readable text or
stable JSON.

The CLI does not call the `darpcd.exe` HTTP API, inject DLLs, or invoke
`loader.exe`. This keeps a useful standalone path for developers and automation
that need only `loader.exe`, `darpc.dll`, and `darpc.exe`.

The command-line boundaries are:

| Tool | Responsibility |
| --- | --- |
| `loader.exe` | Launch, inspect, attach, detach, and apply supported launch patches. |
| `darpc.exe` | Exchange typed protocol messages directly with one injected DLL. |
| `darpcd.exe` | Maintain multiple client connections and expose aggregate state through web APIs. |

## Direct IPC diagnostics

The implemented diagnostics prove communication without hooks or game state:

```text
darpc ipc hello --pid <pid>
darpc ipc ping --pid <pid>
darpc ipc echo --pid <pid> "hello"
```

These commands use the real PID-based named pipe, binary framing, protocol
negotiation, request correlation, sequencing, and connection lifecycle. Only
their payloads are synthetic:

- `hello` reports compatible DLL and process metadata.
- `ping` verifies a complete request and response round trip and reports its
  elapsed time.
- `echo` returns its UTF-8 payload byte-for-byte, with a 4 KiB input limit.

The `ipc` group shares `darpc-protocol` with the DLL and daemon. It requires an
explicit nonzero process ID and cannot manage multiple clients in one command.

## Output

Human-readable output is the default. Put `--output json` before `ipc` to emit
one stable JSON value on standard output:

```text
darpc --output json ipc hello --pid <pid>
darpc --output json ipc ping --pid <pid>
darpc --output json ipc echo --pid <pid> "hello"
```

Diagnostics belong on standard error so scripts can parse JSON from standard
output without filtering it. Exit codes distinguish invalid input, missing or
busy endpoints, protocol incompatibility, malformed responses, and other I/O
failures.

## Connection ownership

The DLL pipe currently accepts one controller at a time. `darpc.exe` and
`darpcd.exe` are alternative consumers of that pipe, not layers in the same
request path. A direct CLI command reports the endpoint as busy when the daemon
owns the connection. It does not fall back to the daemon or disconnect it.

## Future commands

New CLI commands should be added only when the DLL exposes the matching typed
protocol operation. Each command should:

- Target exactly one explicit PID.
- Validate arguments before opening the pipe.
- Use typed protocol messages rather than arbitrary byte or command strings.
- Preserve equivalent human-readable and stable JSON representations.
- Remain usable without `darpcd.exe`.

Game-state reads and actions can extend the existing `ipc` hierarchy as their
protocol messages become real. The CLI should not grow daemon discovery,
aggregation, web configuration, or multi-client policy.

## Daemon access

Consumers that need aggregated multi-client state use the `darpcd.exe` HTTP
API directly. The daemon publishes an OpenAPI document at `/openapi.json` and
an interactive Swagger UI at `/docs`, so another command-line HTTP wrapper is
not part of the planned architecture.
