# Movement

daRPC uses the client's normal main-thread actions for turning and movement.
Ordinary player input can still replace or cancel a route.

| Use | Route or state |
| --- | --- |
| Read position and walking state | `GET /clients/{client}/status` |
| Read the complete current planned route | `GET /clients/{client}/status` |
| Turn | `POST /clients/{client}/turn` |
| Step or walk to a tile | `POST /clients/{client}/walk` |
| Request a server resynchronization | `POST /clients/{client}/resync` |
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

## Pathfinding

For destination walks, daRPC asks the game client's own pathfinder to build the
route. It does not choose a target, chase creatures, or attack automatically.
The result is best effort because the world can change after a route is built
and the server always has the final say about whether a step succeeds.

The pathfinder checks two views of the map:

- The complete map data prevents off-screen walls from being treated as open
  space just because the client has not drawn them yet.
- The live world view accounts for visible occupants, doors, and other current
  changes.

A route can therefore fail even when its destination is inside the map. When
no route is available at command time, the command completes with
`state: "failed"` and `failure: "no_path"`.

### Recalculating a route

daRPC keeps the requested destination while it is walking. It recalculates the
route from the character's latest confirmed tile when a step is blocked, when
an accepted step makes no confirmed progress for 1.2 seconds, or when the
server sends an authoritative position correction.

Every submitted step still passes the client's normal live safety check. If a
new obstacle leaves no path at that moment, daRPC retries for up to five seconds.
The delay grows from 250 milliseconds to one second so repeated attempts do not
overload the client. A new movement request, a map change, invalid client state,
confirmed progress, or the five-second limit ends that recovery attempt.

This recovery only applies to destination walks started by daRPC, because those
have a known goal. Routes and creature pursuits started directly in the game
keep their normal client behavior.

## Resynchronizing position

```console
curl --request POST \
  "http://127.0.0.1:2626/clients/ZiLo/resync"
```

This submits the same opcode-only refresh packet as the client's F5 key. The
server normally responds by sending authoritative client state again. When an
authoritative user-position packet arrives during a daRPC-owned walk, daRPC
cancels the stale native route and rebuilds it from the corrected position to
the retained destination on the next main-thread tick. This also applies when
the server initiates the correction after rejecting a step or detecting a
collision; calling `/resync` is not required for route recovery.

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
repeatedly, and a blocked daRPC ground route can rebuild toward its retained
destination. Each observed rebuild is therefore a separate revision. Consumers
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
