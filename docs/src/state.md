# Client state

daRPC keeps a current view of each connected game client. This includes the
character sheet, map position, inventory, equipment, abilities, active spell
effects, and world objects known to that client. The view belongs to `darpc.dll`
inside that client process, then travels to `darpcd.exe` as safe values instead
of raw memory addresses.

State comes from two complementary sources:

- A complete snapshot reads everything needed to describe the client now.
- Small event-driven updates keep that snapshot current as the game changes.

This lets daRPC attach to a character who is already in game while still
reporting later changes quickly.

## Complete snapshots

A snapshot is a single, consistent observation of one client. The implemented
snapshot includes:

- Lifecycle, revision, capture tick and duration, and world generation.
- Character identity, name, gender, hairstyle, hair color, body sprite, class,
  action restriction, gold, weight and maximum weight, progression, attributes,
  vitals, combat modifiers, and elemental affinities.
- Map identity, name, coordinates, and dimensions.
- Occupied inventory and equipment slots with appearance, names, quantities,
  and durability where applicable.
- Occupied spellbook and skillbook slots with names, icons, levels, target
  behavior, text-input prompts, lines, and available cooldown state.
- Active spell effects with their icon and relative remaining-duration band.
- Known players, monsters, Mundanes, and ground items with their available
  identity, appearance, position, direction, and item stack order.

The Dynamic Link Library (DLL) and binary protocol move this as one complete
snapshot. The daemon stores it and presents separate REST resources for status,
inventory, equipment, spellbook, skillbook, effects, and world objects. Reading
one of those HTTP resources does not make the game client scan memory again.

Full snapshots run when daRPC needs a reliable baseline, such as on a new daemon
connection or after it detects a missed update. They do not run on every game
tick.

## How values are presented

daRPC favors useful game concepts over raw client internals:

- Empty inventory, equipment, spellbook, and skillbook slots are omitted.
- Inventory slot 60 is omitted because it duplicates the top-level `gold`
  value.
- Missing information stays absent instead of becoming a guessed default.
- Inventory and equipment sprite values have the client's internal item and
  monster classification bits removed.
- Stackable items expose `can_stack` and `quantity` separately. Their names do
  not include the rendered `[ quantity ]` suffix.
- Equipment uses names such as `weapon`, `left_ring`, and `accessory3`
  instead of numeric slot positions.
- Text-input spells can expose a cleaned ASCII prompt. Other target modes do not
  expose one.

Some fields need additional context:

- Gender, `hair_style`, `hair_color`, and `body_sprite` come from the local
  character's appearance record. They are unavailable together while the
  character is displayed through a monster-disguise image.
- `is_action_restricted` reflects a specific client flag used by movement,
  world drops, incoming exchange start, and inventory rearrangement. It does not
  mean every action is blocked. Turning and the usual item, skill, and spell
  activation paths can still work.
- `is_blinded` is true only when the retained status event contains the
  client's blind code, `0x08`.
- A spell cooldown may be known to be active even when the client does not keep
  an exact remaining duration.
- Map names normally come from the visible map pane. An accepted map-change
  event can provide the name when a fresh event is newer than that memory view.
- Ground-item `z_index` is local to one tile. Zero is the bottom item and higher
  values are drawn above it.
- A creature's numeric sprite may be absent immediately after late attach when
  retained client memory exposes only its loaded image session. A later draw
  event supplies the numeric sprite without another full snapshot.

## Client lifecycle

Every snapshot identifies the client's current stage:

- `unknown`: daRPC cannot classify the current scene.
- `title`: the client is at the title or login flow.
- `transition`: the client is between stable world states.
- `in_game`: a usable character and world are active.
- `disconnected`: the reconnect dialog is visible.

The reconnect dialog takes priority over the scene behind it. When the old world
is still readable, a disconnected snapshot can retain the last character and
map state. If there is no valid world, character state is absent.

Reconnect detection is part of the same snapshot capture. daRPC checks the
active dialog list carefully and does not keep dialog pointers after the
capture finishes.

When daRPC is loaded before login, the first snapshot naturally reports
`title`. A full `SUserAppearance` server packet marks the point where the client
has entered the game world. daRPC then requires a fresh snapshot so character,
map, and collection state are established from the post-login client memory.
The state-only form of that packet updates the action restriction without
forcing a full snapshot.

