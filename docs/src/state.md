# Client state

Local state is one of the main advantages of direct client integration.
`darpc.dll` maintains the authoritative daRPC view for its process and exposes
that view to `darpcd.exe` without exposing raw pointers.

## Initial snapshot

Attaching to an existing process requires more than observing future events.
`darpc.dll` first reconstructs current state from validated pointers, relative
virtual addresses, and version-specific data structures.

Snapshot results must represent partial, unavailable, and unknown state
explicitly. Missing information must not be replaced with invented defaults.
The snapshot should distinguish at least:

- Character and session state.
- Game-world and entity state.
- Local user interface state.
- Client version information.

## Incremental updates

After the snapshot, relevant client events and packets update the local model.
Game-world and user interface changes may have different consistency and
lifetime rules, so they should remain distinct in the state model even when
they are exposed through one API.

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

Every new `darpcd.exe` connection receives a fresh complete snapshot followed by
later updates. `darpc.dll` must establish an ordered boundary so an update cannot
be lost between capturing the snapshot and subscribing the daemon to events.
The implementation may use a state revision, sequence number, or synchronized
queue, but the protocol-visible ordering guarantee must be explicit.

Events produced while `darpcd.exe` is down do not require an unbounded replay
log. A new snapshot restores current durable state, and real-time delivery
resumes from its boundary. Transient event history during the outage is not
recovered unless a later requirement introduces a deliberately bounded log.
