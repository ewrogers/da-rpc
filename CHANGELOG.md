# Changelog

## Unreleased

### Fixed

- Made long-range native routes combine live collision with complete raw map
  statics, and recover safely when a queued step becomes blocked.
- Retained blocked ground-route destinations for bounded, progressively delayed
  retries when live occupants or doors temporarily leave no path.

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
