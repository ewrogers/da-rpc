# Game data

The game-data chapters are organized around the questions a player-facing tool
usually asks: who is logged in, what the character carries, what is nearby,
and what just happened. They document the stable public view exposed by the
daemon rather than internal client memory.

daRPC presents each game client as a set of familiar resources. Character
status, items, equipment, skills, spells, effects, nearby objects, messages,
dialogs, groups, and exchange state each have their own REST route and
documentation chapter.

This chapter explains the behavior they share. The individual chapters focus
on what a Dark Ages player or tool author can do with each kind of data.

## Finding the data you need

| Domain | Current state | Live changes | Action |
| --- | --- | --- | --- |
| [Character status](status.md) | `/status` | Stats, vitals, progression, gold, weight, modifiers, and flags | None |
| [Inventory](inventory.md) | `/items` | Items added, removed, or changed | None |
| [Equipment](equipment.md) | `/equipment` | No dedicated equipment event yet | None |
| [Skills](skills.md) | `/skills` | Skillbook changes and skill use | `/skills/use`, `/skills/swap` |
| [Spells](spells.md) | `/spells` | Spellbook changes and casting stages | `/spells/cast`, `/spells/swap` |
| [Effects](effects.md) | `/effects` | Effects added, changed, or removed | None |
| [World](world.md) | `/objects` and `/status` | Location, visible objects, and entity visuals | None |
| [Movement](movement.md) | `/status` | Walking and turning | `/turn`, `/walk`, and `/resync` |
| [Emotes](emotes.md) | None | Character emotes | `/emote` |
| [Messages](messages.md) | `/messages` | Chat and system messages by channel | `/messages/send` |
| [Groups](groups.md) | `/group` | Invitations, settings, and roster changes | Group actions |
| [Exchange](exchanges.md) | `/exchange` | Both offers and acceptance state | Offer, accept, and cancel actions |

All client routes begin with `/clients/{client}`. The `{client}` value may be a
process ID or the current character name. See [Choosing a client](web-api.md#choosing-a-client)
for the exact rules.

## Current state and live changes

REST answers the question, "What does this client know now?" Server-Sent Events
(SSE) answer the question, "What changed after I started listening?"

A common consumer flow is:

1. Open `/clients/{client}/events`.
2. Wait for `stream.ready`.
3. Read the REST resources needed by the tool.
4. Apply later SSE events in their delivered order.

The daemon subscribes to changes before it reads the ready boundary. This
prevents a gap between the current REST state and the live stream. SSE does not
replay events from before the subscription. If a stream reports that it fell
behind, read the REST resources again and reconnect.

The [Live events](events.md) chapter documents the common event envelope,
payloads, ordering, reconnect, and lag behavior. Each domain chapter explains
the events relevant to that data in game terms.

## How a client gets its first state

When the daemon connects, the DLL captures one complete baseline from the game
client. That baseline includes the current character, map, planned route,
session path-exclusion registry, collections, effects, and any world objects
still available in client memory.

The capture runs on a normal client tick because that is where the game changes
most of these structures. The DLL copies bounded values into memory it owns,
then lets its pipe worker convert and send them. It does not serialize JSON,
write logs, or perform named-pipe input/output on the game thread.

After the baseline, observed game events update the retained state. A REST read
uses the daemon's current copy and does not make the DLL walk game memory again.
A fresh complete baseline is taken when a new daemon connects or when daRPC
needs to recover from a missed update.

## Observation metadata

Snapshot-backed responses include an `observation` object. It identifies the
source process and helps consumers understand how fresh related resources are.

Important fields include:

- `pid` identifies the source game process.
- `revision` advances when retained state changes.
- `event_sequence` orders incremental changes.
- `captured_tick_ms` is the client tick of the last full baseline.
- `updated_tick_ms` advances when a later event changes the state.
- `capture_duration_us` records how long the baseline memory walk took.
- `world_generation` changes when the active game world is replaced.

```text
ObservationMetadata {
    pid: u32,
    revision: u32,
    event_sequence: u32,
    captured_tick_ms: u32,
    updated_tick_ms: u32,
    capture_duration_us: u32,
    world_generation: u32,
}
```

SSE event observations also carry `instance_id`, which identifies one loaded
DLL lifetime. See [Common observation metadata](events.md#common-observation-metadata).

Two separate REST requests can have different revisions if the client changes
between them. Read the revision when several resources must be compared as one
view.

## Missing and empty values

daRPC does not invent values when the client cannot provide them.

- `null` means that a value or collection was unavailable.
- An empty array means the collection was read successfully and had no entries.
- Optional fields remain absent or null until the client has supplied them.
- Empty inventory, equipment, spellbook, and skillbook slots are omitted.

A state route returns `404 Not Found` for an unknown client and `503 Service
Unavailable` when the client has not produced a usable observation. The latter
may include a capture failure reason.

The structures in this book use a small notation:

- `string`, `bool`, and integer names such as `u32` describe JSON value types.
- A trailing `?` means the value can be absent or `null`.
- `T[]` means an array of values shaped like `T`.
- Structures show JSON fields, not Rust or game-client memory layouts.

## Each client has its own view

Dark Ages characters share one game world, but each running client sees only
part of it. Nearby monsters and players may disappear from one client's view
while another client on the same map still sees them. The same is true for
messages and some local user-interface state.

For that reason, daRPC currently keeps state per client. It does not merge
several clients into one global world model or guess which observation is the
newest. A future aggregator can build that shared view while retaining the
source client and observation time.

## Threading in plain language

The client main thread owns most game state. daRPC copies state and runs native
actions there so it does not race the client from an unrelated thread. The
hook paths remain short and use fixed buffers or bounded queues. Parsing,
serialization, web requests, and named-pipe work happen elsewhere.

See [Runtime hooks](hooks.md) for the installed hooks, their purpose, and the
attach and detach lifecycle.
