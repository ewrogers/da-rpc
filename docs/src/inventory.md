# Inventory

The inventory resource contains the items carried by one character. Empty
slots are omitted, which makes it easy to scan the items that are actually
present.

| Use | Route or events |
| --- | --- |
| Read carried items | `GET /clients/{client}/items` |
| Use, drop, give, or pick up an item | `POST /clients/{client}/items/...` |
| Swap inventory slots | `POST /clients/{client}/items/swap` |
| Drop gold | `POST /clients/{client}/gold/drop` |
| Give gold | `POST /clients/{client}/gold/give` |
| Watch changes and submitted actions | [Inventory events](events.md#inventory-and-equipment-events) |

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

```text
Inventory {
    observation: ObservationMetadata,
    items: InventoryItem[]?,
}

InventoryItem {
    slot: u8,
    sprite: u16,
    dye_color: u8,
    name: string?,
    quantity: u32,
    can_stack: bool,
    durability: u32,
    max_durability: u32,
}
```

The sprite value has the client's internal item classification flag removed.
Stackable names do not include the rendered `[ quantity ]` suffix because
quantity is already a separate field.

The client's special gold slot is omitted. Use the top-level `gold` field from
[character status](status.md) instead.

## Using and moving items

Use an item by one-based slot or case-insensitive name:

```text
POST /clients/{client}/items/use
{"name":"Red Potion"}
```

Drop an item only at a zero-based map tile:

```text
POST /clients/{client}/items/drop
{"slot":12,"destination":{"x":3,"y":6}}
```

Give an item only to a visible human, monster, or NPC:

```text
POST /clients/{client}/items/give
{"name":"Red Potion","quantity":2,"target":"ZiLo"}
```

`quantity` defaults to 1. An empty slot, zero quantity, quantity larger than
the current stack, or quantity other than 1 for a non-stackable item returns
`400 Bad Request`. The DLL checks the live slot and quantity again on the game
thread before submitting the action.

Pick up the top ground item at a tile with:

```text
POST /clients/{client}/items/pickup
{"position":{"x":3,"y":6}}
```

The client protocol identifies the tile rather than a ground object ID. On a
stacked tile, the server decides which visible item is picked up.

Gold uses the same distinct ground and entity routes:

```text
POST /clients/{client}/gold/drop
{"amount":100,"destination":{"x":3,"y":6}}

POST /clients/{client}/gold/give
{"amount":100,"target":"ZiLo"}
```

An object target may be a visible human or named creature, matched without case
sensitivity, or its numeric object ID. Name lookup checks human players first,
then falls back to monsters and NPCs. The local character is not a valid
transfer target.

Rearrange inventory with the same swap payload used by skills and spells:

```text
POST /clients/{client}/items/swap
{"source":{"name":"Red Potion"},"destination":{"slot":12}}
```

Each selector contains exactly one of `slot` or `name`. Names are matched
without case sensitivity. A destination selected by slot may be empty; a name
always resolves to an occupied slot. The two selectors must resolve to
different slots.

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

The complete payload structures and batch rules are in
[Inventory and equipment events](events.md#inventory-and-equipment-events).

| Event | Meaning |
| --- | --- |
| `item.added` | A slot gained an item or a stack quantity increased. |
| `item.removed` | A slot became empty or a stack quantity decreased. |
| `item.changed` | An existing item moved, swapped, split, merged, or changed details. |
| `item.used` | The client submitted an item-use request. |
| `item.dropped` | The client submitted an item drop with slot, quantity, and destination. |
| `item.given` | The client submitted an item exchange request with slot, quantity, and target ID. |
| `item.picked_up` | The client submitted a tile pickup with its chosen destination slot. |
| `gold.dropped` | The client submitted a gold drop with amount and destination. |
| `gold.given` | The client submitted a gold transfer with amount and target ID. |

These names refer only to carried inventory. Ground items use
`item.appeared`, `item.disappeared`, and `item.moved` as described in
[World and movement](world.md).

Action events describe an outgoing request observed at the client's normal
packet boundary. Giving an item opens the game's ordinary exchange flow; it
does not mean the other player accepted it. Later inventory and gold state
events confirm results accepted by the server.

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
handle any inventory event by rereading `/items`.

## Availability

`items: null` means the client could not expose inventory at that time. An
empty array means the inventory was read successfully and no occupied items
were found.
