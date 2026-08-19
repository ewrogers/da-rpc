# Movement

daRPC exposes the game client's stock movement and a narrow exact-route
control surface. It does not replace or improve the client's native planner.
An external bot that needs obstacle avoidance, group-aware costs, or reliable
replanning should own those decisions and submit short exact routes.

## Consumer surface

| Need | HTTP or SSE interface |
| --- | --- |
| Current map, position, walking flag, and planned route | `GET /clients/{client}/status` |
| Visible players, creatures, NPCs, and their positions | `GET /clients/{client}/objects` |
| Current group state and members | `GET /clients/{client}/group` |
| Static map bytes for an external planner | `GET /maps/{map_id}/download` |
| One cardinal step, a stock destination walk, or an exact route | `POST /clients/{client}/walk` |
| Stop the current route | `DELETE /clients/{client}/walk` |
| Ordered movement and world updates | `GET /clients/{client}/events` |

Coordinates are zero-based. Read the current map ID, dimensions, and position
from status before submitting movement.

## Choosing a movement mode

### One step

Submit one direction when the controller wants to drive the character one tile
at a time:

```json
{"direction":"north"}
```

The DLL calls the stock walk helper. A rejected step returns a command failure.
If the helper accepts the step but the character never reaches the adjacent
tile, `walking.stopped` reports `obstructed`.

The client predicts each step until its visual transition commits. A second
direct step submitted during that transition is rejected so it cannot overlap
the prediction. Submit it again after the position update or use a destination
route, which the client can queue safely.

### Stock destination

Submit a destination to ask the client's built-in planner to build and execute
its normal ground route:

```json
{"destination":{"x":120,"y":85}}
```

This mode is intentionally vanilla. daRPC does not change native collision
answers, add player or monster exclusions, retry a rejected edge, or rebuild a
stalled route. Use it when the game's ordinary shortest-path behavior is good
enough. A valid tile with no native path reports `no_path`.

When a destination replaces a route during an active step, the DLL builds from
that step's staged destination and leaves the replacement queued. The client's
normal step-completion callback commits the staged tile and starts the queued
route. Repeated replacements update that queue without starting another
prediction early.

### Exact route

Submit a map-tagged list of absolute tiles when an external planner owns the
route:

```json
{
  "route": {
    "map_id": 3001,
    "tiles": [
      {"x":11,"y":22},
      {"x":12,"y":22},
      {"x":12,"y":23}
    ]
  }
}
```

The route must:

- contain 1 through 256 tiles;
- start at the character's current confirmed position;
- stay on the stated current map and inside its dimensions;
- use unique tiles connected by cardinal one-tile edges; and
- pass both native collision checks at submission time.

The DLL validates the route against both the packet-confirmed position used by
state and events and the position of the client's native local self object. If
those positions or their map IDs disagree, the client is locally
desynchronized and the command fails with `invalid_state`. The route is not
installed, even when its first tile matches `/status`, because native walking
would start from the stale local object. Stop submitting routes until the
client has resynchronized; the supported client normally refreshes its local
world state when the user presses F5. Reread `/status` before planning again.

The DLL places the validated route into the client's native route vector and
starts its normal walker. Animation, packets, acknowledgements, and pacing
remain client-owned. Route injection is not a teleport and does not bypass the
live step validator.

If a later exact-route edge is rejected, daRPC emits
`walking.obstructed`, then `walking.stopped` with reason `obstructed`,
and clears the exact route. It does not retry or replan.

## Cancelling movement

`DELETE /clients/{client}/walk` resets the stock route, clears route
telemetry, and emits `walking.stopped` with reason `cancelled` when a walk
was active. The direct CLI equivalent is:

```console
darpc walk --pid <pid> cancel
```

The reset cannot revoke a step the client has already accepted. A final
`location.changed` can therefore arrive after cancellation. Replan from the
latest confirmed position rather than the position reported by the cancel
response.

