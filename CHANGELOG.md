# Changelog

## Unreleased

### Added

- Added authoritative active field-map state, lifecycle and submitted-selection
  events, a revision-checked REST action, and direct CLI selection.

### Changed

- Cache validated field-map packets that arrive before the native pane becomes
  visible, then publish them only after the pane is confirmed open.
- Accept bounded trailing field-map extension bytes after the declared
  destination records, matching the 7.41 client parser.
- Reopen retained field-map definitions when the native client reuses its
  cached pane without sending another field-map packet.
- Refresh field-map reads and selections from a live DLL snapshot so unrelated
  state-stream resynchronization cannot leave the daemon cache stale.
- Moved stat-point endpoint guidance into the Character status documentation.

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
