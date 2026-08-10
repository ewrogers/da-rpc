# Equipment

The equipment resource describes every occupied wearable slot for the current
character.

| Use | Route or events |
| --- | --- |
| Read equipped items | `GET /clients/{client}/equipment` |
| Unequip an item | `POST /clients/{client}/equipment/unequip` |
| Watch submitted unequip actions | [Equipment events](events.md#inventory-and-equipment-events) |

## Reading equipment

```console
curl "http://127.0.0.1:2626/clients/ZiLo/equipment"
```

Each entry includes:

- A readable `slot` name
- `sprite` and `dye_color`
- An available item `name`
- `durability` and `max_durability`

```text
Equipment {
    observation: ObservationMetadata,
    items: EquipmentItem[]?,
}

EquipmentItem {
    slot: EquipmentSlot,
    sprite: u16,
    dye_color: u8,
    name: string?,
    durability: u32,
    max_durability: u32,
}
```

The sprite value has the client's internal item classification flag removed.
Empty slots are omitted.

Equipment slot names are stable snake-case values:

```text
weapon, armor, shield, helmet, earrings, necklace,
left_ring, right_ring, left_gauntlet, right_gauntlet,
belt, greaves, boots, accessory1, overcoat, over_helm,
accessory2, accessory3
```

## Unequipping an item

Use the same readable slot name to move equipped gear back to inventory:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"slot":"armor"}' \
  "http://127.0.0.1:2626/clients/ZiLo/equipment/unequip"
```

The action is submitted on the client main thread and produces an
`equipment.unequipped` event when the outgoing request is observed. Its
payload contains the slot name. The event records the request, while a later
equipment snapshot or incremental equipment support confirms server state.

## Updates and events

The action payload is documented under
[Inventory and equipment events](events.md#inventory-and-equipment-events).

Equipment is included in the complete client baseline and exposed through
REST. The current implementation does not yet track later gear changes or
publish a dedicated `equipment.changed` SSE event.

`/equipment` therefore reflects the most recent complete baseline. A new daemon
connection or resynchronization captures a fresh baseline before its
`stream.ready` boundary. Until incremental equipment tracking is added, the
absence of an event or REST change is not proof that the character has not
changed gear.

## Availability

`items: null` means the equipment collection was unavailable. An empty array
means it was read successfully and every equipment slot was empty.
