# Bulletin boards and player mail

The bulletin domain covers global boards, trade boards, guild boards, boards
opened from a world tile, and player mail. One native client session owns these
views. daRPC exposes the active session as structured state, observes
server-backed page and entry changes, and drives the native controls for
selection, scrolling, history, and composition.

Board articles and mail share list and entry concepts. Their composition rules
are different:

- A new board article has a subject and body.
- Player mail has a recipient, subject, and body.
- Replying to mail opens the native mail composer with the viewed author as a
  non-editable recipient when the client does the same.

The currently supported packet and native control layouts are based on the
7.41 client. Uninterpreted packet fields and operation status bytes remain raw
until further traces and client-code analysis establish their meaning.

## Opening a bulletin session

Global boards, guild boards, and mail begin with the server-provided section
list:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":0,"action":{"type":"open_server_list"}}' \
  "http://127.0.0.1:2626/clients/ZiLo/bulletin/actions"
```

The direct Windows command is:

```console
darpc bulletin open --pid 1234
```

A board in the world is opened with its tile coordinates. For example, tile
18,9:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"revision":0,"action":{"type":"open_world_board","x":18,"y":9}}' \
  "http://127.0.0.1:2626/clients/ZiLo/bulletin/actions"
```

```console
darpc bulletin world --pid 1234 18 9
```

Open actions use revision zero because no bulletin session is required yet.
They submit the client's canonical request. The resulting server packet and
native dialog determine the active state.

## Reading the active state

```console
curl "http://127.0.0.1:2626/clients/ZiLo/bulletin"
```

The response has observation metadata and a nullable `bulletin`. It is null
when no supported bulletin dialog is active. The same state appears as
`active_bulletin` in the complete client snapshot and in `darpc snapshot`.

Every active state contains:

```text
BulletinState {
    revision: u32,
    pending: BulletinOperation?,
    last_operation_result: BulletinOperationResult?,
    can_go_back: bool,
    can_go_forward: bool,
    view: BulletinView,
}
```

`view.type` identifies one of these shapes:

| Type | Meaning |
| --- | --- |
| `sections` | Global, guild, mail, or other server-provided sections. |
| `entries` | One board or mailbox list, including accumulated pages. |
| `entry` | One opened article or mail message. |
| `board_post` | The native new-article composer. |
| `player_mail` | The native mail composer. |

A section has a signed-independent `id`, display `name`, `kind`, and source.
Source is represented as `{ "kind": "global|clicked|mail|unknown", "raw": n }`
so an unrecognized client value is not discarded. Entry IDs are signed 16-bit
values because the client protocol also uses negative cursor sentinels.

Entry summaries contain the raw `flags`, author, month, day, and subject. A
full entry adds its body, `navigation_flags`, and `unknown_before_id`. The last
two fields are intentionally not decoded into guessed booleans.

The state also tracks the native control viewport as `position` and `maximum`.
This makes scroll state queryable after actions performed either through daRPC
or directly in the game UI.

## Revision-guarded actions

All actions other than opening require the current bulletin revision. Submit
them to:

```text
POST /clients/{client}/bulletin/actions
```

The request always has this envelope:

```json
{
  "revision": 12,
  "action": { "type": "next_entry" }
}
```

The supported action payloads are:

| `action.type` | Additional fields | Effect |
| --- | --- | --- |
| `open_server_list` | none | Request global, guild, and mail sections. |
| `open_world_board` | `x`, `y` | Activate the board at a world tile. |
| `open_section` | `section_id` | Request the newest entries for a section. |
| `select_section` | `section_id` | Select a visible native section row. |
| `open_entry` | `entry_id` | Request one visible or retained entry. |
| `select_entry` | `entry_id` | Select a visible native entry row. |
| `load_older` | none | Request the next older page. |
| `scroll` | `position` | Set the active native scroll control. |
| `back`, `forward` | none | Navigate the native bulletin dialog history. |
| `previous_entry`, `next_entry` | none | Request adjacent entries from an entry view. |
| `begin_board_post` | none | Open the native article composer. |
| `begin_player_mail` | none | Open the native mail composer. |
| `begin_reply` | none | Reply to the currently viewed message. |
| `update_board_post` | `subject`, `body` | Replace native board draft fields. |
| `update_player_mail` | `recipient`, `subject`, `body` | Replace native mail draft fields. |
| `submit_compose` | none | Send the current draft through the client protocol. |
| `delete_entry` | `entry_id` | Request deletion from the current board or mailbox. |
| `highlight_entry` | `entry_id` | Request the board-specific highlight operation. |
| `close` | none | Close the current native bulletin dialog. |

