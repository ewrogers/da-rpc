# Spells

The spellbook resource describes learned spells, their targeting behavior, and
their visible cooldown state. daRPC can cast a spell through native client
methods and report each stage of delayed casting.

## Reading the spellbook

```text
GET /clients/{client}/spellbook
```

Each occupied slot includes:

- One-based `slot`
- `icon` and available `name`
- Current `level` and `max_level`
- The number of chant `lines`
- `target_type`: `none`, `target`, or `text_input`
- An optional cleaned ASCII `prompt` for text-input spells
- Cooldown activity and an optional exact remaining duration

The prompt is only present for text-input spells. A cooldown can be known to be
active without an exact `remaining_ms` value.

## Casting a spell

```text
POST /clients/{client}/spells/cast
```

Select the spell by one-based slot or case-insensitive name:

```json
{
  "name": "Mist"
}
```

Targeted spells accept a character or Mundane name, a visible object ID, or a
map tile:

```json
{ "name": "Taunt", "target": "ZiLo" }
```

```json
{ "slot": 12, "target": 1843 }
```

```json
{ "name": "Ground Spell", "target": { "x": 20, "y": 14 } }
```

A targeted spell with no target defaults to the casting character. A name
search is case-insensitive and checks visible players within 14 tiles before
visible Mundanes. Object IDs must identify a current visible target within the
same range. Tile coordinates are zero-based and must fit the current map.
For example, a named Mundane target could use `"target": "Beggar"`.

Text-input spells use `input`:

```json
{
  "name": "Learning Spell",
  "input": "Elemental Bless 6"
}
```

Text input must contain 1 through 100 ASCII bytes. Extra, conflicting, or
incorrect argument types return `400 Bad Request`. An unknown spell or named
target returns `404 Not Found`.

The DLL checks the live spell slot and arguments again before calling the
matching native client routine. It does not switch the visible spell panel or
synthesize user input.

## Casting events

daRPC observes the same outbound spell path for casts started through the game
interface and casts requested through REST.

| Event | Meaning |
| --- | --- |
| `spell.begin` | A delayed spell began. Includes `slot`, optional `name`, and `total_lines`. |
| `spell.chant` | One visible chant line was submitted. Includes `line` and `total_lines`. |
| `spell.cast` | The final spell use was submitted. Includes any retained arguments. |
| `spell.cancelled` | A delayed spell ended without a final cast. |

An instant spell normally produces only `spell.cast`. A delayed spell normally
produces `spell.begin`, one or more `spell.chant` events, and then `spell.cast`.

Cancellation `source` is `client`, `server`, or `replaced`. Starting another
spell while chanting is allowed. daRPC reports the old spell as replaced before
it reports the new spell's begin or cast event, so the two casts do not appear
to overlap.

The final cast can retain one of these argument forms:

```text
target: object ID, available name, and coordinates
input:  submitted text
values: bounded numeric values used by less common spell types
unknown: the argument could not be classified
```

Spells with no arguments omit this field. Target names are best-effort daemon
enrichment. The object ID and coordinates come from the observed submission.

`is_casting` in [character status](status.md) follows the same ordered begin,
cast, and cancellation events.

## Spellbook events

| Event | Meaning |
| --- | --- |
| `spell.added` | A learned spell appeared in a slot. |
| `spell.removed` | A spell left a slot. |
| `spell.changed` | A spell moved or its retained details changed. |

Spellbook changes use `batch_index`, `batch_count`, `slot`, `before`, and
`after`. Moving or swapping spells can create several frames in one batch. The
daemon applies the full batch before it broadcasts the first frame.

## Availability

`spells: null` means the spellbook was unavailable. An empty array means it was
read successfully and no occupied spell slots were found.
