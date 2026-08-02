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
`HelloAck`. The worker serves bounded `Ping`, `Echo`, tick-health, snapshot,
event-poll, and main-thread command operations.
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

When attached to a running client, `darpc.dll` reconstructs a snapshot from
validated pointers, relative virtual addresses, and version-specific client
layouts. Capture is scheduled through the client tick hook and runs on the
client main thread. Bounded raw values are published to the pipe worker, which
owns text decoding, allocation, and serialization. See [Client state](state.md)
for the snapshot surface and concurrency model.

The DLL also observes the central decoded-event dispatcher after original
handling. Bounded status, action-state, accepted-position, and spell-effect
values update a main-thread cache and enter a fixed 1 MiB queue as ordered
mutations. Map-size metadata is staged until an authoritative position
completes the transition. The pipe worker serves those mutations through
bounded long polls. It requests no allocation, logging, serialization, or IPC
work from the hook path.

A complete snapshot records the latest event sequence already represented in
its values. The queue rebases to that boundary, and overflow or an ordering gap
causes the controller to request another complete snapshot. This keeps state
ownership inside the DLL while avoiding an unbounded replay log.

This state tracking is independent of `darpcd.exe`. If the daemon stops, the DLL
continues to update its state and keeps its named-pipe server ready for a new
connection.

## Command execution

The IPC worker validates command fields and submits pointer-free entries to a
fixed 64-slot queue. It may wait up to the protocol's bounded response window
for a state transition, but it never executes client work. The existing tick
hook removes at most one queue entry per tick and publishes accepted, executed,
failed, cancelled, or timed-out status through atomics.

The diagnostic command is the first implemented executor. It calls no client
function and changes no game state. Its purpose is to prove main-thread routing
and expose queue delay, execution duration, and thread identity before typed
client actions are added. Terminal results remain queryable for a bounded
period; new work may evict the oldest completed result rather than allowing
retained history to consume pending queue capacity.

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
