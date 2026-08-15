# Field map state and interaction plan

## Goal

Expose the currently displayed field map as queryable client state, publish its
lifecycle and selection activity as ordered events, and allow a consumer to
select one of the displayed destinations without accepting forged or stale
travel fields.

The authoritative meaning of `active_field_map` is: a matching native
`FieldMapPane` is registered and visible in the supported client. Receiving
`SFieldMap` alone is not enough, and sending `CFieldMap` does not make it
inactive.

## Confirmed client behavior

Server opcode `0x2E` contains the field asset stem, node count, current node
index, and the complete destination list. All multibyte fields are big-endian.
The four travel words on each node are returned unchanged by client opcode
`0x3F`.

The client creates a `FieldMapPane` only after processing the packet. The event
hook already observes server events after the original dispatcher returns, so
daRPC can validate that construction succeeded before publishing the field map.

The supported client identifies the primary `FieldMapPane` through Complete
Object Locator RVA `0x0029_ADB8`. A pane is up only when all of these hold:

- it is present in the live event dispatcher's bounded pane collection;
- its primary vtable resolves to the exact `FieldMapPane` locator;
- its `Pane` visibility byte at `+0x130` is `1`;
- its registration byte at `+0x188` contains flag `0x02`.

Do not retain a native pane pointer between observations. Closing first removes
or hides a pane and may defer deletion, so every check must scan the live pane
collection again on the client main thread.

A user click does not send immediately. It selects a point, moves the native
balloon marker for roughly 0.5 through 4 seconds, and then a timer sends
`CFieldMap`. The send routine restores `legend.pal` but does not hide or
unregister the field-map pane. A successful captured flow continues with
opcode `0x67`, `SMapSize`, and normal map-entry packets, but `0x67` itself is
not a supported close signal.

The packet's `screen_x` and `screen_y` are fallback presentation coordinates.
For a node name recognized by `<field_name>.txt`, the client can substitute a
locally configured icon and position. They must not be described as guaranteed
click coordinates. Interaction should select a node, not synthesize a pointer
click.

## Public model

Add the following shared model, with the field map stored at the top level of
`ClientSnapshot`:

```text
ClientSnapshot {
    ...
    active_field_map: Option<FieldMapState>
}

FieldMapState {
    revision: u32,
    field_name: String,
    current_node_index: Option<u8>,
    destinations: Vec<FieldMapDestination>,
    selection: Option<FieldMapSelection>,
}

FieldMapDestination {
    index: u8,
    screen_x: u16,
    screen_y: u16,
    name: String,
    checksum: u16,
    map_id: u16,
    map_x: u16,
    map_y: u16,
}

FieldMapSelection {
    destination_index: u8,
}
```

`revision` is a nonzero DLL-lifetime sequence generated for every accepted
`SFieldMap`, not the global snapshot revision. Commands must carry it.
`current_node_index` is `None` when the server value is outside the destination
list, matching the client's marker behavior. Destination `index` is the
canonical interaction key. Names need not be unique.

Keep `checksum` read-only in public responses. It is useful for inspection, but
commands must never accept checksum, map ID, or coordinates from the caller.
`field_name` is an asset key only. daRPC should not redistribute or serve the
client's field-map assets.

Decode packet strings through the existing supported-client text conversion.
Bound the raw packet, node count, each string, decoded protocol payload, and
event transfer storage before allocation.

## DLL ownership and pane tracking

Add a small audited field-map pane lookup boundary. It validates the dispatcher
pointer, entry pointer, count, capacity, exact RTTI locator, visibility, and
registration during each current main-thread operation.

Add a field-map tracker modeled after dialog, but keep packet data in bounded,
allocation-free transfer storage while on the game thread. Decode owned
`String` and `Vec` values only after transfer to the IPC side.

The tracker follows these transitions:

1. After the original `0x2E` handler returns, validate and copy the packet.
2. Resolve an exact registered and visible `FieldMapPane`. Publish `Opened`
   only if both parsing and pane lookup succeed.
3. On each client tick, scan again. If the pane is no
   longer registered and visible, publish `Closed` and expose `None`.
4. Extend outgoing packet observation so exact nine-byte opcode `0x3F` bodies
   are decoded. If all four travel words match one destination in the current
   revision, retain the destination index and publish `SelectionSubmitted`.
5. Do not clear state on `0x3F`, `0x67`, or an inferred successful transfer.
   The registered-and-visible pane test remains authoritative.

If a second valid `SFieldMap` replaces an active pane, publish `Changed` with a
new revision and the complete replacement. Queue exhaustion or malformed data
must increment diagnostics and fail closed rather than publish partial state.
The next complete snapshot remains the recovery mechanism.

## State and event contract

Add one absolute `FieldMapUpdate` domain:

