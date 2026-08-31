# Changelog

## Unreleased

## 1.9.1 - 2026-08-31

### Fixed

- Keep daRPC player-inspection response correlation through longer client or
  server stalls so delayed responses do not open the stock other-player pane.

## 1.9.0 - 2026-08-30

### Added

- Add bulletin boards, trade boards, guild boards, world boards, and player mail across injected state tracking, direct RPC, REST, Server-Sent Events, and the command-line interface. The API supports opening local and requested sources, paging, scrolling, entry navigation, composition, submission, deletion, highlighting, dismissal, and complete active UI state queries.
- Add server-confirmed `bulletin.submitted`, `bulletin.deleted`, and `bulletin.failed` events. Mutation events include the attempted action, raw status, optional server message, and complete bulletin state.

## 1.8.4 - 2026-08-29

### Changed

- Coalesce field-map and message-dialog snapshot reads within a bounded 250-millisecond window to prevent repeated local UI polling from forcing full client captures. Revision-sensitive field-map selection and message-dialog dismissal still force a fresh snapshot.

## 1.8.3 - 2026-08-29

### Fixed

- Support crowded maps by raising the bounded nearby-object snapshot capacity
  to 1,024 and separating it from the 4,096-node object-tree traversal limit.
  Capacity failures now identify the specific limit that was exceeded.

## 1.8.2 - 2026-08-28

### Fixed

- Coalesce main-thread tick work to one run per distinct Windows millisecond
  tick so the client event-dispatcher loop cannot repeat state observation tens
  of thousands of times per second.

### Added

- Allow targeted spells to select invisible players by case-insensitive name or
  retained object ID while their last-known position remains within 14 tiles.

## 1.8.1 - 2026-08-27

### Added

- Add `darpcd --managed` for supervisors and embedded hosts. In managed mode,
  standard-input EOF requests graceful shutdown, including client-worker
  teardown and bounded HTTP-server shutdown, while standard-input read failures
  return a nonzero exit status.

## 1.8.0 - 2026-08-26

### Added

- Expose action source on character turns, walking lifecycle events, route
  changes, planned routes, and active movement status. Sources distinguish
  native client activity from daRPC commands and correlate commands by ID.

### Changed

- Upgrade the wire protocol to 1.8. Protocol 1.8 carries action source through
  snapshots and real-time updates, and is intentionally incompatible with
  protocol 1.7 peers.
- Include action source in daemon REST and Server-Sent Events JSON and in
  direct-client snapshot output.

## 1.7.3 - 2026-08-25

### Added

- Add typed Look and FarLook actions for the tile ahead or a coordinate on the
  current map. Results arrive as correlated `look.result` Server-Sent Events
  containing the resolved tile and server-provided names of NPCs and ground
  items.

### Changed

- Suppress only the bounded native message popup correlated with a typed Look
  or FarLook request, before the original client dispatcher runs. Unrelated
  message dialogs retain their normal client behavior.
- Extend protocol 1.7 with the Look command and result update. Update the DLL
  and its controller or daemon together when adopting this change.

## 1.7.2 - 2026-08-25

### Added

- Add correlated `item.pickup_failed` Server-Sent Events when a ground-item
  pickup receives carry-limit feedback, including the item name, limit, tile,
  destination slot, raw feedback, and submission timing. Ambiguous rapid
  pickup attempts remain available as ordinary `message.system` events rather
  than being attributed to the wrong attempt.

## 1.7.1 - 2026-08-24

### Added

- Return `insufficient_mana`, `resist`, `invalid_target`, and `not_allowed` for
  spell commands that receive the corresponding system feedback during the
  bounded result window. Protocol version negotiation remains at 1.7; update
  the DLL and its controller or daemon together when adopting this change.

## 1.7.0 - 2026-08-23

### Added

- Add ordered `map.requested` and `map.downloaded` events for native game-client
  cache-miss transfers, including the map ID and dimensions. The completion
  event follows the accepted final `0x3C` row after every map row was observed.
