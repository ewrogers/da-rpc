# Character status

Character status is the best starting point for a dashboard, overlay, or
automation rule. It describes who is logged in, the current map, important
character values, and a few pieces of client-only action state.

| Use | Route or events |
| --- | --- |
| Read current status | `GET /clients/{client}/status` |
| Watch changes | [Status and walking events](events.md#character-status-events) |

## Reading status

```console
curl "http://127.0.0.1:2626/clients/ZiLo/status"
```

The response groups the data into a lifecycle, optional character, optional
map, and common observation metadata. The generated Swagger schema shows every
field and exact JSON type.

```text
Status {
    observation: ObservationMetadata,
    lifecycle: ClientLifecycle,
    character: Character?,
    map: MapLocation?,
}
```

The character data includes:

- Character ID, name, gender, class, hairstyle, hair color, and body sprite
- Nation, title, guild rank, display class, and guild from the latest self-look
- Level, ability level, experience, ability points, and progress toward the
  next level and ability level
- Strength, intelligence, wisdom, constitution, and dexterity
- Current and maximum health and mana
- Gold, weight, and maximum weight
- Armor class, damage, hit, magic resistance, attack element, and defense
  element
- `is_blinded`, `is_casting`, `is_walking`, and `is_action_restricted`
- The last server-confirmed `is_group_open` setting and current `group_members`

The map data includes its ID, available name, zero-based x/y position, width,
and height.

```text
Character {
    id: u32?,
    name: string?,
    gender: CharacterGender?,
    hair_style: u16?,
    hair_color: u8?,
    body_sprite: u16?,
    class: CharacterClass,
    identity: PlayerIdentity?,
    is_action_restricted: bool,
    is_blinded: bool,
    is_casting: bool,
    is_walking: bool,
    is_group_open: bool?,
    is_in_exchange: bool,
    group_members: Vec<GroupMember>,
    gold: u32,
    weight: u32,
    max_weight: u32,
    progression: CharacterProgression,
    stats: CharacterStats,
    vitals: CharacterVitals,
    modifiers: CharacterModifiers?,
}

MapLocation {
    id: u32,
    name: string?,
    x: i32?,
    y: i32?,
    width: i32,
    height: i32,
}
```

## Client lifecycle

The `lifecycle` field tells you where the client is, even when no character is
available yet:

| Value | Meaning |
| --- | --- |
| `unknown` | daRPC cannot confidently classify the current scene. |
| `title` | The client is at the title or login flow. |
| `transition` | The client is between stable game worlds. |
| `in_game` | A usable character and map are active. |
| `disconnected` | The reconnect dialog is visible. |

The reconnect dialog takes priority over the scene behind it. A disconnected
status may retain the last readable character and map, so use `lifecycle`
rather than the presence of `character` to decide whether the session is live.
The DLL refreshes lifecycle during client ticks. Consumers can also watch
`client.logged_in` and `client.disconnected`; see
[Client lifecycle events](events.md#client-lifecycle-events).

## Action flags

`is_walking` means the client's native pathfinder has an active queued route.
A single directional step does not set it.

`is_casting` means a delayed spell is in progress. Instant spells often begin
and finish between two REST reads, so the [spell events](spells.md#casting-events)
are the better record of those casts.

`is_action_restricted` represents a specific client restriction used by
movement, ground drops, incoming exchange start, and inventory rearrangement.
It does not mean that every action is blocked. Turning and ordinary skill or
spell activation can still be available.

`is_blinded` follows the blind state retained from the character's latest
status update.

`is_in_exchange` is true while daRPC retains an open player exchange. The full
offer is available from [`GET /exchange`](exchanges.md).

## Appearance limits

Gender, hairstyle, hair color, and body sprite come from the local character's
appearance record. They are unavailable together while the character is shown
through a monster-disguise image.

Readable names are used for gender, class, and elements. Raw client identifiers
and memory addresses are not exposed.

## Live status events

The complete payload structures and recovery route are in
[Character status events](events.md#character-status-events).

Listen on `GET /clients/{client}/events`. These events update status:

| Event | What changed |
| --- | --- |
| `stats.changed` | All five character attributes |
| `vitals.changed` | One or more health or mana values |
| `progression.changed` | Level, ability level, experience, or remaining progress |
| `gold.changed` | Carried gold |
| `weight.changed` | Current or maximum weight |
| `modifiers.changed` | Combat modifiers or elements |
| `blind.changed` | `is_blinded` |
| `action_restriction.changed` | `is_action_restricted` |
| `character.profile_changed` | Nation, title, guild rank, display class, or guild |
| `location.changed` | Absolute x/y and, when applicable, an atomic map change |
| `walking.started` | Native pathfinding began a queued route |
| `walking.stopped` | The queued route ended or was interrupted |
| `spell.begin` | A delayed cast began and `is_casting` became true |
| `spell.cast` | A cast completed and `is_casting` became false |
| `spell.cancelled` | A delayed cast ended without casting |

Status event values are absolute replacements, not amounts to add or subtract.
Several events can share one revision when one game update changed several
groups.

Lifecycle transitions emit `client.logged_in` when the title screen enters the
game and `client.disconnected` when the client returns to its disconnected
state. A closed process also closes its stream. Consumers should reread status
after reconnecting.

See [World](world.md) for map transitions and
[Movement and emotes](movement.md) for route details.
