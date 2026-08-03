# Progress

Last updated: 2026-08-02

This is the working task tracker for the
[roadmap](docs/src/roadmap.md). The roadmap defines milestone scope and
completion criteria. This file tracks current implementation work and should
remain concise.

## Current focus

M15, the first typed client actions, is complete. M16 is the next planned
increment and will add bounded outgoing-packet observation and local rules.

## Milestone snapshot

| Milestone | Status | Notes |
| --- | --- | --- |
| M0, workspace scaffold | Complete | Workspace and documentation checks pass. |
| M1, local DLL lifecycle | Verification pending | Functional and manually exercised; Windows CI and explicit failed-initialization host coverage remain. |
| M2, loader attach MVP | Complete | Inspect, attach, detach, JSON results, timeouts, and Windows integration coverage are implemented. |
| M3, loader launch MVP | Complete | Suspended launch, pre-resume initialization, argument forwarding, and owned-child cleanup are implemented and Windows-tested. |
| M4, client bootstrap without hooks | Complete | Exact validation, live launch and attach, lifecycle evidence, and normal-use acceptance pass. |
| M4.1, optional launch patches | Complete | Exact 7.41 patch contracts, strict endpoint selection, safe pre-resume application, and live-client acceptance pass. |
| M5, minimal binary protocol | Complete | Exact framing, identity handshake, diagnostics, checked codecs, and golden and boundary tests are implemented. |
| M6, direct IPC diagnostics | Complete | The DLL pipe worker and direct hello, ping, and echo commands pass controlled and live-client Windows verification. |
| M7, daemon client registry | Complete | Repeated explicit `--pid` targets, shared controller sessions, independent reconnecting workers, and identity-safe registry records pass controlled and live-client verification. |
| M8, read-only HTTP API | Complete | Loopback Axum routes, client identity snapshots, generated OpenAPI, and vendored Swagger UI pass controlled and live-client verification. |
| M9, discovery and managed launch | Complete | Exact-window discovery and API-managed load, unload, and constrained launch pass native Windows integration coverage. |
| M9.1, automatic managed loading | Complete | Opt-in one-shot loading covers existing and later discoveries while preserving explicit unload. |
| M10, hook qualification harness | Complete | Transactional x86 detours, relocated trampolines, rollback, concurrency, panic containment, and shutdown pass owned native tests. |
| M11, first client tick hook | Complete | Exact validation, safety hardening, direct health observation, repeated live attach and detach, and normal in-game acceptance pass. |
| M12, late-attach client snapshot | Complete | Main-thread capture, the full initial character, collection, spell-effect, and observed world-object surface, protocol and API presentation, and live-client comparison pass. |
| M13, event-driven updates | Complete | Bounded status, location, spell-effect, collection, world-object, and typed message updates, daemon reduction and message lookback, resynchronization, per-client SSE, and live-client acceptance pass. |
| M14, main-thread command queue | Complete | Fixed DLL and daemon queues, one command per tick, direct CLI and REST routing, explicit states, and live-client verification pass. |
| M15, first typed action | Complete | Native turn, directional step, exact-tile pathfinding, non-disruptive skill use, validation, walking state and events, and live direct and REST verification pass. |

## Completed recently

- [x] Added a bounded main-thread state walk with validated roots, pointer
  chains, collection capacities, slots, and world-generation detection.
- [x] Added scalar character, appearance, action-lock and blinded state, map,
  inventory, equipment, spellbook, skillbook, and spell-effect models across the DLL,
  protocol, direct CLI, daemon, REST, and OpenAPI.
- [x] Added bounded reconnect-dialog detection and a `disconnected` lifecycle
  that preserves valid character state beneath the dialog.
- [x] Kept allocation, text conversion, serialization, IPC, and logging off the
  hook path through a fixed-capacity publication handoff.
- [x] Verified the snapshot against late-attached live clients.
- [x] Added per-client player, monster, Mundane, and ground-item snapshots with
  typed REST presentation and ordered object events.
- [x] Added bounded draw, remove, movement, and direction packet handling plus
  map-boundary object clears and per-tile item stack ordering.
- [x] Moved snapshot and packet scratch data into guarded DLL-owned storage,
  reserved a 64 KiB snapshot buffer, and verified repeated live snapshots after
  reproducing and eliminating game-thread stack overflow.
- [x] Added protocol 1.0 event polling with snapshot boundaries, absolute
  mutations, strict ordering, and resynchronization results.
- [x] Added the qualified decoded-event observer, bounded packet parsing,
  DLL-owned reducer cache, and fixed 1 MiB event queue.
- [x] Added daemon event reduction and per-client bounded Server-Sent Events
  with explicit ready, lag, resynchronization, and close behavior.
- [x] Added accepted movement updates and a staged map transition that commits
  map metadata with authoritative coordinates atomically.
- [x] Added active spell-effect capture plus ordered add, remove, and relative
  duration changes from decoded server events.
- [x] Added typed chat and system message parsing, bounded per-client daemon
  lookback, REST history, and channel-specific SSE events.
- [x] Added post-handler inventory, spellbook, and skillbook reconciliation with
  a bounded 5 ms settling window, atomic slot batches, stack-aware semantics,
  no-op suppression, REST reduction, and typed SSE events.
- [x] Treat a full `SUserAppearance` as the post-login resynchronization
  boundary so a DLL loaded at the title screen replaces its initial snapshot
  after entering the game.
