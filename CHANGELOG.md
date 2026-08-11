# Changelog

## Unreleased

## 1.1.2 - 2026-08-11

### Added

- Added `POST /clients/{client}/resync` to request the same server refresh as
  the client's F5 key.
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
