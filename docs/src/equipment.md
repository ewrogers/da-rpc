# Equipment

The equipment resource describes every occupied wearable slot for the current
character.

## Reading equipment

```text
GET /clients/{client}/equipment
```

Each entry includes:

- A readable `slot` name
- `sprite` and `dye_color`
- An available item `name`
- `durability` and `max_durability`

The sprite value has the client's internal item classification flag removed.
Empty slots are omitted.

Equipment slot names are stable snake-case values:

```text
weapon, armor, shield, helmet, earrings, necklace,
left_ring, right_ring, left_gauntlet, right_gauntlet,
belt, greaves, boots, accessory1, overcoat, over_helm,
accessory2, accessory3
```

## Updates and events

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
