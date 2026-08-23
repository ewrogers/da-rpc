# Live events

The daemon exposes a Server-Sent Events (SSE) stream for each connected game
client. Use it when a tool needs to react as the character, inventory, nearby
world, or messages change.

REST and SSE work together:

- REST gives you the latest complete view of one part of the client.
- SSE tells you what changed after you started listening.

This chapter explains how to subscribe, recover from interruptions, and decode
every event currently published by daRPC. The domain chapters explain what the
same data means in the game.

## Subscribe to a client

```console
curl --no-buffer --header "Accept: text/event-stream" \
  "http://127.0.0.1:2626/clients/ZiLo/events"
```

`client` can be a process ID or the current character name, matched without
regard to case. A character name works only while that client is in game. A
process ID remains the reliable identifier at the title screen or after a
disconnect.

The client must be connected to `darpcd.exe` and have current state. Otherwise,
the endpoint returns the normal JSON API error instead of opening a stream.

Swagger describes the JSON event models, but its Try it out interface is not an
SSE viewer. The `curl` command above displays the live stream directly. Browser
applications can use `EventSource`, and other consumers need an SSE-capable
HTTP library.

## Read an SSE frame

Every published frame contains an SSE sequence ID, a routing name, and a JSON
body:

```text
id: 38
event: vitals.changed
data: {"type":"vitals_changed","data":{"observation":{...},"health":4200,"max_health":5000,"mana":1800,"max_mana":2000}}
```

The two names serve different purposes:

- The SSE `event` name uses `domain.action`, such as `vitals.changed`.
- The JSON `type` discriminator uses `snake_case`, such as `vitals_changed`.

Use the SSE name with `addEventListener`. Use the JSON discriminator when one
handler decodes several event types. Do not derive one name mechanically from
the other. Most correspond naturally, but `character.turned` uses `turned`,
`character.emoted` uses `emoted`, and every message channel uses `message`.

The endpoint sends every event for the selected client. It does not currently
accept server-side event filters. One connection is normally enough: register
listeners only for the SSE names your tool uses. Open one stream per game client
when a tool follows several clients.

An SSE ID marks stream order, not a globally unique record. One observed client
update can produce several frames with the same ID. Process frames in delivery
order and do not discard a frame only because its ID matches the previous one.

## Start with a current baseline

The first frame is always `stream.ready`:

```text
id: 38
event: stream.ready
data: {"type":"stream_ready","data":{"pid":6076,"instance_id":"890b3755fccd8d45b165bed41165457a","revision":42,"event_sequence":38}}
```

After receiving it:

1. Read the REST resources your tool needs.
2. Apply later SSE frames in delivery order.

The daemon starts listening for client changes before it creates the ready
boundary. A change therefore cannot slip between the boundary and the REST
reads. The REST responses can already include a newer revision, which is safe
because later events contain absolute replacement values.

```text
StreamReady {
    pid: u32,
    instance_id: string,
    revision: u32,
    event_sequence: u32,
}
```

## Common observation metadata

Most events carry the observation that produced them:

```text
EventObservation {
    pid: u32,
    instance_id: string,
    revision: u32,
    event_sequence: u32,
    tick_ms: u32,
}
```

- `pid` identifies the game process.
- `instance_id` identifies this load of `darpc.dll`. It changes after unload and
  reinjection.
- `revision` identifies the retained client-state revision.
- `event_sequence` orders updates produced by the DLL.
- `tick_ms` is the client's wrapping Windows millisecond tick.

Message events use their normalized message record instead of an
`observation`. Their SSE ID still supplies stream order, and the subscription
path identifies the client. Internal message IDs use `internal-N`; they do not
advance or interrupt the DLL state-event sequence.

A field followed by `?` in the structures below is optional and can be absent
or `null`. Collection types use `T[]`. The examples describe JSON values rather
than Rust memory layouts.

### Shared values

```text
TilePosition { x: i32, y: i32 }
Direction = north | east | south | west
Element = none | fire | water | wind | earth | light | dark | wood | metal | undead | unknown
EffectDuration = white | red | orange | yellow | green | blue
```

Tile coordinates are zero-based. Effect durations are the client's visible
color bands from longest to shortest, not exact timers.

## Client command events

Public speech beginning with one slash is a local command. The DLL suppresses
the chat submission and publishes `client.command` with the JSON discriminator
`client_command`:

