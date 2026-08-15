# Field maps

Field maps are the native world-map panels opened by a map warp tile. daRPC
exposes the panel only while an exact `FieldMapPane` is registered and visible
in the supported client. Receiving the server packet alone does not make the
resource active.

## Reading the active field map

```console
curl "http://127.0.0.1:2626/clients/ZiLo/field-map"
```

The response contains observation metadata and a nullable `field_map`:

```text
FieldMapState {
    revision: u32,
    field_name: string,
    current_node_index: u8?,
    destinations: Vec<FieldMapDestination>,
    selection: FieldMapSelection?,
}

FieldMapDestination {
    index: u8,
    screen_x: u16,
    screen_y: u16,
    name: string,
    checksum: u16,
    map_id: u16,
    map_x: u16,
    map_y: u16,
}
```

`field_name` is the local asset stem, such as `field001`. The current node is
nullable because a malformed or out-of-range server index does not identify a
destination. Destination names are not required to be unique, so use `index`
as the stable selector for one revision.

`screen_x` and `screen_y` are server fallback presentation coordinates. The
client can replace them with values from its local `<field_name>.txt` asset.
They are not guaranteed pointer-click coordinates.

The checksum and travel coordinates are exposed for observation and debugging.
They are read-only command inputs. daRPC reconstructs the selection packet from
the retained destination so callers cannot forge or mix travel fields.

## Selecting a destination

Submit the current revision and one zero-based destination index:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":11,"destination_index":1}' \
  "http://127.0.0.1:2626/clients/ZiLo/field-map/select"
```

The direct Windows command is:

```console
darpc field-map select --pid 1234 11 1
```

The daemon and DLL both validate the revision and index. HTTP 409 indicates no
active panel, a stale revision, or a selection that was already submitted.
HTTP 400 indicates that the index is not one of the retained destinations.

The client normally delays its native `CFieldMap` send while the marker moves.
daRPC publishes `selection_submitted` only after the actual outgoing packet is
observed. It does not publish a separate selection-started event. Submission
does not prove that the server accepted the destination, and it does not close
the panel. Later location and map events are authoritative for arrival.

## Live events

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `field_map.opened` | `field_map_opened` | A validated native field-map panel became active. |
| `field_map.changed` | `field_map_changed` | The server replaced the active field map and destination list. |
| `field_map.selection_submitted` | `field_map_selection_submitted` | The client sent the selected destination's canonical packet. |
| `field_map.closed` | `field_map_closed` | The native panel was no longer registered and visible. |

Every event carries the complete field-map state. `closed` carries it as
`previous`; the other events use `field_map`. On an event-stream resync, reread
`GET /clients/{client}/field-map`.

## Client ownership and bounds

The injected DLL retains field-map state without depending on the daemon. The
server packet is validated and copied into bounded storage on the client main
thread, then decoded after transfer to the IPC thread. Pane checks rescan the
live bounded event-dispatcher collection and never retain a native pane pointer
between observations.

Closing the pane makes `active_field_map` null but retains the last validated
definition inside the DLL. If the client later reopens that cached native pane
without another server packet, daRPC publishes a new `opened` revision and
resets its submitted selection. The retained definition is never exposed while
the pane is hidden or unregistered.
