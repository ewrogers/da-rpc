# Skills

The skillbook resource lists the learned skills in the character's skill pane.
daRPC can also use one of those skills through the same native client path as a
normal activation.

| Use | Route or events |
| --- | --- |
| Read learned skills | `GET /clients/{client}/skills` |
| Use a skill | `POST /clients/{client}/skills/use` |
| Watch skillbook and use activity | [Skill events](events.md#skill-events) |

## Reading the skillbook

```text
GET /clients/{client}/skills
```

Each occupied slot includes:

- One-based `slot`
- `icon` and available `name`
- Current `level` and `max_level`
- `cooldown.active`
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
```

## Using a skill

```text
POST /clients/{client}/skills/use
```

Select the skill by one-based slot:

```json
{
  "slot": 5
}
```

Or by case-insensitive name:

```json
{
  "name": "Assail"
}
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

## Skillbook events

The complete payload structures and batch rules are in
[Skill events](events.md#skill-events).

| Event | Meaning |
| --- | --- |
| `skill.added` | A learned skill appeared in a slot. |
| `skill.removed` | A skill left a slot. |
| `skill.changed` | A skill moved or its retained details changed. |

These events use the same `batch_index`, `batch_count`, `slot`, `before`, and
`after` shape as [inventory events](inventory.md#inventory-events). Moving or
swapping skills can update several slots in one batch. Identical same-slot
updates are ignored.

## Skill use event

`skill.used` is emitted when daRPC observes the client's outbound skill-use
submission. It contains the one-based `slot` and the skill `name` when the
daemon can resolve it from the current skillbook.

This event also covers skills used through the normal game interface. It is an
observation of the client behavior, not only a receipt for the REST command.

## Availability

`skills: null` means the skillbook was unavailable. An empty array means it was
read successfully and no occupied skill slots were found.