- Add the correlated `client.resync_completed` event for the complete refresh
  transaction while retaining protocol version 1.7.
- Include the resync ID, coalesced status, scheduler phase, active resync ID,
  and pending count in successful `POST /resync` responses.

### Changed

- Reconcile each F5 refresh as one ordered transaction. Concurrent physical F5
  and HTTP requests coalesce under one resync ID, ordinary object appearance
  and disappearance events precede `client.resync_completed`, and a missing
  `RefreshUserOK` closes the refresh after a one-second fallback.
- Route physical F5 and `POST /resync` through one movement-safe coordinator.
  Queued movement is cancelled and refresh packet `0x38` is deferred until an
  active step commits, while server-driven correction refreshes remain
  immediate.
- Keep refresh coordination, object reconciliation, completion and fallback
  ordering, and deferred snapshot recovery in one DLL state transaction.
- Put response interception, packet parsing, reusable scratch state, and
  ordered semantic dispatch behind the DLL event-hook seam without adding
  asynchronous work to the client path.
- Gate planned-route expansion on its generation and remaining-step count so
  unchanged client ticks avoid event-buffer claims and native route walks.
- Extend protocol 1.7 with map-download lifecycle updates. Update the DLL and
  its controller or daemon together when adopting this change.
- Inventory the mandatory and optional client launch patches in the README with
  player-facing descriptions of each patch and its purpose.

### Fixed

- Commit every accepted daemon snapshot or state-event batch once, then publish
  REST state and SSE events from that same validated observation. A reduction
  failure now makes public state unavailable, closes affected streams with
  `stream.resync_required`, preserves the last valid internal snapshot, and
  requests a fresh snapshot from the client.
- Own client membership, registry records, and connection workers as one daemon
  roster lifecycle. Worker startup failures remain retryable and removable,
  stale events and commands cannot reach removed clients, and every remaining
  worker is signaled during daemon shutdown.

### Removed

- Remove the protocol 1.7 object-clear update and the SSE `objects.cleared`
  event. Protocol version negotiation remains at 1.7; update the DLL and its
  controller or daemon together when adopting this change.

## 1.6.4 - 2026-08-22

### Added

- Report ground-item palette `dye_color` values through snapshots, REST, SSE,
  and command-line output.

### Changed

- Advance the binary wire schema to protocol 1.7 for the expanded ground-item
  object record.

## 1.6.3 - 2026-08-21

### Fixed

- Avoid attaching generic spell failure feedback to a target when several cast
  submissions are pending and the client feedback cannot identify the cast.

## 1.6.2 - 2026-08-20

### Added

- Add an optional loader and REST launch patch that reveals up to 255 ground
  items while either Alt key is held.
- Include packet, native-object, map, transition, route-mode, and destination
  diagnostics when an exact-route command fails with `invalid_state`.

### Fixed

- Make exact-route replacement transactional so rejected mid-walk routes leave
  the native route, destination tracking, and walking lifecycle untouched.
- Build accepted mid-step exact routes from the staged destination and defer
  their first step until the active client transition commits.
- Treat server movement corrections as route-invalidating updates so external
  exact routes stop without requiring a manual F5 refresh.
- Preserve user-requested self-look responses when an automatic local-player
  inspection is still pending so the native legend view refreshes immediately.

## 1.6.1 - 2026-08-18

### Fixed

- Build replacement destination routes from the client's staged step and defer
  their execution until that step completes, preventing one-tile local position
  desynchronization. Reject overlapping direct steps before native prediction.
- Report `invalid_state` instead of `invalid_destination` when exact-route
  installation detects that the server-authoritative position and the
  client's native local position are desynchronized.
- Launch clients with conventional Win32 executable and working-directory
  paths after canonical validation, preserving legacy in-game audio loading.
