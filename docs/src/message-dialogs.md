# Message dialogs

Message dialogs are the small native windows opened by actions such as
`sense` and `look`. They are `WindowMessageDialogPane` instances, not
merchant or pursuit dialogs, and do not use the NPC [dialog](dialogs.md) model.

## Reading current dialogs

```text
GET /clients/{client}/message-dialogs
```

The response contains observation metadata and a state with a wrapping
`revision` and a `dialogs` array. Each dialog has an opaque `id`, nullable
`text`, and a `truncated` flag. IDs contain no client addresses. Text is
capped at 4096 client bytes.

## Dismissing a dialog

```text
POST /clients/{client}/message-dialogs/dismiss

{"revision":7,"id":3}
```

The direct client provides the same operation:

```text
darpc message-dialog dismiss --pid 1234 7 3
```

The DLL revalidates the revision, ID, pane type, registration, and visibility
on the client main thread before calling the native close operation. A stale
revision fails closed.

## Events

`message_dialogs.changed` carries observation metadata and the complete
current state whenever a message dialog opens, changes, or closes. An empty
`dialogs` array means none remains. After an SSE resync, reread the resource.

The DLL observes applicable `SMessage` packets and, while a dialog is active,
checks the pane collection at most once every 100 milliseconds for native
closes.