- [x] Added strict command request, response, status, cancellation, timeout,
  queue-full, and unavailable protocol results without changing protocol 1.0.
- [x] Added a pointer-free 64-slot DLL queue drained at one entry per client
  tick, plus bounded daemon routing through each existing controller session.
- [x] Added direct CLI and REST diagnostic submission, status, cancellation,
  timing, OpenAPI, and Swagger coverage.
- [x] Added typed turn and walk commands through the existing bounded queue,
  including zero-based destination validation and native `no_path` failures.
- [x] Added native skill use by one-based slot or case-insensitive name without
  selecting the visible skill panel or synthesizing input.
- [x] Added queued-route state plus `walking.started` and `walking.stopped`
  events with current position, requested destination, and reached outcome.

## M15 completion evidence

- [x] Unsupported lifecycle and unavailable map state reject before a native
  call; malformed directions, bodies, and map coordinates return HTTP 400.
- [x] Turn, collision-checked step, and exact-tile pathfinding execute only on
  the client main thread through confirmed native functions.
- [x] Exact-tile walking uses no pursuit target, attack call, or automatic
  retry, and command execution remains distinct from route completion.
- [x] Direct CLI and REST commands pass against the late-attached live client,
  including native `no_path` and out-of-map validation behavior.
- [x] Live Server-Sent Events report both interrupted and reached route
  outcomes with the observed current tile and requested destination.
- [x] Host workspace checks and native x86 and x64 Windows checks pass.

## M14 completion evidence

- [x] Queue submission validates bounded scalar fields on the IPC worker and
  never calls a client function.
- [x] The game main thread drains no more than one command per tick without
  allocation, serialization, logging, IPC, or waits.
- [x] Accepted, executed, failed, cancelled, and timed-out states have strict
  wire and public API representations.
- [x] Queue saturation returns busy immediately, while completed result
  retention cannot consume pending capacity indefinitely.
- [x] Disconnect, replacement, shutdown, and timeout retain no controller or
  client pointers in queued work.
- [x] Host tests, native x86 and x64 tests, direct live IPC, and daemon-routed
  HTTP verification pass on the designated late-attached client.

## M13 completion evidence

- [x] Confirm the first narrow event family and the exact fields it owns.
- [x] Qualify and install an `event_dispatch` observer that always preserves
  original client behavior.
- [x] Copy only bounded, pointer-free values on the client thread without
  allocation, IPC, or logging in the hook.
- [x] Apply ordered absolute updates to DLL-owned state independently of daemon
  availability.
- [x] Define the protocol 1.0 event envelope, ordering, snapshot boundary, and
  resynchronization behavior.
- [x] Update the daemon registry from events and expose bounded Server-Sent
  Events without allowing slow subscribers to block producers or peers.
- [x] Reconcile inventory and ability slots after client handlers, reduce each
  complete batch atomically, and preserve its grouping in public events.
- [x] Live-test same-slot no-op suppression, moves, swaps, quantity changes,
  rapid drop and pickup, and spellbook and skillbook moves against REST and
  Server-Sent Events on the designated Windows client.
- [x] Prove that incrementally maintained fields match a fresh snapshot after
  scripted actions, including sequence-gap and queue-overflow recovery.
- [x] Verify safe repeated attach, event observation, detach, and normal live
  client behavior on Windows.

## M1 completion evidence

- [ ] Add Windows CI that builds the x86 DLL and lifecycle host.
- [ ] Exercise a rejected ABI version in `lifecycle-host.exe` and verify the DLL
  can still unload.
- [ ] Record automated evidence that the DLL is x86 and repeated lifecycle
  cycles leave no module or worker thread behind.

## Established decisions

- `DllMain` remains minimal and does not perform initialization, shutdown, IPC,
  logging, or hook work.
- A completed non-success initialization rolls itself back internally, after
  which the loader may call `FreeLibrary`.
- Uncertain remote lifecycle completion leaves the DLL loaded.
- daRPC-owned hooks and trampolines belong to `darpc_initialize` and
  `darpc_shutdown`.
- Future launch-time client patches run while the launched primary thread is
  suspended. The thread resumes only after patches and DLL initialization
  succeed.
- Existing-process module enumeration is the source of truth for observed
  loaded state. Suspended launch uses the remote load result until Windows
  user-mode loader startup makes enumeration available.
- A lifecycle relative virtual address is used only after the observed loaded
  module path matches the validated DLL path.
- Unflagged launches preserve the stock single-instance behavior. Explicit
  `--allow-multiple` launches apply the validated startup patch before resume.
- Parallels remote commands remain suitable for builds, controlled targets,
  inspection, and late attach. Live launch acceptance uses the active Windows
  interactive token and normal client exit.
- `darpc.exe` talks directly to one DLL and remains usable without the daemon.
  Multi-client aggregation is consumed through the `darpcd.exe` web API.
- Loader and DLL lifecycle paths are server-side configuration. Launch requests
  select the supported client executable but expose no arbitrary argument
  forwarding.
- The daemon aggregates client identity, connection health, and the latest
  complete client snapshot, then exposes focused status and collection views.
- Character and user interface state remain per-client. A future shared-world
  projection must preserve observation source, last-seen freshness, and stale
  or uncertain status.

## Updating this file

- Update the date and checkboxes when work starts or finishes.
- Keep only active and near-term tasks here.
- Move durable design details into the mdBook.
- Update the roadmap status table when a milestone changes state.
