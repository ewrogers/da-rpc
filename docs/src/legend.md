# Legend

The legend resource returns every mark shown in the character's self-look
profile:

```console
curl "http://127.0.0.1:2626/clients/ZiLo/legend"
```

The direct CLI exposes the same server refresh for one injected client:

```text
darpc legend --pid <pid>
darpc --output json legend --pid <pid>
```

```text
LegendSnapshot {
    pid: u32,
    received_tick_ms: u32,
    marks: Vec<LegendMark>,
}

LegendMark {
    text: string,
    tag: string,
    color: u8,
    icon: LegendIcon,
}
```

`icon` is one of `aisling`, `warrior`, `rogue`, `wizard`, `priest`, `monk`,
`heart`, `victory`, `none`, or `unknown`. `color` is the client color value.
The text and tag are decoded from the values supplied by the game server.

## Refresh behavior

Legend data arrives in the server's self-look message and is not pushed again
when a mark changes. For that reason, each request asks the game server for a
new self-look before returning. Requests for the same client are coalesced for
one second, so concurrent or rapidly repeated reads reuse the completed result
instead of sending repeated refresh packets. The endpoint returns `504 Gateway
Timeout` if the server does not answer within three seconds.

## Live events

Comparing a refreshed self-look with the retained legend produces these
Server-Sent Events (SSE):

| Event | Payload |
| --- | --- |
| `legend.mark_added` | `observation`, `mark` |
| `legend.mark_changed` | `observation`, `previous`, `current` |
| `legend.mark_removed` | `observation`, `mark` |

The game normally adds marks and does not remove them, but removal is modeled
so consumers can remain correct if a server ever returns a shorter legend.
Reread `/legend` after stream resynchronization.
