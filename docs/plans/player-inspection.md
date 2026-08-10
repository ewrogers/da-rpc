# Player inspection plan

Status: planned, not implemented.

## Goal

Keep a current, queryable profile for every visible human player without opening
the game's other-player information pane. Refresh that profile whenever opcode
`0x33` redraws the player, allow an explicit refresh while the player remains
visible, and publish one deterministic event for each completed inspection.

This work also persists the local character identity fields already delivered
by self-look opcode `0x39`: nation, title, guild rank, display class, and guild.

## Confirmed client behavior

- Server opcode `0x33` draws one human and supplies position, direction, object
  ID, name, and appearance data. The client evicts objects that leave view and
  the server sends `0x33` again when they reenter.
- Client opcode `0x43`, subtype `1`, requests information for one living object.
  Its body is the opcode, subtype, and a big-endian `u32` object ID.
- Server opcode `0x34` is the paired player response. It starts with the same
  big-endian object ID and includes 18 equipment appearance records, user state,
  name, nation, title, group-open state, guild rank, display class, guild, legend
  marks, and an optional portrait and biography tail.
- UI suppression is required. Binary Ninja shows the `0x34` handler at
  `0x005F1590` resolving the living object and opening or refreshing exact RTTI
  `UserInfoPane_ForOthers`. It opens no pane only when the object is absent or is
  not living.
- The `0x34` equipment order is `1..12, 14, 13, 15..18`. This places Accessory 1
  before Boots. Sprite values must remain the exact wire `u16` values.

## Public model

Add one shared identity value used by local and remote profiles:

```text
PlayerIdentity {
    nation,
    title,
    guild_rank,
    display_class,
    guild,
}
```

`display_class` remains independent from the typed base character class. For
example, a character can have base class `wizard` and display class `Summoner`.

Add `Nation` with exact values:

```text
0 None       1 Suomi      2 Unknown1   3 Loures
4 Mileth     5 Tagor      6 Rucesion   7 Noes
8 Unknown2   9 Piet      10 Unknown3  11 Abel
12 Undine   13 Unknown4
```

Reject other raw values at the packet boundary instead of inventing a nation.

Add `PlayerEquipmentItem { slot, sprite, dye_color }`. It uses the same
`EquipmentSlot`, field names, numeric sprite treatment, and public JSON naming
as self equipment. It does not fabricate the self-only `name`, `durability`, or
`max_durability` fields that `0x34` cannot provide.

Add a remote profile containing:

```text
PlayerProfile {
    identity,
    user_state,
    is_group_open,
    equipment: Vec<PlayerEquipmentItem>,
    legend: Vec<LegendMark>,
    inspected_tick_ms,
}
```

Extend `WorldObject::Player` with `profile: Option<PlayerProfile>`. A fresh
`0x33` creates or replaces the visible player with `profile: None`; a successful
`0x34` fills it. Disappearance, object clear, and map change remove the object
and its profile together. This matches the client's visible-object lifetime,
avoids stale object IDs, bounds the cache by visible state, and guarantees that
reentry triggers a fresh inspection.

Add `identity: Option<PlayerIdentity>` to the local `CharacterSnapshot`. Keep
the existing local equipment and legend collections as their richer canonical
sources.

Do not retain the `0x34` portrait or biography in this feature. Parse and bound
the tail so malformed data is rejected, then discard it. This avoids storing
large or private profile content outside the requested scope.

## Request and suppression flow

1. After a valid `0x33` has updated the visible object state, enqueue that player
   for inspection. Repeated `0x33` packets request a new inspection, but a target
   already queued or in flight is deduplicated.
2. Use a bounded FIFO with one server request in flight. Manual requests receive
   priority over queued automatic work, but join an identical in-flight request.
3. On the client main-thread tick, revalidate that the object ID still identifies
   the same visible player, then submit `43 01 <u32be id>` through the ordinary
   client packet path.
4. Track outgoing subtype-1 requests as short-lived origins keyed by object ID.
   Mark daRPC submissions as internal and observed player submissions as user
   initiated. This prevents an automatic request from swallowing a player click
   aimed at the same target.
5. Arm the decoded-event pre-dispatch fast path only while an internal request is
   pending. For opcode `0x34`, read and match the object ID before doing the full
   bounded parse.
6. A matched internal response updates the profile, completes any waiting manual
   command, queues the state event, and skips the original client handler. An
   unmatched or user-origin response runs normally, opens the game UI, and is
   observed after dispatch so it also refreshes the cache.