## Safe snapshot capture

The game changes most of this state on its main thread. daRPC therefore asks the
next game tick to perform the memory walk on that same thread. It does not
suspend the whole process, and a remote thread does not race through live game
structures.

The game-thread portion is deliberately small:

1. Validate the known roots, pointer chains, lengths, and slots.
2. Copy bounded, pointer-free values directly into memory owned by the DLL.
3. Publish the completed copy as one unit.

The snapshot has a dedicated, preallocated 64 KiB buffer plus a separate fixed
world-object buffer. Neither is placed on the game's stack. The game-thread path
does not allocate, format text, write logs, serialize messages, or perform
named-pipe input/output. A separate pipe worker claims the completed copy,
builds the domain collections, and sends the protocol response. The atomic
ownership handoff prevents that worker from seeing a half-written snapshot or
the game thread from overwriting a snapshot being read.

If a future field belongs to a different game thread, it will need its own safe
copy or synchronization rule before daRPC can expose it.

## Real-time updates

After the initial snapshot, daRPC observes selected server events after the game
client has handled them. The currently supported event families are
`SStatus`, `SSpelled`, the action-state portion of `SUserAppearance`, `SMove`,
`SUserPosition`, `SDrawObjects`, `SDrawHumanObjects`, `SMoveObject`,
`SChangeDirection`, `SRemoveObjects`, `SAddInventory`, `SRemoveInventory`,
`SAddSpell`, `SRemoveSpell`, `SAddSkill`, `SRemoveSkill`, `SMessage`, and
`SSay`.

Together, these events keep the following values current:

- Level, attributes, vitals, weight, progression, and gold.
- Combat modifiers and blinded state.
- `is_action_restricted`.
- Accepted map coordinates.
- Active spell effects.
- Occupied inventory, spellbook, and skillbook slots.
- Known world-object appearance, removal, movement, and direction.
- Typed chat and system messages.

The event observer also runs on the game thread, so it performs only bounded
work. It copies at most 8 KiB from a recognized event into guarded static
scratch memory, ignores values that did not change, and publishes pointer-free
updates. Nested observation is skipped instead of sharing that scratch memory.
The original game handler always runs first, and daRPC preserves its return
value.

Collection packets identify the slot that the client changed. daRPC waits for
a bounded 5 ms quiet period after the last related packet, then rereads the
relevant collection into preallocated DLL memory and compares the affected
slots with its retained state. The short settling window covers slot operations
whose source and destination packets arrive in separate dispatcher calls. It
also lets the client finish its own update and avoids trusting packet text as
the final display state.

All slots changed by one group of packets are reconciled together. Moving or
swapping an entry reports changes to the involved slots instead of a false
removal and addition. Stack quantity increases and decreases are additions and
removals, while a split, merge, or move with the same total quantity is a
change. Repeating an identical update produces nothing. The complete batch is
reduced into daemon state before REST readers or event subscribers can observe
the result.

Game-world state and local user-interface state do not always follow the same
rules. Dialogs, selections, focus, and other client-only values may require
memory or local input observation because a network proxy cannot see them.

## Map movement and transitions

Normal movement and map changes use different update rules.

An accepted `SMove` acknowledgement updates only the character's x/y
coordinates. A refresh or correction can provide an authoritative position
through `SUserPosition`.

Changing maps takes two server events:

1. The map-size event stages the new map identity, name, width, and height.
2. The following `SUserPosition` commits the staged map and its x/y coordinates
   together.

daRPC does not publish the staged map by itself. Movement acknowledgements are
also ignored while a map change is pending. A requested full snapshot waits for
this boundary, so consumers never receive the new map with coordinates left
over from the previous map.

## Spell effects

The initial memory walk reads the client's ten spell-effect slots. Each active
slot contains an icon and one of six relative duration stages. The client does
not retain an exact remaining time, so daRPC reports the same color stages the
game displays.

From longest to shortest, the stages are `white`, `red`, `orange`,
`yellow`, `green`, and `blue`.

After the snapshot, `SSpelled` keeps those slots current:

- A nonzero stage adds an icon or changes its current stage.
- Stage zero removes the icon.
- A new icon is ignored when all ten client slots are occupied.

These changes update the retained effects resource and become
`effect_added`, `effect_changed`, or `effect_removed` events without
another full memory walk.

## World objects

