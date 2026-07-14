# Client state

Local state is one of the main advantages of direct client integration.
`rpc.dll` maintains the authoritative daRPC view for its process and exposes
that view to `rpcd.exe` without exposing raw pointers.

## Initial snapshot

Attaching to an existing process requires more than observing future events.
`rpc.dll` first reconstructs current state from validated pointers, relative
virtual addresses, and version-specific data structures.

Snapshot results must represent partial, unavailable, and unknown state
explicitly. Missing information must not be replaced with invented defaults.
The snapshot should distinguish at least:

- Character and session state.
- Game-world and entity state.
- Local user interface state.
- Client and layout version information.

## Incremental updates

After the snapshot, relevant client events and packets update the local model.
Game-world and user interface changes may have different consistency and
lifetime rules, so they should remain distinct in the state model even when
they are exposed through one API.

UI state may include open panes, dialogs, selections, focus, and other
client-only values. These changes are invisible to a pure network proxy and may
need to be obtained from both memory structures and local input events.

## Snapshot and stream boundary

Every new `rpcd.exe` connection receives a fresh complete snapshot followed by
later updates. `rpc.dll` must establish an ordered boundary so an update cannot
be lost between capturing the snapshot and subscribing the daemon to events.
The implementation may use a state revision, sequence number, or synchronized
queue, but the protocol-visible ordering guarantee must be explicit.

Events produced while `rpcd.exe` is down do not require an unbounded replay
log. A new snapshot restores current durable state, and real-time delivery
resumes from its boundary. Transient event history during the outage is not
recovered unless a later requirement introduces a deliberately bounded log.