7. Once the header identifies a response as belonging to daRPC, suppress it even
   if the detailed parse fails. Leave the cache unchanged, fail the waiting
   command, increment diagnostics, and continue the queue. This preserves the
   promise that daRPC's own inspection never opens UI. Never suppress a response
   that cannot be correlated to an internal origin.
8. Cancel queued work on object disappearance or world clear. Consume and
   suppress a late correlated response, but do not recreate a profile for an
   object that is no longer visible.

The existing Who interception is the implementation pattern, but player
inspection needs object-ID correlation and an origin queue because users and
daRPC can request the same target concurrently.

## State and event contract

Add one atomic model update for a completed visible-player inspection. It
contains the full current player profile plus a change set with these domains:

- `info`: identity, user state, or group-open state changed
- `equipment`: any slot appearance changed
- `legend`: any legend mark changed

Publish exactly one `player.inspected` Server-Sent Event for every successful
inspection, including an unchanged manual inspection. Its payload contains:

```text
observation
trigger: appeared | manual | user
player: complete current player object and profile
changes: [info | equipment | legend]
```

The first inspection reports all three change domains. An identical refresh has
an empty `changes` array. A single full payload prevents consumers from joining
several independently ordered partial events and makes completion observable
even when nothing changed. Consumers interested only in equipment or legend can
filter the change set.

For local opcode `0x39`, parse the packet once and feed the existing group and
legend trackers plus the new local identity state. Publish
`character.profile_changed` only when the persisted identity changes, carrying
the full current identity and its previous value. Keep existing legend events.

## API and direct command

- Extend player objects returned by `GET /clients/{client}/objects?kind=player`
  with the optional `profile`. `null` means the player is visible but inspection
  has not completed.
- Add `POST /clients/{client}/players/{player}/inspect`. Resolve `{player}` as a
  case-insensitive visible name, reject absent or ambiguous targets, submit or
  join the correlated inspection, and wait for the bounded result. Return the
  refreshed complete player object on success and `504` on response timeout.
- Add the matching typed direct protocol command so the daemon does not depend
  on raw packet access. Expose it through `darpc.exe` for parity and testing.
- Surface local identity under the character status DTO with the same field names
  used by remote `identity`.

## Implementation sequence

1. Add `Nation`, shared identity, player equipment appearance, player profile,
   snapshot fields, codecs, and round-trip/boundary tests.
2. Replace the duplicate partial self-look walkers with one bounded `0x39`
   decoder. Persist local identity while preserving group and legend behavior.
3. Add a bounded `0x34` decoder with golden fixtures for all fields, equipment
   order, optional tail, invalid lengths, invalid nation, and maximum counts.
4. Add visible-player profile application and change-domain calculation to the
   model, DLL state, full snapshot, daemon state, and protocol event stream.
5. Add the automatic queue, main-thread sender, `0x43` origin tracking, `0x34`
   pre-dispatch suppression, timeout handling, and diagnostics.
6. Add the manual typed command, REST route, CLI command, OpenAPI models, and
   `player.inspected` and `character.profile_changed` SSE projections.
7. Update the status, world, events, hooks, protocol, and CLI documentation.

## Verification

- Unit tests for every `Nation` value and invalid values.
- Golden and malformed tests for `0x39`, `0x43`, and `0x34`, including all 18
  equipment slots and the Accessory 1/Boots wire-order exception.
- State tests for first inspection, unchanged refresh, each change domain,
  `0x33` replacement, disappearance, map clear, late response, and object-ID
  reuse.
- Suppression tests proving internal responses do not call the original handler,
  user responses do, same-target origins remain ordered, malformed correlated
  responses stay suppressed, and expired origins fail open.
- Protocol round trips and compatibility tests for the appended snapshot fields
  and new event/command discriminants.
- Daemon API and SSE tests for pending `profile: null`, successful manual refresh,
  absent player, timeout, unchanged inspection, and deterministic full payloads.
- Host checks: `cargo fmt --all --check`, workspace tests, and Clippy with warnings
  denied where installed.
- Native Windows verification in the available Parallels guest: build both
  architectures, launch a controlled client, observe `0x33` automatic inspection,
  confirm no pane opens, confirm a real click still opens the pane, change a
  non-visible equipment slot if practical, manually inspect, and verify the cache
  and event change set refresh correctly.