Each client maintains its own observed object collection. It contains players,
creatures, and ground items identified by the client object ID:

- Players expose an available name, tile position, and facing direction.
- Creatures are classified as a monster or Mundane. They expose an available
  name and sprite, tile position, and facing direction.
- Ground items expose a sprite, tile position, and per-tile `z_index`.

The initial snapshot walks the client's bounded object tree and copies no
addresses out of the game thread. It also fills the local player's name from the
character snapshot when the retained world object has no separate name.

Later draw packets add or replace objects. Movement and direction packets update
living objects, and remove packets delete explicit IDs. Accepted self movement
also updates the local player's position and retires cached observations that
have moved outside the client's bounded observation area. A map transition
clears the collection atomically before objects on the new map are accepted.

This is an observation, not permanent world truth. An object may leave one
client's area while another client can still see it. The daemon therefore keeps
the collection attached to its source client and does not merge it into a
global entity list.

## Chat and system messages

Chat is an ordered event stream, not part of the complete character snapshot.
The DLL observes the two server message families and turns their displayed
format into seven useful types: `say`, `shout`, `whisper`, `guild`, `group`,
`system`, and `world`.

Formatting that only exists to label the in-game line is removed. For example,
`Aisling: hello`, `[!Aisling] hello`, and `<!Aisling> hello` retain `Aisling`
as the sender and `hello` as the text. Whispers also distinguish the other
participant as a sender or recipient. The local character name fills the other
side of a whisper when it is available.

Empty and whitespace-only messages are ignored. A world shout is delivered by
the client as both a typed history message and a rendered shout companion.
daRPC keeps only the typed `world` record, with its sender and normalized text,
instead of exposing the same line again as `shout`.

Popup, confirmation, score, and spell-chant messages are not retained. They do
not represent the chat and system history this resource is meant to expose.
Text is copied into a fixed 256-byte event field on the game thread, then
decoded into normal strings by the pipe worker. A longer displayed line is
ignored instead of allocating or truncating it in the hook.

The DLL uses the same fixed 1 MiB update queue as other events and does not keep
a separate unlimited chat log. The daemon keeps a bounded lookback for each DLL
instance: at most 4,096 messages and at most 1 MiB of participant and message
text. It removes the oldest entries first. This lookback survives an ordinary
pipe reconnect while the daemon and DLL instance remain the same, but it does
not persist across daemon restarts or DLL reloads.

## Event queue and recovery

The game thread writes updates to a fixed 1 MiB queue. The pipe worker is the
only reader and returns up to 192 events in one long poll.

This queue is designed to protect the game. If it fills, the game thread does
not wait for the daemon. daRPC drops the new event and records the missing
sequence. The daemon then requests a fresh snapshot instead of guessing what
changed.

The same recovery path handles authoritative lifecycle boundaries such as a
completed login. This keeps snapshot replacement and queue-overflow recovery on
one ordering mechanism.

The DLL keeps updating its current state even when no daemon is connected. It
does not keep an unlimited history during that downtime.

## Snapshot and event ordering

Every daemon connection starts with a fresh complete snapshot. That snapshot
includes the event sequence already reflected in its values. Older queued
events are discarded, while events after that sequence remain available.

This creates a clear handoff:

1. Establish current state with a snapshot.
2. Continue from the next ordered event.
3. Request another snapshot after a sequence gap, queue overflow, revision gap,
   or slow consumer.

No update can slip between the snapshot and the event stream. Reconnecting uses
a new boundary rather than replaying an unbounded backlog.

## Per-client state and the shared world

Character, session, and local user-interface state always belong to one client.
Maps and entities are different. Multiple characters can observe the same game
world, but each sees only its current area and stops receiving information once
something leaves view.

A future shared-world view can combine compatible observations from several
clients, but it must still remember:

- The world or server and map where the observation belongs.
- A stable entity identity when the client provides one.
- Which client observed it.
- When it was last observed or updated.
- Whether it is visible, stale, explicitly removed, or uncertain.

An entity disappearing from one character's view does not prove that it left
the world. Another character on the same map may have a newer observation.
Conflict and expiration rules must therefore be explicit.

Queries also need a point of view. For example, "monsters near player X" starts
with player X's current position and may use recent same-map observations from
other clients. The original per-client observations remain available even when
the daemon later offers a combined world view.
