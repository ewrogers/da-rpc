# `darpc.exe` command-line interface

> **Status:** The direct commands documented below are implemented.

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

## Direct IPC commands

The implemented operations prove communication, expose hook health, read a
current client snapshot, and submit movement through the client:

```text
darpc hello --pid <pid>
darpc ping --pid <pid>
darpc echo --pid <pid> "hello"
darpc tick-health --pid <pid>
darpc snapshot --pid <pid>
darpc diagnostic --pid <pid>
darpc turn --pid <pid> <north|east|south|west>
darpc walk --pid <pid> <north|east|south|west>
darpc walk --pid <pid> <x> <y>
darpc skill-use --pid <pid> <slot>
darpc spell-cast --pid <pid> <slot>
darpc spell-cast --pid <pid> <slot> --target-id <object-id>
darpc spell-cast --pid <pid> <slot> --target <x> <y>
darpc spell-cast --pid <pid> <slot> --input <text>
darpc command-status --pid <pid> <command-id>
darpc command-cancel --pid <pid> <command-id>
```

These commands use the real PID-based named pipe, binary framing, protocol
negotiation, request correlation, sequencing, and connection lifecycle. Their
behavior is:

- `hello` reports compatible DLL and process metadata.
- `ping` verifies a complete request and response round trip and reports its
  elapsed time.
- `echo` returns its UTF-8 payload byte-for-byte, with a 4 KiB input limit.
- `tick-health` samples the client tick counter twice, 250 milliseconds apart,
  and reports installation metadata, both counter values, their wrapping
  difference, and whether the counter advanced.
- `snapshot` schedules a bounded capture on the client main thread and reports
  lifecycle, character, map, inventory, equipment, spellbook, skillbook, and
  active spell-effect state plus capture timing and request round-trip time.
- `diagnostic` submits a no-op command to the bounded main-thread queue, waits
  up to one second, and reports its state, queue delay, execution duration, and
  client main-thread ID.
- `turn` cancels any queued native route and asks the client to face one of the
  four cardinal directions.
- `walk` with a direction cancels any queued route and attempts one native,
  collision-checked step. `walk` with x/y asks the client's native pathfinder to
  follow a route to that zero-based map tile.
- `skill-use` invokes a learned one-based skill slot through the client's native
  activation routine. It does not select the skill panel, change focus, or
  synthesize keyboard or mouse input.
- `spell-cast` invokes a learned one-based spell slot through the matching
  native client routine. Its optional argument is one visible object ID, one
  zero-based map tile, or 1 through 100 ASCII bytes. The DLL checks that the
  selected spell expects that argument shape. A targeted spell defaults to the
  casting character when no target is supplied. A new cast may replace a
  delayed cast already in progress.
- `command-status` reads a retained command result by its nonzero ID.
- `command-cancel` atomically cancels a command that is still accepted. A
  command that already started retains its completed state.

The commands share `darpc-protocol` with the DLL and daemon. Each requires an
explicit nonzero process ID and cannot manage multiple clients in one command.

## Output

Human-readable output is the default. Put `--output json` before the command to
emit one stable JSON value on standard output:

```text
darpc --output json hello --pid <pid>
darpc --output json ping --pid <pid>
darpc --output json echo --pid <pid> "hello"
darpc --output json tick-health --pid <pid>
darpc --output json snapshot --pid <pid>
darpc --output json diagnostic --pid <pid>
darpc --output json turn --pid <pid> north
darpc --output json walk --pid <pid> 120 85
darpc --output json skill-use --pid <pid> 5
darpc --output json spell-cast --pid <pid> 7 --input "nothing"
darpc --output json command-status --pid <pid> <command-id>
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

Additional game-state reads and actions can extend the existing `ipc` hierarchy
as their protocol messages become real. The CLI should not grow daemon discovery,
aggregation, web configuration, or multi-client policy.

## Daemon access

Consumers that need aggregated multi-client state use the `darpcd.exe` HTTP
API directly. The daemon publishes an OpenAPI document at `/openapi.json` and
an interactive Swagger UI at `/docs`, so another command-line HTTP wrapper is
not part of the planned architecture.