- Preserve inherited and client-selected processor affinity during launch
  instead of overriding it during or after startup.

## 1.6.0 - 2026-08-18

### Added

- Add active-walk cancellation through `DELETE /clients/{client}/walk`, the
  direct `darpc walk --pid <pid> cancel` command, and the binary walk command.
- Add `completed`, `obstructed`, `replaced`, `cancelled`, and
  `position_corrected` reasons to `walking.stopped`.
- Add current state, complete SSE updates, and revision-checked dismissal for
  native message dialogs opened by actions such as sense, look, and peek.

### Changed

- Leave native path planning, collision answers, queued-step execution, and
  pursuit behavior unchanged while retaining route and obstruction telemetry.
- Advance the binary protocol to 1.6 for the incompatible movement event,
  snapshot, and command changes.

### Fixed

- Emit spell and skill move events when an ability moves into an empty UI
  slot, matching the existing item movement behavior.

### Removed

- Remove DLL path-exclusion policy, automatic daRPC route retries and replans,
  and the related REST, state, event, and binary protocol surfaces.

## 1.5.4 - 2026-08-17

### Fixed

- Preserve the native stack-quantity prompt for manually added stacked exchange
  items, while skipping it for single items and retaining automatic quantities
  for daRPC requests.
- Reset the encrypted outgoing sequence when the communications worker
  delivers `CHello`, preserving sequence zero during initial startup and when
  returning from a game server to the main login server.

## 1.5.3 - 2026-08-17

### Fixed

- Route a preserved translucent refresh through the full appearance update so
  the walking destination, render displacement, and object-owned translucency
  state remain synchronized.
- Report spell casts rejected by a no-cast map as failed commands when the
  client receives the system message `That doesn't work here.`.
- Derive `is_walking` from accepted movement steps, confirmed position
  progress, and pending route work instead of the client's stale native
  route-active flag.
- Reapply the complete system processor affinity when launched clients restore
  a single-processor mask during startup, for both direct loader and daemon REST
  launches.

## 1.5.2 - 2026-08-15

### Added

- Added opt-in runtime hook timing through loader initialization, direct IPC,
  the direct CLI, and daemon REST endpoints.
- Added bounded per-stage call, duration, maximum, and budget-exceeding counters
  without diagnostic logging, allocation, or IPC from the tick and incoming
  event hooks.

## 1.5.1 - 2026-08-15

### Added

- Exposed `is_solid` for visible world objects and preserved passable monsters
  from server draw packets through snapshots, events, CLI output, and the API.
- Added sustained dispatcher-rate degradation and recovery logging in the
  daemon using the existing atomic tick-health counter.

### Fixed

- Allow spell casts 10 percent tolerance on the one-second command start
  deadline so brief native dispatcher overruns do not drop queued casts.
- Avoid per-tick field-map pane scans until a definition is available, then
  limit visibility polling to ten times per second.
- Tail-jump from the tick detour after daRPC observation so a long-lived client
  dispatcher call cannot prevent graceful DLL unload.

## 1.5.0 - 2026-08-15

### Added

- Added authoritative active field-map state, lifecycle and submitted-selection
  events, a revision-checked REST action, and direct CLI selection.

### Changed

- Treat `You failed to concentrate.` as failed `Fas Spiorad` feedback while
  preserving other queued casts.
- Cache validated field-map packets that arrive before the native pane becomes
  visible, then publish them only after the pane is confirmed open.
- Accept bounded trailing field-map extension bytes after the declared
  destination records, matching the 7.41 client parser.
- Reopen retained field-map definitions when the native client reuses its
  cached pane without sending another field-map packet.
- Refresh field-map reads and selections from a live DLL snapshot so unrelated
  state-stream resynchronization cannot leave the daemon cache stale.
- Moved stat-point endpoint guidance into the Character status documentation.

### Fixed

- Track both slots of outgoing inventory, spellbook, and skillbook swaps so
  retained state and collection events reflect manual client rearrangements.

