# Changelog

## Unreleased

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
