# Spells

The spellbook resource describes learned spells, their targeting behavior, and
their visible cooldown state. daRPC can cast a spell through native client
methods and report each stage of delayed casting.

| Use | Route or events |
| --- | --- |
| Read learned spells | `GET /clients/{client}/spells` |
| Cast a spell | `POST /clients/{client}/spells/cast` |
| Swap spells | `POST /clients/{client}/spells/swap` |
| Watch casting, feedback, and spellbook changes | [Spell events](events.md#spell-events) |

## Reading the spellbook

```console
curl "http://127.0.0.1:2626/clients/ZiLo/spells"
```

Each occupied slot includes:

- One-based `slot`
- `icon` and available `name`
- Current `level` and `max_level`
- The number of chant `lines`
- `target_type`: `none`, `target`, or `text_input`
- An optional cleaned ASCII `prompt` for text-input spells
- Cooldown activity plus optional total and exact remaining durations

The prompt is only present for text-input spells. A cooldown can be known to be
active without exact `cooldown_ms` or `remaining_ms` values. daRPC retains exact
timing from live server action-delay packets. A spell already cooling when the
DLL attaches exposes only its active flag because the spellbook retains no
start or end timestamp.

```text
Spellbook {
    observation: ObservationMetadata,
    spells: Spell[]?,
}

Spell {
    slot: u8,
    icon: u16,
    name: string?,
    level: u8,
    max_level: u8,
    lines: u8,
    target_type: SpellTargetType,
    prompt: string?,
    cooldown: Cooldown,
}

Cooldown {
    active: bool,
    cooldown_ms: u32?,
    remaining_ms: u32?,
}
```

## Casting a spell

Select the spell by one-based slot or case-insensitive name:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"Mist"}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/cast"
```

Targeted spells accept a character or Mundane name, a visible object ID, or a
map tile:

```console
curl --request POST --header "Content-Type: application/json" \
  --data '{"name":"Taunt","target":"OtherPlayer"}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/cast"
```

```console
curl --request POST --header "Content-Type: application/json" \
  --data '{"slot":12,"target":1843}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/cast"
```

```console
curl --request POST --header "Content-Type: application/json" \
  --data '{"name":"Ground Spell","target":{"x":20,"y":14}}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/cast"
```

A targeted spell with no target defaults to the casting character. A name
search is case-insensitive and checks visible players within 14 tiles before
visible Mundanes. Object IDs must identify a current visible target within the
same range. Tile coordinates are zero-based and must fit the current map.
For example, a named Mundane target could use `"target": "Beggar"`.

Text-input spells use `input`:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"Learning Spell","input":"Elemental Bless 6"}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/cast"
```

Text input must contain 1 through 100 ASCII bytes. Extra, conflicting, or
incorrect argument types return `400 Bad Request`. An unknown spell or named
target returns `404 Not Found`.

The DLL checks the live spell slot and arguments again before calling the
matching native client routine. It does not switch the visible spell panel or
synthesize user input.

After the native routine accepts a cast, the command remains pending for 500
milliseconds so immediate server feedback can determine its result. The exact
system message `That doesn't work here.` completes the command as rejected,
which reports attempts to cast on no-cast maps as failures. Silence during the
bounded response window completes the command successfully.

Spell casts have 10 percent tolerance on the normal one-second start deadline.
This bounded window accommodates small native dispatcher overruns during an
earlier cast; the one-second action deadline remains in effect for other
commands.

## Swapping spells

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"source":{"name":"Mist"},"destination":{"slot":12}}' \
  "http://127.0.0.1:2626/clients/ZiLo/spells/swap"
```

Both selectors accept exactly one of `slot` or case-insensitive `name`, using
the same payload as inventory and skill swaps. A destination slot may be empty.
The source must be occupied, and the resolved slots must be different.

## Casting events

The complete payload structures and stream names are in
[Spell events](events.md#spell-events).

daRPC observes the same outbound spell path for casts started through the game
interface and casts requested through REST.

| Event | Meaning |
| --- | --- |
| `spell.begin` | A delayed spell began. Includes `slot`, optional `name`, and `total_lines`. |
| `spell.chant` | One visible chant line was submitted. Includes `line` and `total_lines`. |
| `spell.cast` | The final spell use was submitted. Includes any retained arguments. |
| `spell.cancelled` | A delayed spell ended without a final cast. |
| `spell.succeeded` | The game confirmed a submitted spell by name. |
| `spell.failed` | The game reported that a submitted spell failed or was rejected. |
| `spell.received` | Another player, Mundane, or monster cast or attacked with a spell on this character. |

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

### Cast results

`spell.cast` means the client submitted the spell. It does not by itself mean
the server accepted the result. The daemon keeps up to 256 recent submissions
for each connected DLL instance and compares later system feedback with that
queue. A submission expires after five seconds.

A named success such as `You cast Mist` matches the oldest queued cast with
that spell name. Generic failures match a cast only when exactly one submission
is pending. When several submissions are pending, the feedback does not contain
enough information to prove which cast failed, so the daemon discards the
ambiguous candidates and emits only the original `message.system` event. The
queue is held only in memory and is cleared when the DLL disconnects or the
daemon restarts.

The system message `You failed to concentrate.` matches a queued `Fas Spiorad`
cast by name and produces `spell.failed` with reason `failed`.

`spell.succeeded` and `spell.failed` retain the submitted `slot`, available
spell `name`, and cast `arguments`. They also include:

```text
feedback:          original system feedback
submitted_tick_ms: client tick when spell.cast was observed
elapsed_ms:        wrapping millisecond difference to the feedback
```

Failure `reason` is one of:

```text
failed, error, resisted, already_active, conflicting_effect
```

For a conflicting curse, `active_spell` contains the spell named by the game
when available. The attempted spell remains in `name`.

`spell.received` does not need a matching local submission. It contains the
reported `caster`, spell `name`, and a `kind` of `cast` or `attack`. If that
caster is still visible, `caster_object` supplies the current world object as
best-effort context. Friendly feedback uses the game's "cast ... spell on you"
form, while harmful feedback uses its "attacks you with ... spell" form.

The original `message.system` is still retained and broadcast. Semantic spell
events add useful structure without hiding the text that appeared in the game.

## Spellbook events

| Event | Meaning |
| --- | --- |
| `spell.added` | A learned spell appeared in a slot. |
| `spell.removed` | A spell left a slot. |
| `spell.changed` | A spell moved or its retained details changed. |
| `spell.cooldown` | A retained spell entered or restarted cooldown. |
| `spell.ready` | A retained spell left cooldown and is ready to cast. |

Spellbook changes use `batch_index`, `batch_count`, `slot`, `before`, and
`after`. Moving or swapping spells can create several frames in one batch. The
daemon applies the full batch before it broadcasts the first frame.

`spell.changed` is not emitted for a cooldown-only transition. A live
action-delay packet supplies `cooldown_ms` and `remaining_ms` on
`spell.cooldown`. On late attach those fields remain absent, but daRPC polls the
active slot until it can emit `spell.ready`. Both cooldown events include
`observation`, the one-based `slot`, and the spell `name` when known.

## Availability

`spells: null` means the spellbook was unavailable. An empty array means it was
read successfully and no occupied spell slots were found.
