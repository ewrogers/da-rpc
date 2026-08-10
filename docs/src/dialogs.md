# NPC dialogs

daRPC can observe and interact with the merchant and pursuit windows used by
Mundanes. This includes ordinary conversation choices, text prompts, shop
lists, inventory pickers, and spell or skill pickers.

Dialog actions use the same native client methods as the game interface. They
run on the client main thread, update the visible window normally, and preserve
the client's response-pending behavior.

## Read the current dialog

```console
curl "http://127.0.0.1:2626/clients/ZiLo/dialog"
```

The response contains normal observation metadata and either the current
dialog or `null`:

```json
{
  "observation": {
    "pid": 6076,
    "instance_id": "890b3755fccd8d45b165bed41165457a",
    "revision": 42,
    "event_sequence": 38,
    "captured_tick_ms": 16209995,
    "updated_tick_ms": 16210012,
    "capture_duration_us": 2548,
    "world_generation": 3,
    "lifecycle": "in_game"
  },
  "dialog": {
    "revision": 7,
    "kind": "pursuit",
    "target": { "id": 4172 },
    "speaker": {
      "name": "Beggar",
      "sprite": 31,
      "sprite_type": "creature",
      "color": 0,
      "show_graphic": true
    },
    "content": "Can you spare a moment?",
    "response_pending": false,
    "navigation": {
      "previous": false,
      "next": true,
      "close": true
    },
    "interaction": {
      "type": "choices",
      "data": [
        { "index": 0, "text": "Yes." },
        { "index": 1, "text": "Not now." }
      ]
    }
  }
}
```

`kind` is `merchant` or `pursuit`. These names describe the two client dialog
families, not only shops and quests. A merchant-family dialog can also ask for
text or show player-owned items, spells, and skills.

The speaker sprite has its item or creature marker removed. `sprite_type`
preserves which kind of graphic the server supplied. `show_graphic` tells you
whether the game requested the portrait area.

## Interaction types

The `interaction.type` field tells a controller which response, if any, the
current page accepts:

| Type | Meaning |
| --- | --- |
| `message` | Informational page with navigation or close actions. |
| `choices` | Select one zero-based row from `data`. |
| `input` | Submit text using the supplied byte limit and optional surrounding text. |
| `items` | Select a server-provided item row. Some rows include a price, description, or available quantity. |
| `inventory` | Select one of the character's inventory rows. |
| `spells` | Select a spell row. |
| `skills` | Select a skill row. |
| `protected` | A client-managed protected form. It can be observed but not automated. |
| `unsupported` | The page is retained for observation, but daRPC does not know how to answer it. |

The fields in a row depend on what the server supplied. Optional values can be
missing. A `slot` is one-based when present, while a displayed `index` is
always zero-based.

## Start a conversation

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"target":"Beggar"}' \
  "http://127.0.0.1:2626/clients/ZiLo/interact"
```

Use the visible Mundane's case-insensitive name or object ID. For example, the
same request can select an object by ID:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"target":4172}' \
  "http://127.0.0.1:2626/clients/ZiLo/interact"
```

The target must be a Mundane in the current `/objects` state. This route does
not synthesize a click. It invokes the client's normal world-object interaction
method on the main thread.

## Answer a dialog

Every dialog action includes the `revision` returned by the current dialog.
This prevents an answer intended for one page from being applied after the
server has replaced it.

Select a choice, item, inventory entry, spell, or skill:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":7,"index":0,"quantity":1}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/select"
```

`quantity` defaults to `1`. It is checked against the current row when the
server supplied a limit.

Submit an input prompt:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":8,"input":"ZiLo"}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/input"
```

Input must be nonempty ASCII text and must fit the current dialog's byte limit.

Use the current navigation controls:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":8}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/previous"

curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":8}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/next"

curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":8}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/close"
```

Each accepts the same small revision body. The requested button must be
available in `navigation`. A dialog waiting for
the server has `response_pending: true`; further answers are rejected until a
new page arrives. Close remains available when the client permits it.

## Follow the current page

NPC conversations are server-driven. After every action, read `/dialog` again
and respond to the new `interaction` and `revision`. Do not assume that every
shop, pursuit, or character sees the same sequence of pages.

A typical purchase works like this:

1. Start the conversation with `/interact`.
2. Select the Buy choice from the returned `choices` page.
3. Read the new `items` page. Each row identifies its displayed `index` and
   can include its name, description, price in `value`, and available quantity.
4. Submit the chosen row with its current revision and desired quantity.
5. Confirm the result through `/items`, `/status`, and the event stream.

For example, selecting one item from revision 12 looks like this:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":12,"index":1,"quantity":1}' \
  "http://127.0.0.1:2626/clients/ZiLo/dialog/select"
```

Selling commonly adds a confirmation page:

1. Select Sell from the shopkeeper's opening `choices` page.
2. Read the returned `inventory` rows. Each row maps a displayed zero-based
   `index` to a one-based inventory `slot`.
3. Select the row and quantity. The server can replace it with a `choices`
   page containing the offered price.
4. Read that new page and revision, then select Yes or No by its displayed
   index.
5. Confirm that the inventory and gold state changed as expected.

The exact pages and wording belong to the game server. daRPC exposes what the
client is currently showing instead of assigning special meanings to choice
indexes.

## Revisions and errors

Dialog revisions are wrapping nonzero `u32` values scoped to one loaded DLL
instance. They increase when a dialog opens, changes, is submitted, or closes.
They do not need to be consecutive in an application.

A stale revision returns `409 Conflict` with `stale_dialog`. Other useful
conflict codes are `dialog_unavailable` and `dialog_pending`. A response that
does not fit the current page returns `400 Bad Request` with
`invalid_dialog_action`.

The daemon validates the revision first, then the DLL validates it again on
the client main thread. The second check closes the small race where the server
could replace a page while an HTTP command was waiting in the queue.

Normal action responses use the shared command model. `200 OK` means the native
call reached a final state during the HTTP wait. `202 Accepted` means it is
still queued. See [Using daRPC](web-api.md#native-command-results).

## Live dialog events

The client event stream publishes four dialog events:

```text
dialog.opened

DialogOpened {
    observation: EventObservation,
    dialog: DialogState,
}

dialog.changed

DialogChanged {
    observation: EventObservation,
    dialog: DialogState,
}

dialog.submitted

DialogSubmitted {
    observation: EventObservation,
    previous_revision: u32,
    dialog: DialogState,
    submission: DialogSubmission,
}

dialog.closed

DialogClosed {
    observation: EventObservation,
    previous: DialogState?,
    reason: client | server | world_changed | disconnected | replaced,
}
```

`DialogSubmission` has one of these JSON forms:

```json
{ "action": "select", "index": 0, "quantity": 1 }
{ "action": "input", "input": "ZiLo" }
{ "action": "previous" }
{ "action": "next" }
{ "action": "close" }
```

`dialog.submitted` describes a response sent through daRPC and includes the new
pending state. The following server page normally produces `dialog.changed`.
Actions made directly through the game interface still produce the resulting
changed or closed state, but are not labeled as daRPC submissions.

After `stream.resync_required`, reread `/dialog` along with any other resources
your tool uses. A fresh DLL connection includes the dialog cache in its initial
snapshot boundary.

## Timing and late attach

Incoming dialog packets are copied after the client accepts them. Parsing,
serialization, IPC, and web publication happen away from the hook. Dialog
packets use their own fixed-capacity storage so they cannot consume the main
high-volume event queue.

If daRPC is injected while a dialog is already open, it has not seen the packet
that created that page. `/dialog` can remain `null` until the existing window
closes or the server sends another dialog page. Open the conversation again
when a late-attached tool needs a complete baseline.
