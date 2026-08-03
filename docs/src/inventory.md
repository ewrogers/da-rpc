# Inventory

The inventory resource contains the items carried by one character. Empty
slots are omitted, which makes it easy to scan the items that are actually
present.

## Reading inventory

```text
GET /clients/{client}/items
```

Each occupied item includes:

- `slot`, using the client's one-based inventory slot
- `sprite` and `dye_color`
- An available canonical `name`
- `quantity` and `can_stack`
- `durability` and `max_durability`

The sprite value has the client's internal item classification flag removed.
Stackable names do not include the rendered `[ quantity ]` suffix because
quantity is already a separate field.

The client's special gold slot is omitted. Use the top-level `gold` field from
[character status](status.md) instead.

## How inventory stays current

The initial baseline reads the occupied inventory slots from client memory.
Later inventory packets tell daRPC which slots may have changed.

Some game actions update more than one slot. Moving an item, swapping two
items, splitting a stack, or merging stacks can arrive as several closely
spaced updates. daRPC waits for a short quiet period, rereads the affected
slots, and applies the complete group before REST or SSE consumers see it.

This avoids reporting a simple move as an item leaving and immediately coming
back. Repeating an identical same-slot update produces no event.

## Inventory events

| Event | Meaning |
| --- | --- |
| `item.added` | A slot gained an item or a stack quantity increased. |
| `item.removed` | A slot became empty or a stack quantity decreased. |
| `item.changed` | An existing item moved, swapped, split, merged, or changed details. |

These names refer only to carried inventory. Ground items use
`item.appeared`, `item.disappeared`, and `item.moved` as described in
[World and movement](world.md).

Each inventory event contains:

```text
{
    observation,
    batch_index,
    batch_count,
    slot,
    before,
    after,
}
```

`before` is null when the slot was empty. `after` is null when the slot became
empty. `batch_index` is zero-based, and all frames from one multi-slot change
share the same `batch_count`. The daemon has already applied the whole batch to
the REST inventory before it sends the first frame.

When a tool needs a simple answer rather than slot-by-slot history, it can
handle any inventory event by rereading `/inventory`.

## Availability

`items: null` means the client could not expose inventory at that time. An
empty array means the inventory was read successfully and no occupied items
were found.
