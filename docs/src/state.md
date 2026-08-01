# Client state

Local state is one of the main advantages of direct client integration.
`darpc.dll` maintains the authoritative daRPC view for its process and exposes
that view to `darpcd.exe` without exposing raw pointers.

## Current snapshot

Attaching to an existing process requires more than observing future events.
`darpc.dll` reconstructs current state from validated relative virtual
addresses, pointer chains, and version-specific structures. The implemented
snapshot contains:

- Lifecycle, revision, capture tick and duration, and world generation.
- Character identity, name, gender, class, gold, progression, attributes,
  vitals, combat modifiers, and elemental affinities.
- Map identity, name, coordinates, and dimensions.
- Occupied inventory and equipment slots with appearance, names, quantities,
  and durability where applicable.
- Occupied spellbook and skillbook slots with names, icons, levels, target
  behavior, text-input prompts, lines, and available cooldown state.

Empty collection slots are omitted. Inventory slot 60 is the client's currency
display and is omitted because `gold` is represented once as character state.
Optional values remain unavailable rather than receiving invented defaults.
For example, the client exposes whether a spell action delay is active but not
its remaining duration, so the duration remains absent.

Inventory and equipment sprites are canonical resource identifiers with the
client's item and monster classification bits removed. Stackable inventory
entries expose `can_stack`, keep quantity as its own field, and remove the
client-rendered `[ quantity ]` suffix from the domain name. Equipment uses typed
slot names from `weapon` through `accessory3` rather than exposing numeric
positions. A text-input spell retains an ASCII-only prompt; other target modes
do not expose one.

Map names normally come from the validated map pane. The bounded map-size event
hook also copies an accepted map name into DLL-owned storage so a fresh event
can supplement the memory baseline.

## Lifecycle

Snapshots classify the client as `unknown`, `title`, `transition`, `in_game`,
or `disconnected`. A visible, registered reconnect dialog takes precedence over
the scene beneath it and produces `disconnected`. When a valid world remains
behind that dialog, the snapshot preserves the character and map state that can
still be read. If no valid world is present, character state remains absent.

Reconnect detection scans the active event-pane list during the same
main-thread capture as the rest of the snapshot. The scan validates the list
pointer, signed count, capacity, and a conservative entry limit, and it rechecks
the list roots before publication. Pane pointers are never retained between
captures.

## Capture concurrency

The pipe worker requests a capture and waits with a fixed timeout. The next
client tick performs the memory walk on the client's main thread, where the
relevant user interface structures are normally mutated. The walk validates
roots, lengths, slots, and repeated root values before publishing a result.
There is no process-wide suspension and no remote thread reads client state.

The tick hook copies only bounded, pointer-free fixed-capacity data into a
single DLL-owned publication slot. It does not allocate, serialize, log, or
perform pipe input/output. The pipe worker claims that slot with acquire/release
atomics, decodes client text, allocates domain collections, and serializes the
protocol response. This ownership handoff prevents the worker from observing a
partially written publication. Fields later discovered to be owned by another
client thread will require their own synchronization or an event-owned copy.

## Incremental updates

The current implementation captures a fresh complete snapshot on demand and
when a daemon establishes a connection. Event-driven updates are the next
layer: relevant client events and packets will update DLL-owned state groups,
while an occasional full capture remains the reconciliation source of truth.
This hybrid model supports low-latency change events without requiring a full
memory walk every tick.

Game-world and user interface changes may have different consistency and
lifetime rules, so they remain distinct in the state model even when exposed
through one API.

UI state may include open panes, dialogs, selections, focus, and other
client-only values. These changes are invisible to a pure network proxy and may
need to be obtained from both memory structures and local input events.

## Per-client observations and shared world state

Character, session, and local user interface state always belong to one client.
Map and entity state are different: multiple active characters can observe the
same game world, but each client sees only its current view and can stop
receiving information about an entity after it leaves that view.

The daemon may therefore derive a shared-world projection from compatible
client observations, but that projection is not an unquestioned global truth.
Every shareable observation must retain enough provenance to evaluate it:

- World or server scope and map identity.
- Stable entity identity where the client provides one.
- Source client identity.
- Last-observed time or state revision.
- Whether the entity is currently visible, stale, removed by an authoritative
  event, or otherwise uncertain.

An entity disappearing from one character's view is not by itself proof that
the entity left the world. A fresh observation from another active client on
the same map may supersede an older one. Conflicts and expiration must use an
explicit deterministic policy rather than silently choosing whichever client
reported last.

Queries must also state their perspective. For example, "monsters near player
X" is anchored to player X's current position and may be enriched by fresh
same-map observations from other clients, while exposing or filtering stale
results deliberately. The per-client observations remain available even when
the daemon offers this derived view.

## Snapshot and stream boundary

Every new `darpcd.exe` connection receives a fresh complete snapshot. Once
incremental delivery is implemented, it will be followed by updates from an
explicit ordered boundary so no change can be lost between capture and stream
subscription. Snapshot revisions and protocol sequence numbers provide the
existing foundations for that boundary.

Events produced while `darpcd.exe` is down do not require an unbounded replay
log. A new snapshot restores current durable state, and real-time delivery
resumes from its boundary. Transient event history during the outage is not
recovered unless a later requirement introduces a deliberately bounded log.
