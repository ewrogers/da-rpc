# Changelog

## Unreleased

### Added

- Added automatic visible-player inspection caching, local self-look identity,
  `player.inspected` and `character.profile_changed` events, and manual REST and
  direct CLI inspection without opening the other-player pane.
- Added in-game slash commands that publish `client.command` SSE events without
  sending the command as chat. A leading `//` escapes the interception and
  sends one literal leading slash.

## 1.0.0 - 2026-08-10

Initial release.
