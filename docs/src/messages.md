# Messages

daRPC normalizes recent chat and system messages so a tool does not need to
parse the punctuation the client uses for each channel.

| Use | Route or events |
| --- | --- |
| Read recent messages | `GET /clients/{client}/messages` |
| Send a message | `POST /clients/{client}/messages/send` |
| Filter retained history | `channels`, `since`, `skip`, and `count` |
| Watch new messages | [Message events](events.md#message-events) |

## Reading recent messages

```console
curl "http://127.0.0.1:2626/clients/ZiLo/messages"
```

Each message contains:

- `timestamp`, formatted as ISO 8601 in the daemon's local time and UTC offset
- `tick_ms`, the client's wrapping Windows millisecond tick
- `channel`
- Optional `sender` and `recipient`
- Cleaned message `text`

```text
Messages {
    messages: Message[],
}

Message {
    timestamp: string,
    tick_ms: u32,
    channel: MessageChannel,
    sender: string?,
    recipient: string?,
    text: string,
}
```

Retained history uses one of these channels:

```text
say, shout, whisper, guild, group, system, world
```

Spell chants are transient `message.chant` SSE events and are intentionally not
stored by `/messages`. This keeps spell and NPC command chants from crowding
ordinary conversation history.

Channel markers and participant punctuation shown by the game are removed from
the text. Empty messages are ignored. A world shout is stored once as `world`,
even though the client also renders a duplicate shout-form message.

## Sending messages

Send nearby speech, shouts, guild chat, group chat, or a whisper through the
selected client:

```console
curl -X POST "http://127.0.0.1:2626/clients/ZiLo/messages/send" \
  -H "content-type: application/json" \
  -d '{"channel":"whisper","recipient":"Eidolon","content":"hello"}'
```

The request body has `channel`, optional `recipient`, and `content` fields.
`channel` must be `say`, `shout`, `guild`, `group`, or `whisper`. A whisper
requires a recipient; every other channel rejects one. Content must contain
from 1 through 100 ASCII characters. Whisper recipients must contain from 1
through 15 ASCII characters without whitespace.

Guild and group messages use the game's directed-message packet with the
special recipients `!` and `!!`, respectively. Callers select `guild` or
`group`; they do not supply those markers as whisper recipients.

## Filtering and paging

Messages are sorted newest first. The route returns 20 records by default.

| Query | Meaning |
| --- | --- |
| `channels` | Comma-separated channels, such as `say,shout`. |
| `since` | Only messages strictly newer than this ISO 8601 timestamp. |
| `skip` | Skip this many matching records after sorting. Default `0`. |
| `count` | Return at most this many records. Default `20`, maximum `100`. |

Example:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/messages?channels=say,shout&since=2026-08-02T15:00:00-04:00&skip=0&count=20"
```

`since` is optional. When it is omitted, the route searches the retained
history without a time boundary.

## Live message events

The complete message payload and stream behavior are in
[Message events](events.md#message-events).

Each channel has its own SSE routing name:

```text
message.say
message.shout
message.chant
message.whisper
message.guild
message.group
message.system
message.world
```

All eight routes use the JSON discriminator `type: "message"`. The channel is
inside `data.channel`. Separate SSE names let a browser subscribe only to the
channels it cares about.

Message events do not contain the common state `observation` object. The SSE
`id` still provides ordering, and the subscription path identifies the client.
The daemon adds normal chat and system messages to REST history before
broadcasting them. It broadcasts chants without retaining them.

Some system messages also confirm spell results. In those cases the stream
contains both `message.system` and a semantic `spell.succeeded`, `spell.failed`,
or `spell.received` event. The original message remains available for display
and debugging. See [Spells](spells.md#cast-results) for correlation behavior.

## Retention

The daemon keeps at most 4,096 messages and 1 MiB of message text per DLL
instance. It removes the oldest messages first. History is held in memory and
is cleared when the daemon restarts or a new DLL instance replaces the old one.

If an SSE connection is interrupted, read `/messages` with a suitable `since`
value to recover recent conversation context. Chants and state events from
before the subscription are not replayed.

## Privacy

Message history can contain private whispers. daRPC does not write message text
to its normal logs, but any local program with access to the loopback API can
read retained messages. Run only consumers you trust.
