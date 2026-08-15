# Runtime hooks

This chapter explains which parts of the client daRPC observes and why some
actions must run on the game thread. It intentionally stays above instruction
addresses and assembly details. API consumers do not need to manage these hooks.

A hook gives daRPC a short callback at a useful point in the normal client
flow. The original client function still runs. daRPC uses the callback to copy
bounded pieces of state, observe an action, or execute one queued native command.

Hooks are installed only after the executable has been identified as the exact
supported client build.

## Installed hooks

| Hook | Purpose |
| --- | --- |
| Client tick | Captures requested baselines, settles collection changes, watches walking state, and executes at most one queued native command. |
| Decoded server event | Observes supported updates after the client has handled them, including status, inventory, abilities, effects, objects, movement, and messages. It captures correlated daRPC Who and player-inspection responses before the client opens their panels. |
| Outbound packet submission | Observes supported ability, item, gold, equipment, emote, pickup, turn, Who, player-inspection, and local slash-command requests before encryption. |
| Map size | Captures map identity, name, and dimensions so a map change can be committed atomically with the following position. |
| Native path control | Combines live and complete-map collision during breadth-first search, recovers failed queued steps, and copies the retained route before movement consumes its first step. |

These five hooks have different jobs because no single client boundary provides
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
- Detects confirmed steps that shorten the retained planned route
- Replans a retained native ground destination after a queued step is rejected
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
- Body animations, attached visual effects, and temporary health meters for
  visible living entities
- Chat and system messages
- Merchant and pursuit dialog pages
- Field-map panels, destination lists, and submitted selections
- Player exchange state, offers, acceptance, completion, and cancellation

Unknown, malformed, oversized, or unreadable events are ignored. The client's
original result is preserved. The intentional exceptions are Who and
other-player information responses matched to daRPC requests. Those responses
are copied for the waiting controller and are not passed to their stock
panel-opening handlers. Player-started requests still run normally.

The pre-dispatch checks are dormant unless daRPC has an outstanding Who or
player-inspection request. The x86 detour checks the pending markers and opcode
`0x36` or `0x34` before entering Rust, and restores the registers it uses before
continuing into the client. Ordinary server events therefore take the original
dispatcher path without client-memory reads or Rust work before dispatch.

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
- Who requests, including whether daRPC or the player started each request
- Object-information requests (`0x43` subtype 1), correlated by visible ID and order
- Public-speech slash commands and escaped literal slashes

NPC dialog responses use native main-thread methods and are observed through
their retained dialog state. This preserves the visible page and the client's
normal response-pending transition without constructing dialog packets in
daRPC.

Field-map server events are observed after native dispatch so daRPC can confirm
that an exact `FieldMapPane` was registered and made visible. Each tick rescans
the bounded live pane collection to detect closure. Outgoing packet `0x3F` is
the only selection-submitted signal; receiving `0x2E` or observing the native
click animation alone is insufficient.

This is how daRPC reports ability and action events for requests started
through either daRPC or the normal game interface. It also helps keep spell
replacement and cancellation ordering sensible.

Only the recognized, bounded fields needed by the state model are copied. Full
packet bodies are not retained or written to the diagnostic log. Original
client submissions continue normally except that one-slash commands are
suppressed and a double-slash escape is submitted with one slash removed.

## Atomic map changes

The game supplies a new map and the character's new position in separate
updates. Publishing the map immediately could briefly pair it with coordinates
from the old map.

The map-size hook stages the new map ID, name, width, and height. The decoded
event hook commits that staged map only when the following position arrives.
Snapshots and ordinary movement publication pause across this short boundary.

Consumers therefore see one `location.changed` event containing a consistent
map and position.

## Planned route capture

The path-control hook validates three exact version-741 code contracts. Its
planning wrapper requires both native collision modes to accept each candidate
edge. The live mode retains current static replacements, door states, and known
dynamic occupants. The raw mode reads complete map storage, including statics
outside the rendered viewport. The client's ordinary per-step live validator
remains authoritative before any movement request is sent.

The collision wrapper also consults a preallocated, map-tagged exclusion bit
set after both native modes accept a candidate destination. A bounded sparse
registry retains configured tile lists across map changes. The main thread
builds the matching map into the inactive dense bitset and atomically swaps it
active during a map transition. The hook only reads the last complete bitset,
without locking, allocation, or a registry lookup.

The failed-step wrapper checks the result that the stock queued-route code
ignores and publishes the rejected edge as `walking.obstructed`. It retains a
native ground route for two one-second queued-step retries before a full reset
and replan, preserves the timed retry generation for entity pursuit, and
cancels an injected exact route without native replanning. Replanning is
deferred so breadth-first search is never entered recursively from the movement
validator.

The path-builder entry hook runs after the client's breadth-first search succeeds.
It reads the retained 12-byte step records, reverses their goal-to-start queue
order, and expands direction values into absolute start-to-goal tile positions.
The client tick compares the same remaining-step prefix after movement so
confirmed consumption also produces a route revision. Pathfinder generations
distinguish rebuilds, including pursuit routes that happen to select identical
tiles.

The game-thread callback writes only to preallocated route buffers. Four event
buffers bound pending revisions; exhaustion requests the normal snapshot
resynchronization instead of blocking movement or allocating in the hook.

Exact route commands validate a bounded, map-tagged cardinal tile sequence on
the main thread. They use the client's native vector append helper to create
12-byte `direction, source_y, source_x` records in goal-to-start order, publish
the installed route, and start the first step through normal queued movement.

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
3. Remove the outbound, event, path-builder, map, and tick hooks in safe reverse order.
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
