# World

World data describes the current map and the objects this client can see. It is
a client-sized view of the world, not a permanent list of everything on the
map.

| Use | Route or events |
| --- | --- |
| Read map and self position | `GET /clients/{client}/status` |
| Read visible objects | `GET /clients/{client}/objects` |
| Watch objects and visuals | [World object events](events.md#world-object-events) |

## Map and position

The current map is part of:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/status"
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

```console
curl "http://127.0.0.1:2626/clients/ZiLo/objects"
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

```console
curl "http://127.0.0.1:2626/clients/ZiLo/objects?types=player,mundane,monster"
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