```text
enum FieldMapUpdate {
    Opened(FieldMapState),
    Changed(FieldMapState),
    SelectionSubmitted(FieldMapState),
    Closed { previous: FieldMapState },
}
```

The model reducer sets `active_field_map` to the supplied complete state for
the first three variants and to `None` for `Closed`. Full values prevent SSE
consumers from reconstructing state from partial node or selection deltas.

Project the updates as:

- `field_map.opened`
- `field_map.changed`
- `field_map.selection_submitted`
- `field_map.closed`

Do not publish a `selection_completed` event. `CFieldMap` proves only that a
request was sent, not that the server accepted the destination. Later location
and map events remain authoritative for arrival.

## Selection command

Append a typed protocol command:

```text
SelectFieldMapDestination {
    revision: u32,
    destination_index: u8,
}
```

The daemon first checks its current snapshot, exact revision, destination
index, and absence of an already submitted selection. The DLL repeats all
checks when the queued command executes, resolves a currently registered and
visible `FieldMapPane`, and reads the destination only from its retained
`SFieldMap` state. It then builds the exact nine-byte body:

```text
0x3F checksum map_id map_x map_y
```

All words are big-endian. The normal client packet submit function performs
framing and encryption. Repeating validation in the DLL closes the race between
an HTTP or CLI read and main-thread command execution.

The typed command sends immediately rather than emulating the native marker
animation. Both typed and user selections converge through outgoing `0x3F`
observation. Reject another selection once a native selection has started or a
typed selection has been submitted.

## REST and CLI

Add:

- `GET /clients/{client}/field-map`, returning observation metadata and
  `field_map: null | FieldMapState`;
- `POST /clients/{client}/field-map/select`, accepting only `revision` and
  `destination_index`;
- `darpc field-map select --pid <pid> <revision> <destination-index>`.

The direct CLI snapshot already provides the query path, so a second IPC query
message is unnecessary. Table snapshot output should summarize the field name,
revision, current node, selection, and destinations; JSON preserves the full
model.

Use `409 Conflict` for no active pane, stale revision, or an existing
selection, and `400 Bad Request` for an out-of-range destination index. The DLL
can still return `InvalidState` if the native pane closes before execution.

## Protocol and compatibility

Append the snapshot field, state-event discriminant, and command discriminant.
Bump the binary protocol minor version and gate all three additions on that
version. Preserve the existing snapshot and event ordering contract: every
connection receives a fresh snapshot containing the pane state at its event
boundary, followed only by later consecutive updates.

Add explicit maximums for the transferred field-map body and decoded strings.
The observed game-event body is at most `u16::MAX`, while the node count is a
single byte. Use a field-map-specific bounded transfer pool so worst-case
storage is deliberate rather than multiplying a maximum packet buffer by the
general event capacity.

## Implementation sequence

1. Add the shared model, snapshot field, reducer, codecs, protocol version, and
   round-trip and boundary tests.
2. Add supported-client RTTI and pane-layout constants, then extract the bounded
   registered-visible pane lookup used by dialog and field maps.
3. Add bounded `SFieldMap` parsing, tracker storage, post-dispatch activation,
   tick-based pane and native-selection observation, snapshot capture, and
   diagnostics.
4. Add exact outgoing `0x3F` observation and selection matching without using
   it as a close signal.
5. Add the typed command with daemon and DLL validation and the canonical
   big-endian packet builder.
6. Add daemon state, REST routes, OpenAPI models, SSE projections, direct CLI
   selection, and snapshot output.
7. Document field maps, events, hooks, protocol details, REST usage, and CLI
   usage in the mdBook.

## Verification

- Golden decode tests for the supplied 12-node `field001` packet and its exact
  `3F 00 00 0B C4 00 0E 00 0A` selection body.
- Malformed tests for truncation at every field, zero and maximum node counts,
  maximum strings, out-of-range current index, duplicate names, trailing bytes,
  transfer-pool exhaustion, and oversized protocol payloads.
- Pane lookup tests for null or changing dispatcher pointers, negative counts,
  count above capacity, excessive capacity, wrong RTTI, hidden panes,
  unregistered panes, and deferred deletion.
- State tests for open, replacement, native selection start, matched and
  unmatched outgoing selection, typed selection, close before execution,
  close after submission, reconnect snapshot, and no false completion event.
- Protocol compatibility tests for the prior version and the new snapshot,
  event, and command discriminants.
- Daemon API, OpenAPI, SSE, and CLI tests for null state, stale revision,
  invalid index, selection conflict, and deterministic full payloads.
- Host formatting, workspace tests, and Clippy with warnings denied.
- Native Windows verification in the available Parallels guest: open a field
  map, verify `active_field_map` only after the pane is registered and visible,
  start a real click and observe the delayed selection phases, confirm `0x3F`
  does not clear the state, confirm native pane removal does clear it, and run a
  typed selection against the same destination.