## 1.4.2 - 2026-08-14

### Fixed

- Cleared retained world entities before an explicit client resync and
  preserved packet-observed monster and Mundane sprite IDs through the
  follow-up snapshot.
- Made loader detach reject an on-disk DLL whose Portable Executable identity
  or lifecycle export layout differs from the module loaded in the client.

## 1.4.1 - 2026-08-14

### Fixed

- Correlated the login self-look response with its active automatic inspection
  before the initial in-game snapshot has populated the local character ID.
- Avoided sending a redundant local-player inspection while the client is
  already performing its built-in login self-look request.
- Routed local-player inspections through the client's canonical `0x2D`
  self-look request instead of the other-player `0x43` object-info request.

## 1.4.0 - 2026-08-14

### Added

- Added tracked character stat-point availability plus REST and CLI commands
  that spend one point through client packet `0x47`.
- Exposed complete human and transformed-player sprite and color visuals from
  opcode `0x33` through snapshots, events, the REST API, and CLI JSON output.
- Added group invitations by validated character name without requiring the
  target to be visible.
- Added optional `cooldown_ms` total-duration metadata for active skill and
  spell cooldowns, including authoritative live spell timing from server
  action-delay packets.
- Added `player.replaced` SSE events and automatic removal of stale same-name
  player objects when a relog assigns a new object ID.
- Added local and visible-player `is_hidden` state, including zero-body and
  translucent player detection.
- Added `character.appearance_changed` and `character.hidden_changed` SSE
  events after authoritative snapshot recapture.

### Changed

- Rate-limited stat-point spending to one request per character every 500
  milliseconds; faster requests return HTTP 429.
- Leaving or disbanding a group now reopens invitations by default. The group
  toggle endpoint accepts `{"leave_open": false}` to retain the original
  single-toggle behavior.
- Clamped remaining cooldown time to the total duration so reported progress
  stays within its valid range.
- Preserved the last visible local appearance and remote player name and
  inspected profile when a hidden draw omits those fields.
- Replaced the Windows build-output cache with a dependency-focused Rust cache
  and canceled superseded pull-request workflow runs.
- Advanced the binary protocol from version 1.1 to 1.3 for cooldown timing,
  player replacement, and hidden-state fields and events.
- Advanced the binary protocol from version 1.3 to 1.4 for complete
  visible-player visual blocks, character stat points, and stat spending.

### Fixed

- Unified daRPC destinations, in-game ground clicks, and pursuits on the same
  augmented native pathfinder, including constant-time visible-player
  occupancy checks that route around occupied tiles.
- Retried blocked native ground-route steps twice at one-second intervals, then
  replanned from the latest confirmed tile, using a bounded observation cadence
  to avoid affecting client animation timing.
- Kept recovery active when a rebuilt route's first step was immediately
  rejected, and separated visible-player path occupancy from general object
  cache reconciliation.
- Accepted group invitation and toggle request bodies at the HTTP routing
  boundary.
- Kept skill cooldown snapshots advancing to ready when elapsed remaining time
  differs from the daemon's retained baseline.
- Isolated automatic self-look and object-info response suppression so one
  response family cannot consume the other's pending inspection.
- Corrected the native spell-denial calling convention to prevent access
  violations when casting spells.
- Suppressed the self-look panel produced when the local player's `0x33`
  login draw triggers automatic profile inspection.

## 1.3.0 - 2026-08-13

### Added

- Added map-tagged exact route injection through the client's native queued
  movement vector.
- Added a session-persistent sparse registry of per-map pathfinding exclusions
  for native breadth-first search, with map-scoped REST resources and automatic
  activation on map changes.
- Added `map.exclusions_changed` events for exclusion replacement, removal, and
  registry clearing.
- Added `walking.obstructed` events with the rejected edge, route context, and
  retained destination.

### Changed

- Advanced the binary protocol to version 1.1 for the new movement, exclusion,
  and event discriminants.

