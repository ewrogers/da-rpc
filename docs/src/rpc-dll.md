# `darpc.dll`

`darpc.dll` is a 32-bit x86 dynamic-link library injected into one compatible
game client. It provides the bridge between the client's internal event system
and the daRPC named-pipe protocol.

## IPC lifecycle

`darpc_initialize` validates the host identity and starts one IPC worker. The
worker binds `\\.\pipe\da-rpc-{pid}` before initialization reports success,
then waits for one local controller without touching the game thread. `DllMain`
does not start IPC or wait for the worker.

Each connection begins with the DLL's `Hello` and must answer with a compatible
`HelloAck`. The worker currently serves bounded `Ping` and `Echo` diagnostics.
It uses overlapped reads, writes, and accepts so `darpc_shutdown` can signal the
worker, cancel pending input/output, and join it before unloading. If bounded
shutdown cannot prove the worker stopped, shutdown fails and the loader leaves
the DLL loaded.

Malformed frames, invalid ordering, and broken connections end only that
connection. The worker returns to listening for a replacement controller. The
pipe is local-only, has one instance, and grants access to the process owner,
Windows system, and administrators. A disconnected or absent controller does
not affect the client process.

## Event integration

The client dispatches input and network events through a tree of user interface
panes. Hooking this dispatch boundary allows daRPC to monitor events, block an
event when explicitly requested, and inject new events.

Client event handlers also use a common function to queue outgoing network
actions. Integrating at that boundary allows daRPC to initiate actions through
the client's own serialization and encryption path rather than implementing a
second packet stack.

Mouse and keyboard events remain important. Some operations change local user
interface state and are not completed by sending a network packet alone.

## State ownership

When attached to a running client, `darpc.dll` reconstructs an initial state from
known pointers, relative virtual addresses, and reverse-engineered client
layouts. It then applies relevant events and packet changes to keep local game,
world, character, and user interface state synchronized.

This state tracking is independent of `darpcd.exe`. If the daemon stops, the DLL
continues to update its state and keeps its named-pipe server ready for a new
connection.

## Operational boundaries

- Client layouts and addresses are version-specific.
- Hooks must not be installed until the executable version and required
  invariants have been validated.
- Hook paths must avoid unbounded work, blocking IPC, and daemon-owned
  lifetimes.
- Initialization and cleanup must not perform substantial work under the
  Windows loader lock.
- Failures must be contained at foreign function and hook boundaries without
  unwinding into the client.