```text
ClientCommand {
    observation: EventObservation,
    command: string,
    args: string[],
}
```

The command is the nonempty text between `/` and the first whitespace. The
remaining text is split on commas; surrounding whitespace and empty entries are
removed. For example, `/walk x, , y` publishes `command: "walk"` and
`args: ["x", "y"]`. Commands are transient and have no REST recovery route.

The outgoing refresh packet publishes `client.resync` with the JSON
discriminator `client_resync`:

```text
ClientResync {
    observation: EventObservation,
    resync_id: u32,
}
```

Closing the refresh window publishes `client.resync_completed` with the JSON
discriminator `client_resync_completed`:

```text
ClientResyncCompleted {
    observation: EventObservation,
    resync_id: u32,
}
```

Both events are transient. The identifier correlates the completion with the
packet submission and HTTP request. `client.resync_completed` is published
after `RefreshUserOK` or the one-second fallback closes reconciliation. Object
lifecycle updates caused by the refresh appear before it. See
[Refresh and resynchronization](resync.md) for coalescing, movement safety,
fallback behavior, and the consumer sequence.

Begin speech with `//` to escape interception. The DLL removes one slash before
submission, so `//walk x,y` is spoken as `/walk x,y` and does not publish a
command event. Other speech and chat channels continue normally.

## Client lifecycle events

The DLL checks the client lifecycle on the game thread and emits semantic
events when it enters the two states most useful to automation:

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `client.logged_in` | `client_logged_in` | The client entered the in-game state. |
| `client.disconnected` | `client_disconnected` | The client displayed its reconnect dialog. |

```text
ClientLifecycleChanged {
    observation: EventObservation,
    previous: unknown | title | transition | in_game | disconnected,
    current: unknown | title | transition | in_game | disconnected,
}
```

`client.disconnected` describes the game client's server connection. It is not
the same as `stream.closed`, which means the daemon lost its DLL connection.
Transitions to other lifecycle states update the REST snapshot without
publishing another lifecycle event.

## Reconnect and recover

Events are ordered within one client stream. daRPC does not replay state events
created before a subscription. It also does not resume from the browser's
`Last-Event-ID` header.

The daemon keeps a bounded 4,096-entry broadcast queue. If a subscriber falls
behind, or if the daemon cannot reduce a client event batch into its retained
observation, it publishes this final event and closes the connection:

```text
stream.resync_required

StreamResyncRequired {
    pid: u32,
    instance_id: string,
    last_event_sequence: u32,
    dropped_events: u64,
}
```

If the DLL connection ends, the daemon publishes this final event and closes
the stream:

```text
stream.closed

StreamClosed {
    pid: u32,
    instance_id: string,
    last_event_sequence: u32,
    reason: string,
}
```

For either event, discard assumptions based only on the old stream. Wait for
the client observation to become available, or for the client to reconnect if
necessary, then open a new stream, wait for `stream.ready`, and reread the REST
resources your tool uses. A reduction failure temporarily returns `503 Service
Unavailable` from observation-backed REST routes and new stream requests while
the daemon obtains a fresh snapshot. Message history has its own bounded
lookback and can restore recent conversation context.

Fifteen-second SSE comments keep an idle connection observable. They do not
change ordering or carry game data.

## Collection batches

Inventory, skill, and spell updates can change several slots as one client
operation. These events use the same shape:

```text
SlotChanged<T> {
    observation: EventObservation,
    batch_index: u16,
    batch_count: u16,
    slot: u8,
    before: T?,
    after: T?,
}
```

`batch_index` starts at zero. Every frame in the same update shares
`batch_count`, revision, and event sequence. The daemon applies the whole batch
to REST state before publishing the first frame.

A detailed consumer can wait for every batch entry. A simpler consumer can
reread the matching REST route after receiving any event in that domain.

## Character status events

Read the current values from `GET /clients/{client}/status`. See
[Character status](status.md) for field meaning and lifecycle behavior.