## 1.2.5 - 2026-08-13

### Fixed

- Read group membership and the leader marker from the server's self-look
  roster instead of an invalid client-memory layout.
- Refresh group membership after server-confirmed joins and disbands without
  opening the client's self-look interface.

## 1.2.4 - 2026-08-12

### Fixed

- Suppressed the client self-look window when daRPC automatically inspects the
  local player after login, while preserving user-requested self-look windows.

## 1.2.3 - 2026-08-12

### Fixed

- Retried the automatic self inspection after the initial map transition so
  character identity fields, including player class, populate without clicking
  the local player.
- Cleared walking and planned-route state when the client changes maps.

## 1.2.2 - 2026-08-12

### Fixed

- Made every new supported-client launch reset the encrypted outgoing packet
  sequence before queuing `CHello`, preventing a bootstrap race that could
  reuse sequence zero for `CMulti` and disconnect before server transfer.
- Made the heartbeat priority queue participate in the client's native FIFO
  empty check so an otherwise idle client wakes and sends its queued heartbeat
  instead of waiting for unrelated outgoing traffic.

## 1.2.1 - 2026-08-11

### Added

- Added `message.internal` REST and Server-Sent Events support for daRPC-only
  inter-client messages, including optional named recipients and structured
  payloads.
- Added `skill.cooldown`, `skill.ready`, `spell.cooldown`, and `spell.ready`
  events with targeted client-side cooldown completion tracking.

### Fixed

- Made initial location tracking independent of whether the authoritative
  position or map-size event arrives first after login.
- Prioritized heartbeat responses outside the normal outbound transport queue
  so high-volume game events cannot delay them until the server disconnects.
- Classified unformatted whisper errors as whisper messages even when they do
  not include the normal sender-name syntax.

### Changed

- Reserved `skill.changed` and `spell.changed` for intrinsic ability metadata
  changes instead of cooldown-only transitions.

## 1.2.0 - 2026-08-11

### Added

- Added `POST /clients/{client}/messages/send` for bounded say, shout, guild,
  group, and whisper messages, including recipient validation for whispers.

## 1.1.2 - 2026-08-11

### Added

- Added `POST /clients/{client}/resync` to request the same server refresh as
  the client's F5 key.
- Added `client.resync` events when the client sends an F5 refresh request.
- Split emotes into a dedicated documentation chapter and added an accessible
  pathfinding guide covering route rules, best-effort behavior, and recovery.

### Fixed

- Replanned active destination walks from the corrected tile when the server
  sends an authoritative user-position update.

## 1.1.1 - 2026-08-11

### Fixed

- Made long-range native routes combine live collision with complete raw map
  statics, and recover safely when a queued step becomes blocked.
- Retained blocked ground-route destinations for bounded, progressively delayed
  retries when live occupants or doors temporarily leave no path.
- Replanned locally accepted routes that receive no confirmed position progress
  instead of leaving an empty native queue marked active indefinitely.
- Preserved a queued replan when the previous step is confirmed in the same
  client tick that the following step becomes blocked.

## 1.1.0 - 2026-08-10

### Added

- Added complete native planned-route snapshots to client status and
  `walking.route_changed` events with pathfinder generations and absolute tile
  arrays for rebuilds, confirmed-step consumption, and route clearing.
- Added bounded raw map-file downloads through
  `GET /maps/{map_id}/download`, with automatic local client map-directory
  discovery and an optional `--maps-path` override.
- Added automatic visible-player inspection caching, cache-only lookup by
  visible player name, local self-look identity, `player.inspected` and
  `character.profile_changed` events, and manual REST and direct CLI inspection
  without opening the other-player pane.
- Added in-game slash commands that publish `client.command` SSE events without
  sending the command as chat. A leading `//` escapes the interception and
  sends one literal leading slash.

## 1.0.0 - 2026-08-10

Initial release.
