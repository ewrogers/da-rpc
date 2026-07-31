# Discovery and recovery

Discovery is owned by `darpcd.exe`. The daemon periodically reconciles candidate
game clients with available daRPC endpoints. `darpc.dll` does not need to locate
or notify the daemon.

The daemon also accepts repeated explicit `--pid <pid>` targets for controlled
processes or clients without the normal game window. Explicit and discovered
targets use the same registry and connection workers.

## Deterministic pipe names

Once initialized, each `darpc.dll` creates a named pipe derived from its process
identifier (PID):

```text
\\.\pipe\da-rpc-{pid}
```

The DLL keeps this endpoint available and accepts a replacement connection
after a controller disconnects or restarts. The implemented endpoint rejects
remote clients and grants access to the process owner, Windows system, and
administrators.

The pipe exposes one instance because the DLL has one controller. During
development, `darpc.exe ipc` can own it for direct diagnostics. In normal use,
`darpcd.exe` owns it; a second connector receives a distinct busy error and must
not inject another DLL.

## Reconciliation loop

`darpcd.exe` reconciles once at startup and then once per second.

1. Enumerate top-level windows and select the verified `Darkages` game window
   class using an exact, case-sensitive match.
2. Resolve every matching window to its process identifier.
3. Derive the expected named-pipe path for each PID.
4. Attempt a short, bounded connection and perform the daRPC handshake.
5. Remove discovered targets whose game window has disappeared.
6. Retry living candidates during the next reconciliation.

A window-class match is only a candidate filter. It is not proof of a safe
client version or a valid daRPC endpoint. Explicit load and launch operations
therefore go through `loader.exe`, which repeats executable, architecture, DLL,
and already-loaded validation before changing the process.

The daemon retains a newly launched PID for five seconds while the resumed
client creates its game window. This grace period prevents a successful launch
from disappearing between the loader result and the next window enumeration.

## Candidate states

The pipe result determines the next step:

| Result | Meaning | Action |
| --- | --- | --- |
| Connection and handshake succeed | A compatible `darpc.dll` is available. | Request a snapshot and begin listening for updates. |
| Pipe is busy | An endpoint exists but cannot accept this connection yet. | Retry without injecting. |
| Pipe is missing during a short grace period | The DLL may still be initializing. | Wait for the next reconciliation. |
| Pipe remains missing | The process may not be injected. | Report `not_loaded` and allow an explicit API load. |
| Handshake fails | An endpoint exists but is incompatible or invalid. | Report the error and do not inject automatically. |

`loader.exe` must repeat its own compatibility and already-loaded checks before
injection, even when `darpcd.exe` reports a candidate.

The HTTP API exposes explicit load and unload operations for tracked PIDs and a
launch operation for the daemon-configured executable. Discovery itself never
injects, unloads, or launches anything. Handshake failures therefore cannot
cause an automatic reinjection loop.

## Daemon recovery

While the daemon is unavailable, `darpc.dll` continues updating local state. The
pipe server detects the broken connection and returns to its listening state.
After restart, `darpcd.exe` performs its normal startup reconciliation, connects,
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
