# Discovery and recovery

Discovery is owned by `rpcd.exe`. The daemon periodically reconciles candidate
game clients with available daRPC endpoints. `rpc.dll` does not need to locate
or notify the daemon.

## Deterministic pipe names

Once initialized, each `rpc.dll` creates a named pipe derived from its process
identifier (PID):

```text
\\.\pipe\da-rpc-{pid}
```

The DLL keeps this endpoint available and accepts a replacement connection
after `rpcd.exe` disconnects or restarts. Pipe access should be local-only and
restricted to the intended Windows user.

## Reconciliation loop

`rpcd.exe` reconciles once at startup and then at a short fixed interval. An
initial interval around two seconds is simple and responsive enough to start;
it can become configurable only if a real need appears.

1. Enumerate top-level windows and select those with the configured supported
   game window class. The exact class name must be verified before it is fixed
   in code.
2. Resolve every matching window to its process identifier.
3. Derive the expected named-pipe path for each PID.
4. Attempt a short, bounded connection and perform the daRPC handshake.
5. Remove connections whose process or pipe has disappeared.
6. Retry living candidates during the next reconciliation.

A window-class match is only a candidate filter. It is not proof of a safe
client version or a valid daRPC endpoint.

## Candidate states

The pipe result determines the next step:

| Result | Meaning | Action |
| --- | --- | --- |
| Connection and handshake succeed | A compatible `rpc.dll` is available. | Request a snapshot and begin listening for updates. |
| Pipe is busy | An endpoint exists but cannot accept this connection yet. | Retry without injecting. |
| Pipe is missing during a short grace period | The DLL may still be initializing. | Wait for the next reconciliation. |
| Pipe remains missing | The process may not be injected. | Present it as a possible `loader.exe` candidate. |
| Handshake fails | An endpoint exists but is incompatible or invalid. | Report the error and do not inject automatically. |

`loader.exe` must repeat its own compatibility and already-loaded checks before
injection, even when `rpcd.exe` reports a candidate.

## Daemon recovery

While the daemon is unavailable, `rpc.dll` continues updating local state. The
pipe server detects the broken connection and returns to its listening state.
After restart, `rpcd.exe` performs its normal startup reconciliation, connects,
requests a new snapshot, and resumes event delivery from the snapshot boundary.

This restores current state without requiring a registry entry, shared file,
system service, or event backlog.

## Why not custom window messages

Custom Windows messages are not a primary discovery mechanism because a
notification can be missed and an uninjected client cannot respond. They also
create a reverse dependency in which injected code must know how to locate the
daemon.

A message may be added later as a latency optimization, but reconciliation must
remain the source of truth. If polling is already fast and inexpensive, the
notification adds little value.