Replacing an active walk with a destination or route emits reason `replaced`.
A direct step submitted during an active visual transition is rejected and
leaves the current movement intact. Turning while a walk is active emits reason
`cancelled`.

Cancelling a queued command through
`DELETE /clients/{client}/commands/{command_id}` is different. It prevents a
command that has not begun from executing; it does not stop an already active
route.

## Recommended external-planner loop

1. Read status, objects, and group state. Download and cache the raw map when
   the map ID changes.
2. Build the planner's own cost field. Static map collision can be combined
   with temporary dynamic costs from visible objects.
3. Plan from the latest confirmed position. A controller can treat creature
   tiles and nearby safety margins as blocked or expensive, prefer proximity
   to group members, and give unrelated players a smaller cost. Those policies
   belong to the controller because they depend on its goal and risk tolerance.
4. Submit a short exact-route prefix. Short prefixes reduce the amount of work
   invalidated when an object moves.
5. Replace the saved route whenever `walking.route_changed` arrives. Update
   the start position from `location.changed`.
6. On a terminal event, apply the reason-specific recovery below. Never wait
   indefinitely for the same route to recover itself.

A useful starting segment length is 4 through 16 edges. The best value depends
on map density and how quickly the controller receives object updates.

## Stop reasons

`walking.stopped` contains:

```text
walking.stopped {
    observation: EventObservation,
    current: TilePosition,
    destination: TilePosition?,
    reached_destination: bool?,
    reason: completed | obstructed | replaced | cancelled | position_corrected,
}
```

| Reason | Meaning | Suggested controller action |
| --- | --- | --- |
| `completed` | The observed walk ended normally. If a destination is known, `reached_destination` says whether the final tile matches it. | Confirm the current position and submit the next segment if needed. |
| `obstructed` | The walk ended before reaching its known destination, including a rejected edge or an accepted direct step that made no progress. | Penalize `walking.obstructed.attempted` when that event is present, otherwise use `destination`, then replan from `current`. |
| `replaced` | A different route or movement command superseded this walk. | Track the replacement command and discard the old plan. |
| `cancelled` | Movement was explicitly reset or cancelled. | Stop unless the controller deliberately requested cancellation as part of replanning. |
| `position_corrected` | The server corrected the character position while walking. | Discard the route and reread status before planning again. |

`destination` and `reached_destination` can be null for movement initiated
inside the game when no reliable destination was observed.

## Route and obstruction events

`planned_route` in status is the latest observed client route:

```text
planned_route {
    generation: u32,
    tiles: Vec<TilePosition>,
}
```

`walking.route_changed` carries the same fields. Tiles are absolute and
ordered from the current tile toward the goal. A new native build advances the
generation. Confirmed movement consumes tiles from the front without changing
the generation. An empty tile list is the authoritative cleared route.

`walking.obstructed` reports:

```text
walking.obstructed {
    observation: EventObservation,
    map_id: u32,
    current: TilePosition,
    attempted: TilePosition,
    direction: north | east | south | west,
    destination: TilePosition?,
    mode: direct | native_route | exact_route | pursuit,
}
```

The DLL reports the rejection but does not modify native routes or pursuits.
Only a failed externally installed exact route is reset automatically.

## Stream recovery and map changes

SSE is ordered per client. If the consumer receives
`stream.resync_required`, it must reread every resource it uses before
planning again. At minimum, reread status, objects, and group state.

Never submit one exact route across two maps. End the first segment on the warp
tile, wait for the atomic `location.changed` event containing the new map and
entry position, refresh the map and world inputs, and plan a new segment.

## Command completion

The HTTP command response reports whether the main-thread operation completed,
failed, or remains queued. It does not prove that a multi-step walk later
reached its destination. Use ordered location, route, obstruction, and stopped
events for the movement outcome.

See [Web API](web-api.md#native-command-results) for command status and timeout
behavior, and [Events](events.md) for stream ordering and recovery.
