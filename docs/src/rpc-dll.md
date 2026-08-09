# `darpc.dll`

This chapter describes the injected component's responsibilities and lifecycle.
Most tool authors can treat it as the client-side engine behind the
[Web API](web-api.md).

`darpc.dll` is a 32-bit x86 dynamic-link library injected into one compatible
game client. It provides the bridge between the client's internal event system
and the daRPC named-pipe protocol.

## IPC lifecycle

`darpc_initialize` validates the host identity and starts one IPC worker. The
worker binds `\\.\pipe\da-rpc-{pid}` before initialization reports success,
then waits for one local controller without touching the game thread. `DllMain`
does not start IPC or wait for the worker.

Each connection begins with the DLL's `Hello` and must answer with a compatible
`HelloAck`. The worker serves bounded `Ping`, `Echo`, tick health, snapshot,
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

The client dispatches decoded server events through a central handler. daRPC
observes supported events after the client handles them, preserving the
client's original result and behavior.

Client event handlers also use a common function to queue outgoing network
actions. Integrating at that boundary allows daRPC to initiate actions through
the client's own serialization and encryption path rather than implementing a
second packet stack.

Native actions run through the client's own methods on its main thread. This
keeps local interface state and client timing in the normal path instead of
trying to recreate an action with a packet alone.

## State ownership

When attached to a running client, `darpc.dll` reconstructs a snapshot from
validated pointers, relative virtual addresses, and version-specific client
layouts. Capture is scheduled through the client tick hook and runs on the
client main thread. Bounded raw values are published to the pipe worker, which
owns text decoding, allocation, and serialization. See [Game data](state.md)
for the snapshot surface and concurrency model.

The DLL also observes the central decoded-event dispatcher after original
handling. Bounded status, collection, effect, position, world-object, and
message values update the retained state and enter a fixed 1 MiB queue as
ordered mutations. Map-size metadata is staged until an authoritative position
completes the transition. The pipe worker serves those mutations through
bounded long polls. It requests no allocation, logging, serialization, or IPC
work from the hook path.

A separate observer watches the common outbound submission boundary after the
client has processed an action. It copies only recognized bounded fields,
preserves the original result, and records ordered ability, item, gold,
equipment, pickup, emote, and turn events. The DLL tracks an active delayed
spell so completion, server or client cancellation, and replacement by another
spell remain distinct.

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

The diagnostic executor calls no client function and changes no game state.
Turn and walk executors resolve only the supported live world and call the
client's confirmed direction, collision, reset, and pathfinding functions on
the main thread. Exact-tile walking checks current zero-based map bounds first;
a native builder that cannot reach a valid tile reports `no_path`. It uses the
ground route builder and never enables the client's target pursuit or automatic
attack loop. The DLL retains a daRPC-requested destination until the native
route stops, then compares it with the latest accepted position for ordered
walking lifecycle events. Terminal results remain queryable for a bounded
period; new work may evict the oldest completed result rather than allowing
retained history to consume pending queue capacity.

Skill use resolves the live lower-tray root, skill inventory, pointer table,
and one-based entry on the main thread, then calls the client's normal skill
activation routine. These pane objects exist independently of the visible tab;
daRPC does not select the skill page, synthesize input, or disturb focus. A
missing or changed entry fails closed. The native routine retains its ordinary
action-delay checks and configured skill-text behavior.

Spell casting resolves the equivalent live spell entry and checks its expected
argument type, action delay, denial state, object or map target, and bounded
text before calling the matching native routine. It supports no-argument,
object-target, tile-target, and text-input spells without selecting the spell
page or synthesizing input. A new cast is allowed to replace a delayed cast in
progress; ordered outbound observations identify the interrupted and new
spells separately.

Item use resolves the live inventory pane and calls its ordinary activation
routine. Item and gold transfers, pickup, equipment removal, and emotes build
their small confirmed opcode-first request bodies and submit them through the
client's normal plaintext network boundary on the main thread. Tile actions
check the current map bounds. Item actions revalidate the live slot, retained
slot number, stackability, and quantity immediately before submission.

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
