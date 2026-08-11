# Movement and emotes

daRPC uses the client's normal main-thread actions for turning, movement, and
emotes. Native pathfinding stays synchronized with the game and ordinary player
input can still replace or cancel a route.

| Use | Route or state |
| --- | --- |
| Read position and walking state | `GET /clients/{client}/status` |
| Read the complete current planned route | `GET /clients/{client}/status` |
| Turn | `POST /clients/{client}/turn` |
| Step or walk to a tile | `POST /clients/{client}/walk` |
| Play an emote | `POST /clients/{client}/emote` |
| Watch movement changes | [Walking and character action events](events.md#walking-and-character-action-events) |

## Turning

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"direction":"north"}' \
  "http://127.0.0.1:2626/clients/ZiLo/turn"
```

The direction must be `north`, `east`, `south`, or `west`. daRPC calls the
client's native direction path on its main thread. An observed direction
request produces `character.turned` with the requested direction.

## Emotes

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"wave"}' \
  "http://127.0.0.1:2626/clients/ZiLo/emote"
```

Names are case-insensitive. The confirmed names are:

| Ctrl shortcut | Name | Code | Ctrl+Alt shortcut | Name | Code |
| --- | --- | ---: | --- | --- | ---: |
| Ctrl+1 | `smile` | 0 | Ctrl+Alt+1 | `rock` | 25 |
| Ctrl+2 | `cry` | 1 | Ctrl+Alt+2 | `scissors` | 26 |
| Ctrl+3 | `sad` | 2 | Ctrl+Alt+3 | `paper` | 27 |
| Ctrl+4 | `wink` | 3 | Ctrl+Alt+4 | `oof` | 28 |
| Ctrl+5 | `stunned` | 4 | Ctrl+Alt+5 | `speechless` | 29 |
| Ctrl+6 | `raz` | 5 | Ctrl+Alt+6 | `blue` | 30 |
| Ctrl+7 | `surprise` | 6 | Ctrl+Alt+7 | `blush` | 31 |
| Ctrl+8 | `sleepy` | 7 | Ctrl+Alt+8 | `heart` | 32 |
| Ctrl+9 | `yawn` | 8 | Ctrl+Alt+9 | `sweat` | 33 |
| Ctrl+0 | `kiss` | 12 | Ctrl+Alt+0 | `sing` | 34 |
| Ctrl+- | `wave` | 13 | Ctrl+Alt+- | `ack` | 35 |

You may instead provide `{"code":13}`. Numeric codes also keep the unnamed
Alt-only expressions available. A code must be one exposed by the client UI:
0 through 8 or 12 through 35. An observed request produces
`character.emoted` with the numeric code.

## Walking one step

Use the same `/walk` route with a direction:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"direction":"south"}' \
  "http://127.0.0.1:2626/clients/ZiLo/walk"
```

This performs a native directional step. It does not create a queued route, so
`is_walking` remains false and the resulting position arrives through
`location.changed`.

## Walking to a tile

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"destination":{"x":20,"y":14}}' \
  "http://127.0.0.1:2626/clients/ZiLo/walk"
```

Coordinates are zero-based and must satisfy `0 <= x < width` and
`0 <= y < height`. Bad directions, malformed choice bodies, and out-of-map
tiles return `400 Bad Request`. A client that is not in game or has no current
map returns `409 Conflict`.

The client builds and follows its native ground route. daRPC does not select a
living target, pursue it, or automatically attack. Ordinary player input can
cancel or replace the route naturally.

An in-bounds tile can still be unreachable. In that case, the command completes
with `state: "failed"` and `failure: "no_path"`.

## Walking events

`planned_route` on character status retains the client's current native plan:

```text
planned_route: {
    generation: u32,
    tiles: [{ x: i32, y: i32 }, ...]
}
```

`tiles` is absolute and ordered from the character's current tile through the
goal. It is empty when no queued path remains. `planned_route` is null only
before route telemetry is available, such as outside a supported in-game
world.

`walking.route_changed` carries the same `generation` and complete `tiles`
array whenever the native plan changes. A new pathfinder build advances the
generation even if it chooses the same tiles. Confirmed movement consumes the
front step without changing the generation, so the next event has a shorter
array beginning at the new current tile. Native pursuit can rebuild its plan
repeatedly; each observed rebuild is therefore a separate revision. Consumers
replace their saved route with the event payload rather than merging arrays.

`walking.started` includes the current position and the requested destination
when daRPC knows it.

`walking.stopped` includes the final current position, the available
destination, and `reached_destination`. The outcome is true only when the final
position equals the requested tile. A route started directly through the game
may not expose a reliable destination, so its destination and outcome can be
null.

The started and stopped events update `is_walking` in
[character status](status.md). `walking.route_changed` updates
`planned_route`; its empty array is the authoritative cleared plan.

## Command completion

Turn and walk requests use the common bounded main-thread command system. The
HTTP response tells you whether the local command executed, failed, or remains
queued. Later location and walking events describe what happened in the game;
the command is not retried automatically.

See [Web API](web-api.md#native-command-results) for command status, timeout,
and cancellation behavior.
