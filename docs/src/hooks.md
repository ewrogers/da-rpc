# Runtime hooks

A hook gives daRPC a short callback at a useful point in the normal client
flow. The original client function still runs. daRPC uses the callback to copy
small pieces of state, observe an action, or execute one queued native command.

Hooks are installed only after the executable has been identified as the exact
supported client build.

## Installed hooks

| Hook | Purpose |
| --- | --- |
| Client tick | Captures requested baselines, settles collection changes, watches walking state, and executes at most one queued native command. |
| Decoded server event | Observes supported updates after the client has handled them, including status, inventory, abilities, effects, objects, movement, and messages. |
| Outbound packet submission | Observes supported ability, item, gold, equipment, emote, pickup, and turn requests before encryption. |
| Map size | Captures map identity, name, and dimensions so a map change can be committed atomically with the following position. |

These four hooks have different jobs because no single client boundary provides
all the information daRPC needs.

## Client tick

The client tick is daRPC's safe meeting point with the game main thread.

When requested, it copies a complete state baseline into preallocated DLL
memory. The pipe worker converts and sends that copy later. Regular REST reads
do not request another baseline.

The tick also:

- Reconciles inventory, spellbook, and skillbook slots after their short
  settling window
- Detects when native pathfinding starts and stops
- Executes at most one queued action or diagnostic command
- Publishes small health counters used by hook diagnostics

This work is bounded. The callback does not allocate, serialize, log, wait for
another thread, or perform named-pipe input/output.

## Decoded server events

The event hook runs after the client has successfully handled a recognized
server event. Running afterward lets the client remain authoritative and lets
daRPC compare against the state the client accepted.

The hook copies only bounded data from event families daRPC understands. Those
updates drive:

- Character status, vitals, progression, gold, weight, and modifiers
- Blind and action-restriction state
- Position and map transition completion
- Inventory, spellbook, and skillbook reconciliation
- Active spell effects
- Visible players, monsters, Mundanes, and ground items
- Chat and system messages

Unknown, malformed, oversized, or unreadable events are ignored. The client's
original result is preserved.

## Outbound action observation

Some useful events describe what the client sends rather than what it receives.
The outbound hook watches the common plaintext submission path for:

- Skill use
- The start of a delayed spell
- Each submitted chant line
- Final spell use
- Item use, tile drop, and player or monster exchange requests
- Gold tile drops and exchange requests
- Ground-item pickup, equipment removal, emotes, and turning

This is how daRPC reports ability and action events for requests started
through either daRPC or the normal game interface. It also helps keep spell
replacement and cancellation ordering sensible.

Only the recognized, bounded fields needed by the state model are copied. Full
packet bodies are not retained or written to the diagnostic log. The original
client submission always continues normally.

## Atomic map changes

The game supplies a new map and the character's new position in separate
updates. Publishing the map immediately could briefly pair it with coordinates
from the old map.

The map-size hook stages the new map ID, name, width, and height. The decoded
event hook commits that staged map only when the following position arrives.
Snapshots and ordinary movement publication pause across this short boundary.

Consumers therefore see one `location.changed` event containing a consistent
map and position.

## Main-thread affinity

Most game state is owned and changed by the client main thread. Native movement,
skill, and spell methods also expect the client state associated with that
thread.

Calling them directly from the daemon's pipe worker would create two problems:

1. The game could change a structure while daRPC was reading it.
2. A native method could run in a thread context it was never designed for.

daRPC instead uses bounded handoffs:

```text
read:   client hook -> fixed copy or event queue -> pipe worker -> daemon
action: daemon -> pipe worker -> command queue -> client tick -> native method
```

The client-facing side handles only fixed-size or preallocated data. Text
conversion, JSON, logging, HTTP, and named-pipe work happen away from the game
thread.

The queues are deliberately bounded. If a producer cannot enqueue safely, it
reports pressure or requests a later resynchronization instead of blocking the
client or growing memory without limit.

## Attach and detach lifecycle

Each hook installation is transactional. If a later hook cannot be installed,
DLL initialization removes the hooks already installed and stops the worker.

Detaching follows the reverse flow:

1. Stop accepting new pipe work.
2. Cancel commands that have not started.
3. Remove the outbound, event, map, and tick hooks in safe reverse order.
4. Wait for any callback already in progress to finish.
5. Release DLL-owned state and unload the library.

This order prevents a hook from calling code or using memory that has already
been unloaded. Installation and removal also coordinate with the process's
other threads so an instruction pointer is not left inside code while it is
being replaced or restored.

`DllMain` remains minimal. Substantial initialization and shutdown happen
outside the Windows loader lock.

## Failure behavior

Hook callbacks preserve original client behavior and do not unwind across the
client boundary. A bad or unsupported observation is skipped. A daemon or pipe
disconnect does not remove local client state or terminate the game.

The project also tests the reusable hook mechanism against an owned x86 harness
before qualifying it in the live client. The harness covers installation,
original-function trampolines, concurrent calls, rollback, and removal.
