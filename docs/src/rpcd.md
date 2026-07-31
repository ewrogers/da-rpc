# `darpcd.exe`

> **Status:** The explicit-PID connection manager and identity registry are
> implemented. Automatic discovery, snapshots, actions, and web APIs remain
> planned.

`darpcd.exe` is a 64-bit x86-64 Windows daemon that makes injected clients easy
to use from local and remote applications.

Its responsibilities are to:

- Discover game client processes and their deterministic daRPC pipe endpoints.
- Identify uninjected processes that may be candidates for `loader.exe`.
- Connect and reconnect to available `darpc.dll` instances.
- Query and aggregate the state maintained by each injected client.
- Listen for real-time state changes and client events.
- Route valid actions from API consumers to the intended client.
- Expose REST, Server-Sent Events, and WebSocket interfaces.

The daemon is not the authority for client memory or local state.

## Explicit client registry

Until automatic discovery exists, start the daemon with one or more explicit
process identifiers (PIDs):

```text
darpcd.exe --pid 3780
darpcd.exe --pid 3780 --pid 6648
```

Each `--pid` option accepts one nonzero 32-bit PID. At least one is required,
and duplicate PIDs are rejected.

The daemon gives every PID an independent worker. A worker retries a missing or
busy pipe, performs the shared controller handshake, and sends a bounded
periodic `Ping` to detect a broken connection. An accepted release connection
must also report the supported x86 architecture, executable fingerprint, and
layout ID. Registry identity combines the PID, raw process creation time, and
DLL instance ID. A reused PID or reloaded DLL therefore replaces the prior
record instead of inheriting it.

The current console output reports transitions such as:

```text
client pid=3780 status=connecting
client pid=3780 status=connected creation_time=... instance=... protocol=1.0 ...
client pid=3780 status=disconnected instance=... reason="..."
client pid=3780 status=busy
client pid=3780 status=incompatible instance=... reason="..."
```

An incompatible peer remains visible as a target status but is not inserted as
an accepted client. The current registry contains identity, compatibility, and
connection health only. Once state messages exist, every new daemon connection
will obtain a fresh snapshot and then follow updates from an ordered boundary.

## Failure isolation

A daemon restart must not end a game session. The pipe closes when the daemon
stops, `darpc.dll` immediately returns to listening, and a replacement daemon
can reconnect without reinjection. One worker failure changes only that target's
status and cannot terminate another worker or the daemon.

Connections and queues must be bounded so one slow API consumer or game client
cannot starve the others.
