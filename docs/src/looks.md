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
server-provided dialog text, including its separators.

The DLL intercepts only a bounded popup response while a typed look command is
pending. It publishes the exact text through the normal ordered event path and
suppresses that popup before the original client dispatcher runs. It does not
open and dismiss the dialog afterward. Unrelated message dialogs and popups
continue through the normal client behavior.

The game response contains no request identifier. Only one typed Look or
FarLook request may therefore be pending for a client; another request fails
with `rejected` until the first completes, expires, or is cancelled. A result
is transient and is not part of the retained client snapshot. Subscribe to
`GET /clients/{client}/events` before submitting the request when the result
must not be missed.