The `character.*` namespace describes the local character. Nearby players use
`player.*` events in the [world object](#world-object-events) domain. In
particular, the local Hide transition is `character.hidden_changed`; there is
no `player.hidden_changed` event.

| SSE event | JSON type | Payload after `observation` |
| --- | --- | --- |
| `stats.changed` | `stats_changed` | `stat_points`, `strength`, `intelligence`, `wisdom`, `constitution`, `dexterity` |
| `vitals.changed` | `vitals_changed` | Nullable `health`, `max_health`, `mana`, `max_mana` |
| `progression.changed` | `progression_changed` | Nullable level, ability, and experience fields |
| `gold.changed` | `gold_changed` | `gold` |
| `weight.changed` | `weight_changed` | `weight`, `max_weight` |
| `modifiers.changed` | `modifiers_changed` | Armor class, damage, hit, magic resistance, and elements |
| `location.changed` | `location_changed` | `x`, `y`, and optional changed map details |
| `blind.changed` | `blind_changed` | `is_blinded` |
| `action_restriction.changed` | `action_restriction_changed` | `is_action_restricted` |
| `character.appearance_changed` | `character_appearance_changed` | Optional complete `previous` and `current` appearances |
| `character.hidden_changed` | `character_hidden_changed` | Boolean `previous` and `current` hidden state |

```text
StatsChanged {
    observation: EventObservation,
    stat_points: u8,
    strength: u16,
    intelligence: u16,
    wisdom: u16,
    constitution: u16,
    dexterity: u16,
}

VitalsChanged {
    observation: EventObservation,
    health: u32?,
    max_health: u32?,
    mana: u32?,
    max_mana: u32?,
}

ProgressionChanged {
    observation: EventObservation,
    level: u8?,
    ability_level: u8?,
    experience: u32?,
    ability_points: u32?,
    experience_to_next_level: u32?,
    ability_to_next_level: u32?,
}

ModifiersChanged {
    observation: EventObservation,
    armor_class: i8,
    damage: u8,
    hit: u8,
    magic_resistance: u16,
    attack_element: Element,
    defense_element: Element,
}

LocationChanged {
    observation: EventObservation,
    x: i32,
    y: i32,
    map: MapChanged?,
}

MapChanged {
    id: u32,
    name: string?,
    width: i32,
    height: i32,
}

CharacterAppearanceChanged {
    observation: EventObservation,
    previous: CharacterAppearance?,
    current: CharacterAppearance?,
}

CharacterHiddenChanged {
    observation: EventObservation,
    previous: bool,
    current: bool,
}
```

Appearance and hidden changes are emitted after an authoritative snapshot
recapture, including the `0x33` self redraw used when Hide begins or ends.

### Hide and monster-form transitions

Hide and monster form are separate, mutually exclusive states. Consumers
should use `is_hidden`, not nullable appearance fields, to detect Hide.

| Local transition | SSE event | `/status` result |
| --- | --- | --- |
| Visible human to hidden | `character.hidden_changed` with `false` to `true` | `is_hidden: true`; the last complete gender, hair, and body fields remain available |
| Hidden to visible human | `character.hidden_changed` with `true` to `false` | `is_hidden: false`; gender, hair, and body contain the current visible appearance |
| Human to monster form | `character.appearance_changed` with a complete human `previous` and `current: null` | `gender`, `hair_style`, `hair_color`, and `body_sprite` are null; `is_hidden` remains false |
| Monster form to human | `character.appearance_changed` with `previous: null` and a complete human `current` | The four human appearance fields repopulate; `is_hidden` remains false |

A monster-form transition does not publish `character.hidden_changed`. The
local character model currently represents the normal human appearance but
does not expose the temporary monster sprite identifier.

For another visible player, a hidden `0x33` redraw updates the player's
`is_hidden` field and is published through the normal `player.appeared` object
event. The object retains the last known name and inspected profile by entity
ID. Its `visual` block distinguishes `human` from `creature`; transformed-player
events include the creature sprite and transmitted color bytes. Consumers
should use that discriminator rather than infer a remote monster form from
`is_hidden` or missing profile data.

`map` is present when the map itself changed. Position and map details are
published together so consumers never observe coordinates from one map paired
with another map.

## Map download events

Map download events describe the native game client's cache-miss transfer, not
the daemon's `GET /maps/{map_id}/download` route.

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `map.requested` | `map_requested` | The native client submitted a matching `0x05` map request after cache validation failed. |
| `map.downloaded` | `map_downloaded` | The native client returned from handling the final `0x3C` row after daRPC observed every prepared row. |

Both payloads contain `observation`, `map_id`, `width`, and `height`. The
completion event is published after the client's native final-row path has
recomputed the map checksum, attempted the complete cache-file write, closed
the loading pane, and applied the prepared map. The client does not report the
cache writer's result, so `map.downloaded` confirms transfer completion and the
native write attempt, not durable filesystem persistence.

These events are transient and have no REST recovery route. A fresh daemon
connection resumes from the DLL's current ordered event boundary; it does not
invent a request for a cache hit or for a transfer that began before injection.

## Walking and character action events

Read the current flags and position from `GET /clients/{client}/status`. See
[Movement](movement.md) and [Emotes](emotes.md) for the action routes.

| SSE event | JSON type |
| --- | --- |
| `walking.started` | `walking_started` |
| `walking.stopped` | `walking_stopped` |
| `walking.obstructed` | `walking_obstructed` |
| `walking.route_changed` | `walking_route_changed` |
| `character.turned` | `turned` |
| `character.emoted` | `emoted` |

```text
walking.started {
    observation: EventObservation,
    current: TilePosition,
    destination: TilePosition?,
}

walking.stopped {
    observation: EventObservation,
    current: TilePosition,
    destination: TilePosition?,
    reached_destination: bool?,
    reason: completed | obstructed | replaced | cancelled | position_corrected,
}

walking.obstructed {
    observation: EventObservation,
    map_id: u32,
    current: TilePosition,
    attempted: TilePosition,
    direction: Direction,
    destination: TilePosition?,
    mode: direct | native_route | exact_route | pursuit,
}

walking.route_changed {
    observation: EventObservation,
    generation: u32,
    tiles: Vec<TilePosition>,
}

character.turned {
    observation: EventObservation,
    direction: Direction,
}

character.emoted {
    observation: EventObservation,
    code: u8,
}
```

`destination` is available for pathfinding but can be absent for a single
step. `reached_destination` is known only when there was a retained destination.
The stop reason distinguishes normal completion, obstruction, replacement,
explicit cancellation, and a server position correction. An obstruction
reports the rejected edge first. daRPC does not replan; a later rejected edge
resets the executing exact route and leaves recovery to the consumer. A
replacement rejected during preflight leaves the existing route intact and
emits no replacement, route-change, or obstruction event.
Action events mean the request reached the client's normal action boundary.
They do not promise that the server accepted the result.

## Inventory and equipment events

Read inventory from `GET /clients/{client}/items` and equipment from
`GET /clients/{client}/equipment`. See [Inventory](inventory.md) and
[Equipment](equipment.md).

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `item.added` | `item_added` | A slot gained an item or stack quantity. |
| `item.removed` | `item_removed` | A slot lost an item or stack quantity. |
| `item.changed` | `item_changed` | An item moved, swapped, split, merged, or changed details. |
| `item.used` | `item_used` | The client submitted item use. |
| `item.dropped` | `item_dropped` | The client submitted an item to a ground tile, including `/items/drop`. |
| `item.given` | `item_given` | The client submitted an item to an entity, including `/items/give`. |
| `item.picked_up` | `item_picked_up` | The client submitted a ground-item pickup. |
| `gold.dropped` | `gold_dropped` | The client submitted gold to a ground tile, including `/gold/drop`. |
| `gold.given` | `gold_given` | The client submitted gold to an entity, including `/gold/give`. |
| `equipment.unequipped` | `equipment_unequipped` | The client submitted an unequip request. |

`item.added`, `item.removed`, and `item.changed` use
`SlotChanged<InventoryItem>`. Action payloads are:

```text
ItemUsed { observation, slot }
ItemDropped { observation, slot, quantity, destination }
ItemGiven { observation, slot, quantity, target_id }
ItemPickedUp { observation, destination_slot, position }
GoldDropped { observation, amount, destination }
GoldGiven { observation, amount, target_id }
EquipmentUnequipped { observation, slot }
```

Giving an item opens the game's normal exchange flow. It does not mean the
other character accepted the exchange. Later inventory or gold events confirm
changes accepted by the server.

## Skill events

Read the skillbook from `GET /clients/{client}/skills`. See [Skills](skills.md).

| SSE event | JSON type | Payload |
| --- | --- | --- |
| `skill.added` | `skill_added` | `SlotChanged<Skill>` |
| `skill.removed` | `skill_removed` | `SlotChanged<Skill>` |
| `skill.changed` | `skill_changed` | `SlotChanged<Skill>` |
| `skill.cooldown` | `skill_cooldown` | A retained skill entered or restarted cooldown. |
| `skill.ready` | `skill_ready` | `AbilityReady` |
| `skill.used` | `skill_used` | `observation`, `slot`, optional `name` |

`skill.used` records an observed submission through the client's normal skill
path. `skill.cooldown` confirms that the retained skill entered cooldown, while
`skill.ready` confirms that it left cooldown. `skill.changed` is reserved for
changes to the retained skill itself, such as its name, icon, or level. A single
client update can emit both `skill.changed` and `skill.cooldown` when both kinds
of state changed together.

## Spell events

Read the spellbook from `GET /clients/{client}/spells`. See [Spells](spells.md)
for targeting, chanting, replacement, and feedback matching.

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `spell.added` | `spell_added` | A spellbook slot gained a spell. |
| `spell.removed` | `spell_removed` | A spellbook slot became empty. |
| `spell.changed` | `spell_changed` | A retained spell's own details changed. |
| `spell.cooldown` | `spell_cooldown` | A retained spell entered or restarted cooldown. |
| `spell.ready` | `spell_ready` | A retained spell left cooldown. |
| `spell.begin` | `spell_begin` | A delayed spell began. |
| `spell.chant` | `spell_chant` | One chant line was submitted. |
| `spell.cast` | `spell_cast` | The final spell use was submitted. |
| `spell.cancelled` | `spell_cancelled` | A delayed spell ended without a final cast. |
| `spell.succeeded` | `spell_succeeded` | System feedback confirmed a recent submission. |
| `spell.failed` | `spell_failed` | System feedback rejected or resisted a recent submission. |
| `spell.received` | `spell_received` | Another entity cast or attacked with a spell on this character. |

The spellbook events use `SlotChanged<Spell>`. Cooldown and cast activity use:

```text
CooldownStarted { observation, slot, name?, cooldown_ms?, remaining_ms? }
AbilityReady { observation, slot, name? }
SpellBegin { observation, slot, name?, total_lines }
SpellChant { observation, slot, name?, line, total_lines }
SpellCast { observation, slot, name?, arguments? }
SpellCancelled { observation, slot, name?, source }

SpellSucceeded {
    observation,
    slot,
    name?,
    arguments?,
    feedback,
    submitted_tick_ms,
    elapsed_ms,
}

SpellFailed {
    observation,
    slot,
    name?,
    arguments?,
    reason,
    active_spell?,
    feedback,
    submitted_tick_ms,
    elapsed_ms,
}

SpellReceived {
    observation,
    caster,
    caster_object?,
    name,
    kind,
    feedback,
}
```

Skill cooldown events use the same `CooldownStarted` and `AbilityReady`
payloads. `cooldown_ms` is the stable total duration from the live action-delay
packet and `remaining_ms` is the time left at observation. Skills also retain
start and end timestamps in client memory, which restores exact timing after a
late attach. Spells do not, so both fields can be absent after a late attach
even while the active cooldown and eventual ready transition remain observable.
When both are present, `remaining_ms` does not exceed `cooldown_ms`. A consumer
can calculate progress from both values but should treat `*.ready` as the
authoritative completion signal rather than relying only on a local countdown.

`SpellCastArguments` is tagged by its own `type` field:

```text
SpellCastArguments =
    { type: "unknown" }
  | { type: "target", id: u32?, name: string?, x: i32, y: i32 }
  | { type: "input", value: string }
  | { type: "values", values: u16[] }
```

Cancellation `source` is `client`, `server`, or `replaced`. Failure `reason` is
`failed`, `error`, `resisted`, `already_active`, or `conflicting_effect`.
Received spell `kind` is `cast` for friendly wording or `attack` for harmful
wording.

An instant spell normally emits only `spell.cast`. A delayed spell normally
emits `spell.begin`, one or more `spell.chant` events, and `spell.cast`.
`spell.succeeded` and `spell.failed` are later interpretations of system text,
not replacements for `spell.cast`.

## Persistent effect events

Read active status effects from `GET /clients/{client}/effects`. See
[Effects](effects.md).

| SSE event | JSON type |
| --- | --- |
| `effect.added` | `effect_added` |
| `effect.removed` | `effect_removed` |
| `effect.changed` | `effect_changed` |

```text
effect.added { observation, icon, duration }
effect.removed { observation, icon }
effect.changed { observation, icon, duration }
```

These events describe the persistent effect icons shown by the client. They are
different from the temporary `player.effect`, `monster.effect`, and
`mundane.effect` visuals described below.

## World object events

Read the currently retained view from `GET /clients/{client}/objects`. See
[World](world.md) for object fields, view-range behavior, and map
boundaries.

Players, monsters, and Mundanes publish these object actions:

```text
player.appeared             monster.appeared             mundane.appeared
player.replaced
player.disappeared          monster.disappeared          mundane.disappeared
player.moved                monster.moved                mundane.moved
player.direction_changed    monster.direction_changed    mundane.direction_changed
player.inspected
```

`player.replaced` is published instead of `player.appeared` when a newly shown
player has the same name as one or more retained players but a different object
ID. Its payload contains every stale snapshot and the authoritative replacement:

```text
PlayerReplaced {
    observation: EventObservation,
    previous: WorldObject[],
    current: WorldObject,
}
```

Ground items publish:

```text
item.appeared
item.disappeared
item.moved
```

Their JSON discriminator replaces the dot with an underscore, such as
`player_appeared`, `mundane_direction_changed`, or `item_moved`.

The remaining object events use this payload:

```text
ObjectChanged {
    observation: EventObservation,
    object: WorldObject,
}
```

`WorldObject` is tagged by `kind`:

```text
WorldObject =
    Player { kind: "player", id, name?, x, y, direction, profile? }
  | Monster { kind: "monster", id, sprite?, x, y, direction }
  | Mundane { kind: "mundane", id, sprite?, name?, x, y, direction }
  | Item { kind: "item", id, sprite, dye_color, x, y, z_index }
```

Appeared, moved, and direction-changed events carry the object after the
update. A disappeared event carries the last retained object. Refreshes and map
changes publish these same per-object lifecycle events instead of a
collection-wide clear event.

Reduce the stream into retained object state as follows:

- Upsert `object` by ID for appeared, moved, and direction-changed events.
- Remove `object.id` for disappeared events.
- For `player.replaced`, remove every `previous` ID and upsert `current`.
- For `player.inspected`, upsert `player` when retaining profile data.
- Do nothing to the object collection for `client.resync` or
  `client.resync_completed`.
- On a map change, apply `location.changed` first and then the following
  disappearance events in delivery order. Do not clear objects when the
  location event arrives.

An appeared event can replace an already retained ID when a redraw supplies
changed fields. These reducer rules therefore remain idempotent across ordinary
draws, F5 reconciliation, and map changes.

`player.inspected` is one atomic completion event:

```text
PlayerInspected {
    observation: EventObservation,
    trigger: "appeared" | "manual" | "user",
    player: WorldObject,
    changes: ("info" | "equipment" | "legend")[],
}
```

The player contains the complete current profile. The first inspection lists
all three change domains. An identical refresh has an empty `changes` array,
which makes manual completion observable without several independently ordered
partial events. `character.profile_changed` similarly carries the previous
optional local identity and complete current identity from self-look.

## Entity visual events

Visible players, monsters, and Mundanes can publish animation, visual effect,
and damage feedback:

```text
player.animated    monster.animated    mundane.animated
player.effect      monster.effect      mundane.effect
player.damaged     monster.damaged     mundane.damaged
```

Their JSON discriminators follow the same underscore form, such as
`player_animated`, `monster_effect`, or `mundane_damaged`.

```text
EntityAnimated {
    observation,
    entity: WorldObject,
    animation: u8,
    initial_duration_ms: i32,
}

EntityEffect {
    observation,
    entity: WorldObject,
    effect: u16,
    source: WorldObject?,
    frame_interval_ms: i16?,
}

EntityDamaged {
    observation,
    entity: WorldObject,
    health_percent: u8,
}
```

`health_percent` is the server's 0 through 100 value for the temporary health
meter. It is not the amount of damage dealt. Effects drawn only at ground
coordinates are not published yet.

## Audio events

Audio packets provide useful automation cues even when they do not change
visible state.

| SSE event | JSON type | Payload after `observation` |
| --- | --- | --- |
| `sound.played` | `sound_played` | `effect: u8` |
| `music.started` | `music_started` | `track: u8` |
| `music.stopped` | `music_stopped` | No additional fields |

The numeric values are the effect and music identifiers sent by the server.
These transient events have no REST recovery route and are not replayed.

## Message events

Read recent retained messages from `GET /clients/{client}/messages`. See
[Messages](messages.md) for channel parsing, filtering, paging, retention, and
privacy.

| SSE event | Channel |
| --- | --- |
| `message.say` | Nearby speech |
| `message.shout` | Nearby shout |
| `message.chant` | Spell chant or mock chant used for an NPC interaction |
| `message.whisper` | Incoming or outgoing whisper |
| `message.guild` | Guild chat |
| `message.group` | Group chat |
| `message.system` | Client or server system text |
| `message.world` | World shout |
| `message.internal` | Daemon-only inter-client payload |

All message routes use the JSON discriminator `type: "message"`. The channel
inside the payload distinguishes them:

```text
Message {
    timestamp: string,
    tick_ms: u32?,
    channel: MessageChannel,
    sender: string?,
    recipient: string?,
    text: string?,
    payload: object?,
}
```

The SSE ID carries message stream ordering; it is not repeated in the JSON
message. The daemon stores a normalized message before broadcasting it, except
for `message.chant`, which is intentionally transient. Some system messages
also produce `spell.succeeded`, `spell.failed`, or `spell.received`.
Both frames are intentional: one preserves the text shown by the game and the
other supplies semantic spell data.

Internal messages contain `payload` and omit `tick_ms` and `text`. They are
published by `POST /messages/send` only to the selected recipient stream, or to
every connected client stream for a broadcast. They never enter the game or
DLL transport. See [Messages](messages.md#internal-messages).

## NPC dialog events

Read the current page from `GET /clients/{client}/dialog`. The
[NPC dialogs](dialogs.md) chapter documents the dialog model, revision checks,
response actions, and complete event payloads.

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `dialog.opened` | `dialog_opened` | A merchant or pursuit dialog became active. |
| `dialog.changed` | `dialog_changed` | The server replaced the current page or response choices. |
| `dialog.submitted` | `dialog_submitted` | A daRPC action answered or navigated the current page. |
| `dialog.closed` | `dialog_closed` | The dialog ended locally, remotely, during a map change, or during recovery. |

## Message-dialog events

Read the current set from `GET /clients/{client}/message-dialogs`. The
[Message dialogs](message-dialogs.md) chapter documents capture bounds,
revision checks, and dismissal.

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `message_dialogs.changed` | `message_dialogs_changed` | The complete set of native message dialogs opened, changed, or closed. |

## Field-map events

Read current state from `GET /clients/{client}/field-map`. The
[Field maps](field-maps.md) chapter documents pane detection, destinations,
revision checks, selection, and complete payload fields.

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `field_map.opened` | `field_map_opened` | A validated native field-map panel became active. |
| `field_map.changed` | `field_map_changed` | The active field map and destination list were replaced. |
| `field_map.selection_submitted` | `field_map_selection_submitted` | The client sent the retained destination's canonical selection packet. |
| `field_map.closed` | `field_map_closed` | The native panel was no longer registered and visible. |

Opened, changed, and selection-submitted events contain `field_map` with the
complete `FieldMapState`. Closed contains the complete state as `previous`.
There is no selection-started event: a native click can animate before sending,
and only the observed outgoing packet is authoritative for submission.

## Group events

Read current membership and invitations from `GET /clients/{client}/group`.
The [Groups](groups.md) chapter explains invitation actions, the group-open
toggle, and server confirmation.

```text
group.settings_changed
GroupSettingsChanged {
    observation: EventObservation,
    group: GroupState,
}

group.invitation_sent
GroupInvitationSent {
    observation: EventObservation,
    target: string,
}

group.invitation_received
GroupInvitationReceived {
    observation: EventObservation,
    invitation: GroupInvitation,
    group: GroupState,
}

group.invitation_closed
GroupInvitationClosed {
    observation: EventObservation,
    invitation: GroupInvitation,
    reason: GroupInvitationCloseReason,
    group: GroupState,
}

group.joined
GroupJoined {
    observation: EventObservation,
    group: GroupState,
}

group.member_joined | group.member_left
GroupMemberChanged {
    observation: EventObservation,
    member: GroupMember,
    group: GroupState,
}

group.disbanded
GroupDisbanded {
    observation: EventObservation,
    group: GroupState,
}
```

State-bearing events include the complete resulting group. Replace the
consumer's retained value with that `group` instead of applying an inferred
partial change. `group.invitation_sent` only confirms local submission because
the game does not send a direct response when the other player declines.

## Player exchange events

Read the current offer from `GET /clients/{client}/exchange`. The
[Exchange](exchanges.md) chapter explains initiation, quantity handling,
one-time gold, acceptance, and cancellation.

```text
exchange.opened
ExchangeOpened {
    observation: EventObservation,
    exchange: ExchangeState,
}

exchange.item_added
ExchangeItemAdded {
    observation: EventObservation,
    party: ExchangeParty,
    item: ExchangeItem,
    exchange: ExchangeState,
}

exchange.gold_changed
ExchangeGoldChanged {
    observation: EventObservation,
    party: ExchangeParty,
    gold: u32,
    exchange: ExchangeState,
}

exchange.accepted
ExchangeAccepted {
    observation: EventObservation,
    party: ExchangeParty,
    message: string,
    exchange: ExchangeState,
}

exchange.completed | exchange.cancelled
ExchangeFinished {
    observation: EventObservation,
    message: string,
    exchange: ExchangeState,
}
```

`party` is `local` or `other`. Each event includes the complete offer state at
that point. Replace a consumer's retained value with `exchange` instead of
trying to infer state from only the changed field.

## Complete event index

| Domain | Events | REST recovery route |
| --- | --- | --- |
| Stream | `stream.ready`, `stream.resync_required`, `stream.closed` | Reread every resource the consumer uses. |
| Client lifecycle | `client.logged_in`, `client.disconnected` | `/status` |
| Client requests | `client.command`, `client.resync`, `client.resync_completed` | None; transient events are not replayed. |
| Status | `stats.changed`, `vitals.changed`, `progression.changed`, `gold.changed`, `weight.changed`, `modifiers.changed`, `location.changed`, `blind.changed`, `action_restriction.changed`, `character.appearance_changed`, `character.hidden_changed`, `character.profile_changed` | `/status` |
| Map downloads | `map.requested`, `map.downloaded` | None; transient events are not replayed. |
| Walking | `walking.started`, `walking.stopped`, `walking.obstructed`, `walking.route_changed`, `character.turned`, `character.emoted` | `/status` |
| Inventory | `item.added`, `item.removed`, `item.changed`, `item.used`, `item.dropped`, `item.given`, `item.picked_up`, `gold.dropped`, `gold.given` | `/items`, then `/status` for gold |
| Equipment | `equipment.unequipped` | `/equipment` |
| Skills | `skill.added`, `skill.removed`, `skill.changed`, `skill.cooldown`, `skill.ready`, `skill.used` | `/skills` |
| Spells | `spell.added`, `spell.removed`, `spell.changed`, `spell.cooldown`, `spell.ready`, `spell.begin`, `spell.chant`, `spell.cast`, `spell.cancelled`, `spell.succeeded`, `spell.failed`, `spell.received` | `/spells`, then `/status` for casting state |
| Effects | `effect.added`, `effect.removed`, `effect.changed` | `/effects` |
| World objects | `player.appeared`, `player.replaced`, `player.inspected`, `player.disappeared`, `player.moved`, `player.direction_changed`; the corresponding appeared, disappeared, moved, and direction events for monsters and Mundanes; `item.appeared`, `item.disappeared`, `item.moved` | `/objects` |
| World visuals | `player.animated`, `player.effect`, `player.damaged`, and the corresponding monster and Mundane events | None; transient events are not replayed. |
| Audio | `sound.played`, `music.started`, `music.stopped` | None; transient events are not replayed. |
| Messages | `message.say`, `message.shout`, `message.chant`, `message.whisper`, `message.guild`, `message.group`, `message.system`, `message.world`, `message.internal` | `/messages`, except transient chants |
| NPC dialogs | `dialog.opened`, `dialog.changed`, `dialog.submitted`, `dialog.closed` | `/dialog` |
| Message dialogs | `message_dialogs.changed` | `/message-dialogs` |
| Field maps | `field_map.opened`, `field_map.changed`, `field_map.selection_submitted`, `field_map.closed` | `/field-map` |
| Groups | `group.settings_changed`, `group.invitation_sent`, `group.invitation_received`, `group.invitation_closed`, `group.joined`, `group.member_joined`, `group.member_left`, `group.disbanded` | `/group`, then `/status` for convenience fields |
| Exchange | `exchange.opened`, `exchange.item_added`, `exchange.gold_changed`, `exchange.accepted`, `exchange.completed`, `exchange.cancelled` | `/exchange`, then `/status` for `is_in_exchange` |
| Legend | `legend.mark_added`, `legend.mark_changed`, `legend.mark_removed` | `/legend` |

The OpenAPI document at `/openapi.json` remains the exact machine-readable
schema for these payloads. This chapter is the human-readable reference.
