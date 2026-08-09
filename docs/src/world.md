# World and movement

World data describes the current map, the objects this client can see, and the
character's native walking state. It is a client-sized view of the world, not a
permanent list of everything on the map.

| Use | Route or events |
| --- | --- |
| Read map and self position | `GET /clients/{client}/status` |
| Read visible objects | `GET /clients/{client}/objects` |
| Turn, step, walk, or emote | `POST /clients/{client}/...` |
| Watch movement, objects, and visuals | [World events](events.md#world-object-events) |

## Map and position

The current map is part of:

```text
GET /clients/{client}/status
```

It includes the map ID, available name, zero-based x/y coordinates, width, and
height.

Ordinary movement acknowledgements update the character's absolute x/y
position. A refresh or server correction can also replace it with an
authoritative position.

A map change arrives in two parts. The client first receives the new map
identity and size, then receives the character's position on that map. daRPC
holds the first part until the position arrives and publishes both together in
one `location.changed` event. It does not expose a new map with coordinates
left over from the previous map.

## Visible objects

```text
GET /clients/{client}/objects
```

The response can contain four object kinds:

| Kind | Available data |
| --- | --- |
| `player` | ID, optional name, x/y, and direction |
| `monster` | ID, optional sprite, x/y, and direction |
| `mundane` | ID, optional name and sprite, x/y, and direction |
| `item` | ID, sprite, x/y, and per-tile `z_index` |

Mundane is the Dark Ages name for a non-player character (NPC). The `npc`
filter remains accepted as an alias. Item sprite values have the client's
internal item classification flag removed.

Ground-item `z_index` is local to one tile. Zero is the bottom item, and higher
values are drawn above it.

```text
WorldObjects {
    observation: ObservationMetadata,
    objects: WorldObject[]?,
}

WorldObject =
    Player { id, name?, x, y, direction }
  | Monster { id, sprite?, x, y, direction }
  | Mundane { id, sprite?, name?, x, y, direction }
  | Item { id, sprite, x, y, z_index }
```

Filter the result with a comma-separated `types` query:

```text
GET /clients/ZiLo/objects?types=player,mundane,monster
```

Without `types`, the route returns every observed kind. An unknown type or
malformed filter returns `400 Bad Request`.

The initial baseline walks the client's retained object collection. A creature
name or numeric sprite can be unavailable after a late attach when the client
no longer retains the original draw details. Pressing the normal client refresh
key causes the server to redraw nearby objects and fills those details again.

## Object events

The complete object and visual payload structures are in
[World object events](events.md#world-object-events).

Players, monsters, and Mundanes each use these actions:

```text
player.appeared              monster.appeared              mundane.appeared
player.disappeared           monster.disappeared           mundane.disappeared
player.moved                 monster.moved                 mundane.moved
player.direction_changed     monster.direction_changed     mundane.direction_changed
```

Ground items use:

```text
item.appeared
item.disappeared
item.moved
```

Each object event carries the complete public object after the change.
Disappearance carries the last retained object. `objects.cleared` marks a map
or world boundary and carries no object.

The server normally sends draw events for objects entering view but may not send
an explicit removal when the local character simply walks out of range. After
accepted self movement, daRPC culls retained objects outside the client-sized
view and reports their disappearance. The collection is still this client's
latest observation rather than an authoritative map population.

## Entity visual events

The stream also reports temporary visuals for visible players, monsters, and
Mundanes:

```text
player.animated              monster.animated              mundane.animated
player.effect                monster.effect                mundane.effect
player.damaged               monster.damaged               mundane.damaged
```

An `*.animated` payload contains the complete entity, the client animation
number, and `initial_duration_ms`. That timer is the initial value sent by the
server, not a promise that the animation remains visible for exactly that long.

An `*.effect` payload contains the entity and the one-based `effect` number
sent by the server. It can also contain a source entity and a frame interval
when the packet supplies them. Effects drawn only at ground coordinates are
not published yet.

An `*.damaged` payload contains the entity and `health_percent`, the server's
0 through 100 value used for the temporary health meter. It is a percentage,
not the amount of damage dealt.

## Turning

```text
POST /clients/{client}/turn
```

```json
{
  "direction": "north"
}
```

The direction must be `north`, `east`, `south`, or `west`. daRPC calls the
client's native direction path on its main thread. An observed direction
request produces `character.turned` with the requested direction.

## Emotes

```text
POST /clients/{client}/emote
{"name":"wave"}
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

```text
POST /clients/{client}/walk
```

```json
{
  "direction": "south"
}
```

This performs a native directional step. It does not create a queued route, so
`is_walking` remains false and the resulting position arrives through
`location.changed`.

## Walking to a tile

```json
{
  "destination": {
    "x": 20,
    "y": 14
  }
}
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

`walking.started` includes the current position and the requested destination
when daRPC knows it.

`walking.stopped` includes the final current position, the available
destination, and `reached_destination`. The outcome is true only when the final
position equals the requested tile. A route started directly through the game
may not expose a reliable destination, so its destination and outcome can be
null.

Both route events update `is_walking` in [character status](status.md).

## Command completion

Turn and walk requests use the common bounded main-thread command system. The
HTTP response tells you whether the local command executed, failed, or remains
queued. Later location and walking events describe what happened in the game;
the command is not retried automatically.

See [Web API](web-api.md#native-command-results) for command status, timeout,
and cancellation behavior.
