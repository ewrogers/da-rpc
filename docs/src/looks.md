# Looking at tiles

daRPC exposes the game's Look and FarLook requests as typed asynchronous
actions. Their result text can reveal the server-provided names of NPCs and
ground items without retaining or displaying the response as a native popup.

Look at the tile directly ahead:

```http
POST /clients/Eidolon/look
```

Look at a tile on the character's current map:

```http
POST /clients/Eidolon/far-look
Content-Type: application/json

{"position":{"x":40,"y":19}}
```

FarLook coordinates are zero-based nonnegative integers. They must fit the
game's unsigned 16-bit wire fields and the current map bounds. FarLook cannot
select another map.

Both routes return the usual native command status. The `command_id` correlates
the command with a later `look.result` Server-Sent Events (SSE) frame:

```text
event: look.result
id: 100
data: {"type":"look_result","data":{"observation":{"pid":6864,"instance_id":"...","revision":224,"event_sequence":100,"tick_ms":559274097},"command_id":7,"target":{"kind":"tile","x":40,"y":19},"text":"Light Belt\tLight Belt\tfior sal"}}
```

`observation` supplies the usual source, revision, and event-ordering metadata.
`target` includes `x` and `y` for both routes. Its `kind` is `ahead` for Look and
`tile` for FarLook. The DLL resolves an ahead target from its confirmed position
and facing when it submits the native packet. `text` preserves the
server-provided dialog text, including its separators. An empty response is
published with `text: ""`; it does not identify an entity or item.

The DLL claims a response only after the outgoing hook observes the exact
native request under that command's ID. It accepts a bounded popup with a
complete, exact length, publishes its text through the ordered event path, and
suppresses the popup before the original client dispatcher runs. If publication
fails, the popup remains visible. Other message subtypes pass through normally.

Only one look may own the response channel at a time. Another typed request
fails with `rejected` while a typed or manually initiated look is pending. A
single manual or raw look retains its normal popup behavior and releases the
channel when its response arrives.

Once a submitted typed request expires or is cancelled, the channel stays
quarantined. Overlapping manual/raw looks, duplicate or mismatched outgoing
requests, synthetic popup injection, malformed responses, failed packet
observation, and failed result publication also quarantine it. An active typed
request fails with `invalid_state` when ambiguity is detected, or `internal`
when publication fails. Later typed looks fail with `rejected`. Late replies
pass through to the game and cannot acquire a newer command's ID.

Neither a timer, a late reply, IPC reconnection, nor hook reinitialization
clears quarantine. Recovery requires a fresh game process with the DLL loaded
at startup. Unloading and reloading the DLL in a running process loses its
memory but does not prove that old server replies have drained. Attaching to
an already running client likewise cannot account for earlier look requests.

The game popup contains neither a request ID nor an entity ID. daRPC's
`command_id` and `target` are local correlation metadata, not server-confirmed
identity. An unsolicited popup with the same subtype and layout cannot be
distinguished from a look reply. Consumers resolving names must also verify
that the intended entity remained on the requested tile throughout the
observation interval and discard results across movement, replacement, or
lost observation continuity. Detected uncertainty must never become a name
cached for another entity.

A result is transient and is not part of the retained client snapshot. Subscribe to
`GET /clients/{client}/events` before submitting the request when the result
must not be missed.