`select_section`, `select_entry`, `scroll`, `back`, `forward`, composition, and
close operate on the native UI. Open, page, entry, post, mail, delete, and
highlight operations submit canonical packets. Selection is distinct from
opening so callers can reproduce keyboard or mouse navigation without causing
a server request.

HTTP 409 means there is no active session, the revision is stale, or the
requested action is invalid for the current view. HTTP 400 means the action
payload or bounded text is invalid. HTTP 429, 503, or 504 means the bounded
live-client command path was busy, unavailable, or timed out.

## Lists, paging, and scrolling

An `entries` view retains pages already received for the current section.
`pagination` reports:

- `unknown` before paging state can be established.
- `ready` when another older-page request can be submitted.
- `loading` after the request is observed and before its response.
- `exhausted` after the server returns an empty page.

Pages are merged by entry ID, so a repeated boundary row does not create a
duplicate. Storage is bounded. `truncated: true` means the client delivered
more sections or entries than daRPC could retain. Callers should not interpret
truncation as the server's end of history.

Loading older entries and scrolling are separate actions. `load_older` extends
server-backed state; `scroll` changes the native viewport. An application can
load until `exhausted`, then use the retained entries and viewport to drive its
own UI.

Examples:

```console
darpc bulletin open-section --pid 1234 12 4
darpc bulletin older --pid 1234 13
darpc bulletin scroll --pid 1234 14 6
darpc bulletin open-entry --pid 1234 15 4280
darpc bulletin next --pid 1234 16
```

## Composing and mutation

Composition deliberately uses two steps. `begin_*` or `reply` opens the native
composer. `update_*` writes its currently queryable draft fields. A draft is
only the unsent content visible in that composer. `submit_compose` sends it.

```console
darpc bulletin compose-post --pid 1234 20
darpc bulletin update-post --pid 1234 21 "Market day" "Trading begins at noon."
darpc bulletin submit --pid 1234 22
```

```console
darpc bulletin compose-mail --pid 1234 30
darpc bulletin update-mail --pid 1234 31 Mileth "Hello" "Meet me by the inn."
darpc bulletin submit --pid 1234 32
```

Recipient text is at most 15 ASCII bytes, subject text is at most 60 ASCII
bytes, and body text is at most 3000 ASCII bytes. NUL bytes are rejected. These
bounds reflect the client protocol and native controls. Empty fields remain
valid while a draft is being edited; server acceptance is reported separately.

Deletion and highlighting are server-backed requests. An observed outgoing
request becomes `pending`; a later server result clears it and sets
`last_operation_result`. `raw_status` is preserved without assigning success
or failure semantics that have not been confirmed. An optional server message
is exposed verbatim as decoded text. A command submission therefore means the
client performed the requested action, not that the game server accepted it.

## Live events

Subscribe through `GET /clients/{client}/events`:

| SSE event | JSON type | Meaning |
| --- | --- | --- |
| `bulletin.opened` | `bulletin_opened` | A supported bulletin session became active. |
| `bulletin.changed` | `bulletin_changed` | Its view, selection, page, viewport, draft, or navigation state changed. |
| `bulletin.action_submitted` | `bulletin_action_submitted` | The client's outgoing bulletin request was observed. |
| `bulletin.operation_result` | `bulletin_operation_result` | The server returned a status for a mutation. |
| `bulletin.closed` | `bulletin_closed` | The native bulletin session closed. |

Opened, changed, and operation-result events carry `bulletin`. Submitted events
carry the operation and a nullable bulletin because an opening action may
precede active UI state. Closed events carry the prior state as `previous`.
After an event-stream resynchronization, reread
`GET /clients/{client}/bulletin`.

## Ownership and observation boundaries

The injected DLL owns bulletin state independently of the daemon. Incoming and
outgoing packets are copied into fixed-capacity, pointer-free storage. Native
dialog pointers are rediscovered and validated on the client main thread; they
are never sent over IPC or retained as public state. UI polling is bounded and
runs only while bulletin tracking is active.

Player mail can contain private content. Keep the daemon on its default
loopback listener, limit API access to trusted local consumers, and do not log
complete bulletin snapshots or event payloads by default.

The implementation recognizes exact bulletin dialog and control layouts for
the supported executable fingerprint. It fails closed when the session,
dialog, control type, list bounds, or requested revision does not match. Future
validation should confirm additional server status values, navigation flag
bits, unusual page boundaries, guild-board permissions, and successful and
failed mutations against a live client.
