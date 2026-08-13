# Skills

The skillbook resource lists the learned skills in the character's skill pane.
daRPC can also use one of those skills through the same native client path as a
normal activation.

| Use | Route or events |
| --- | --- |
| Read learned skills | `GET /clients/{client}/skills` |
| Use a skill | `POST /clients/{client}/skills/use` |
| Swap skills | `POST /clients/{client}/skills/swap` |
| Perform a basic attack | `POST /clients/{client}/assail` |
| Watch skillbook and use activity | [Skill events](events.md#skill-events) |

## Reading the skillbook

```console
curl "http://127.0.0.1:2626/clients/ZiLo/skills"
```

Each occupied slot includes:

- One-based `slot`
- `icon` and available `name`
- Current `level` and `max_level`
- `cooldown.active`
- Optional `cooldown.cooldown_ms` containing the total cooldown duration
- Optional `cooldown.remaining_ms` when the client retains an exact expiry

A cooldown can be known to be active even when the exact remaining time is not
available.

```text
Skillbook {
    observation: ObservationMetadata,
    skills: Skill[]?,
}

Skill {
    slot: u8,
    icon: u16,
    name: string?,
    level: u8,
    max_level: u8,
    cooldown: Cooldown,
}

Cooldown {
    active: bool,
    cooldown_ms: u32?,
    remaining_ms: u32?,
}
```

## Using a skill

Select the skill by one-based slot:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"slot":5}' \
  "http://127.0.0.1:2626/clients/ZiLo/skills/use"
```

Or by case-insensitive name:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"Assail"}' \
  "http://127.0.0.1:2626/clients/ZiLo/skills/use"
```

Use exactly one selector. Slots range from 1 through 90. An invalid body or
slot returns `400 Bad Request`; an unknown name or empty learned slot returns
`404 Not Found`.

The daemon resolves a name against its retained skillbook. The DLL then checks
the live slot again and calls the client's normal skill activation routine. It
does not open the skill panel, change the visible lower-tray page, move the
mouse, or synthesize a click.

An executed command means local client activation ran. `skill.used` is the
later observation that the client submitted the skill. Neither result alone is
a promise that the game server accepted the action.

## Basic attack (Assail)

Assail is the client's built-in basic attack action. It does not select or
require a learned skillbook slot:

```console
curl --request POST "http://127.0.0.1:2626/clients/ZiLo/assail"
```

The direct named-pipe client exposes the same action for one injected process:

```console
darpc assail --pid 3780
```

The command queues on the game thread and submits the native client attack
packet `0x13`. A successful command result means the packet was submitted. The
corresponding server response can produce `player.animated` and `sound.played`
events, which provide the observable animation and audio cues.

Use `/assail` for the built-in basic attack. Use `/skills/use` when selecting a
learned skillbook entry by slot or name.

## Swapping skills

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"source":{"slot":5},"destination":{"name":"Assail"}}' \
  "http://127.0.0.1:2626/clients/ZiLo/skills/swap"
```

Both selectors accept exactly one of `slot` or case-insensitive `name`, using
the same payload as inventory and spell swaps. A destination slot may be empty.
The source must be occupied, and the resolved slots must be different.

## Skillbook events

The complete payload structures and batch rules are in
[Skill events](events.md#skill-events).

| Event | Meaning |
| --- | --- |
| `skill.added` | A learned skill appeared in a slot. |
| `skill.removed` | A skill left a slot. |
| `skill.changed` | A skill moved or its retained details changed. |
| `skill.cooldown` | A retained skill entered or restarted cooldown. |
| `skill.ready` | A retained skill left cooldown and is ready to use. |

These events use the same `batch_index`, `batch_count`, `slot`, `before`, and
`after` shape as [inventory events](inventory.md#inventory-events). Moving or
swapping skills can update several slots in one batch. Identical same-slot
updates are ignored.

`skill.changed` is not emitted for a cooldown-only transition. A cooldown event
contains `observation`, the one-based `slot`, optional `name`, optional
`cooldown_ms`, and optional `remaining_ms`. `cooldown_ms` is the stable total
duration, while `remaining_ms` is the time left at observation and never
exceeds the total when both are present. A ready event
contains `observation`, `slot`, and optional `name`. When the client exposes an
exact skill expiry, daRPC schedules a read at that deadline. Otherwise it polls
only the watched active slot until the skill is ready.

## Skill use event

`skill.used` is emitted when daRPC observes the client's outbound skill-use
submission. It contains the one-based `slot` and the skill `name` when the
daemon can resolve it from the current skillbook.

This event also covers skills used through the normal game interface. It is an
observation of the client behavior, not only a receipt for the REST command.

## Availability

`skills: null` means the skillbook was unavailable. An empty array means it was
read successfully and no occupied skill slots were found.
