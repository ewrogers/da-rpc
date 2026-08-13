# Path exclusions

Path exclusions let an external controller add per-map policy tiles that the
game client does not know are undesirable. Common examples are server-side map
warps, one-way transitions, and tiles selected by application-specific
heuristics. They preserve the client's native breadth-first search while
preventing it from considering excluded destination tiles.

## Map resources

Each client has a sparse, session-scoped registry keyed by numeric map ID. A
`PUT` replaces the complete tile list for one map:

```console
curl --request PUT \
  --header "Content-Type: application/json" \
  --data '{"tiles":[{"x":40,"y":50},{"x":41,"y":50}]}' \
  "http://127.0.0.1:2626/clients/ZiLo/maps/3001/path-exclusions"
```

The map does not have to be the client's current map. This allows a controller
to configure a trip before walking starts. Duplicate tiles are removed and the
stored list has a stable tile order.

Use the item resource to read or remove one map:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/maps/3001/path-exclusions"
curl --request DELETE \
  "http://127.0.0.1:2626/clients/ZiLo/maps/3001/path-exclusions"
```

Use the collection resource to list configured maps or clear the registry:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/maps/path-exclusions"
curl --request DELETE \
  "http://127.0.0.1:2626/clients/ZiLo/maps/path-exclusions"
```

The collection response contains each `map_id` and `tile_count`, plus
`total_tiles`. Read an item resource when the complete tile list is needed.
Deleting an absent item is idempotent. A `PUT` requires at least one tile;
delete the item to remove its last tile.

## Bounds and lifetime

The injected DLL owns the authoritative registry. It accepts:

- Map IDs from 0 through 65535
- Coordinates satisfying `0 <= x,y < 400`
- From 1 through 256 tiles per map
- At most 1024 configured maps and 65535 total tiles per client session

The sparse registry retains entries across map changes and daemon disconnects.
Every complete DLL snapshot includes the registry, so a replacement daemon
reconstructs the same resources. Entries are lost when the injected DLL is
unloaded, the game process exits, or the registry is explicitly cleared. They
are not written to disk.

## Pathfinding behavior

On a map transition, the DLL automatically publishes that map's stored tiles
to a double-buffered dense bitset. The native collision hook reads the active
bitset without a lock, allocation, or map lookup. External clients do not need
to resend exclusions after `location.changed`.

An excluded tile is rejected after the client's live collision and complete
raw-map collision views have accepted it. Exclusions affect native ground
pathfinding, daRPC destination walks, and client pursuit. They do not block a
manual one-tile step or an [exact injected route](movement.md#walking-an-exact-route).
The server and the client's normal per-step validator remain authoritative.

This separation is deliberate. Use exclusions as policy for native routing.
Use an exact route when the external planner must choose every edge, including
an intentional final step onto an excluded warp tile.

The registry, atomic activation, protocol, and API paths pass native Windows
tests. The added rejection at the supported client's live breadth-first-search
call site has not yet completed its first live-client trial. A safe first check
uses one harmless open-area tile, compares a nearby native route before and
after the `PUT`, and removes the test resource immediately afterward. Do not use
a warp tile for that initial check.

## Change events

Successful changes publish the Server-Sent Events (SSE) event
`map.exclusions_changed`. Its payload contains the standard `observation`, an
`operation` of `replaced`, `removed`, or `cleared`, an optional `map_id`, the
resulting `tile_count` for the affected map, and the resulting `map_count`.
Removal and clear events report a zero `tile_count`.

The event carries resource metadata instead of copying the full tile array.
Consumers that need the new contents should read the item or collection after
the event. The observation revision orders that read with other client state
changes. Deleting an absent map or clearing an already empty registry does not
publish an event. Replacing a map with the identical canonical tile list also
does not publish an event.
