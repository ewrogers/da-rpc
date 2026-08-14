# World

World data describes the current map and the objects this client can see. It is
a client-sized view of the world, not a permanent list of everything on the
map.

| Use | Route or events |
| --- | --- |
| Read map and self position | `GET /clients/{client}/status` |
| Read visible objects | `GET /clients/{client}/objects` |
| Read one cached visible player by name | `GET /clients/{client}/players/{player}` |
| Refresh one visible player | `POST /clients/{client}/players/{player}/inspect` |
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
| `player` | ID, optional name, x/y, direction, `is_hidden`, optional visual, and optional inspected profile |
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
    Player { id, name?, x, y, direction, is_hidden, visual?, profile? }
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

### Player visuals

Opcode `0x33` supplies a `visual` block for each drawn player. A normal player
uses `form: "human"` and exposes every sprite used by the client renderer:
head, body, arms, boots, pants, armor, weapon, shield, overcoat, and three
accessories. `hair_color` and `skin_color` are top-level visual fields, alongside
the boots, pants, overcoat, and accessory dye colors. The block also includes
gender, rest position, face shape, and translucency flag used by the renderer.

```text
HumanVisual {
    form: "human",
    gender,
    head_sprite, body_sprite, arms_sprite, boots_sprite, pants_sprite,
    armor_sprite, weapon_sprite, shield_sprite, overcoat_sprite,
    accessory1_sprite, accessory2_sprite, accessory3_sprite,
    hair_color, skin_color, boots_color, pants_color, overcoat_color,
    accessory1_color, accessory2_color, accessory3_color,
    rest_position, face_shape, is_translucent,
}

CreatureVisual {
    form: "creature",
    sprite, color, boots_color, pants_color,
}
```

A transformed player uses `form: "creature"` and exposes the creature sprite
plus the three color bytes carried by that packet layout. Creature-form draws
are not treated as hidden. `visual: null` means the player was synthesized from
partial retained state before a complete packet or memory appearance was
available; no sprite or color defaults are invented.

### Player profiles

When opcode `0x33` draws a human, daRPC automatically requests that player's
object information. The successful response fills `profile` with nation, title,
guild rank, display class, guild, user state, `is_group_open`, worn equipment,
legend marks, and `inspected_tick_ms`. `profile: null` means the player is
visible but the inspection has not completed.

`is_hidden` is true when the draw has a zero body sprite or the packet marks
the player translucent. Hidden draws can zero other fields, so daRPC merges
them by entity ID and retains the last observed name plus inspected profile.
Monster-form draws use the `creature` visual layout and are not classified as
hidden merely because they do not contain the normal human appearance block.

The profile equipment uses the same slot, sprite, and dye-color names as local
equipment. Other-player packets do not provide item names or durability, so
those fields are not invented. `display_class` is separate from the base class;
for example, it can be `Summoner` for a Wizard.

Automatic requests do not open the game's other-player information pane. A
normal player click still opens it and also refreshes daRPC's cache. Leaving
view removes the player object, and the next `0x33` redraw starts a fresh
inspection.

Read one visible player's retained object and latest profile without sending a
packet to the game server:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/players/Eidolon"
```

The cached lookup is case-insensitive and returns the same `WorldObject` shape
as the objects collection. It can return `profile: null` while the automatic
inspection is pending. It searches only the current visible-object set; daRPC
does not retain a historical profile after that player leaves view.

Use a manual refresh for changes that do not redraw the player, such as a belt
or necklace change:

```console
curl -X POST "http://127.0.0.1:2626/clients/ZiLo/players/Eidolon/inspect"
```

Both player-name routes use the same case-insensitive visible-player lookup.
Missing names return `404` and ambiguous names return `409`. Only the refresh
route can return `504` when the game server does not respond.

The initial baseline walks the client's retained object collection. A creature
name or numeric sprite can be unavailable after a late attach when the client
no longer retains the original draw details. Pressing the normal client refresh
key clears the retained visible-object set before the server redraws nearby
objects. Numeric creature sprites learned from those draw packets are retained
through the follow-up snapshot.

## Object events

The complete object and visual payload structures are in
[World object events](events.md#world-object-events).

Players, monsters, and Mundanes publish the same core object actions. Players
also publish two events for identity and profile changes:

| Object | Events |
| --- | --- |
| Player | `player.appeared`, `player.replaced`, `player.inspected`, `player.disappeared`, `player.moved`, `player.direction_changed` |
| Monster | `monster.appeared`, `monster.disappeared`, `monster.moved`, `monster.direction_changed` |
| Mundane | `mundane.appeared`, `mundane.disappeared`, `mundane.moved`, `mundane.direction_changed` |

`player.replaced` is emitted instead of `player.appeared` when a newly drawn
player has the same name as one or more retained players but a different
object ID. Its payload contains every stale player snapshot in `previous` and
the authoritative replacement in `current`.

Ground items use:

```text
item.appeared
item.disappeared
item.moved
```

Each object event carries the complete public object after the change.
Disappearance carries the last retained object. `objects.cleared` marks a map
or world boundary, including an explicit client refresh, and carries no object.

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
