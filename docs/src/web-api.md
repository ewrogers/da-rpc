# Web API

`darpcd.exe` exposes standard web interfaces so applications do not need to
implement Windows injection, named-pipe IPC, or client-specific data layouts.

The planned interfaces have distinct roles:

| Interface | Primary role |
| --- | --- |
| REST | Discovery, current-state queries, configuration, and discrete actions. |
| Server-Sent Events | One-way real-time event and state-update streams. |
| WebSocket | Bidirectional real-time communication where interactive request and event traffic share a connection. |

## Aggregation

One daemon may manage multiple game clients. External models must identify the
source client without exposing raw process pointers or version-specific memory
layouts. A slow client or API consumer must not block unrelated connections.

API models should remain separate from both client memory layouts and binary
protocol models. This allows each boundary to evolve deliberately.

## Remote access

The daemon can support applications on another operating system because the
external boundary uses web protocols. Network listeners should default to the
least exposed practical interface. Any remote-listening mode must define
authentication, authorization, request limits, and transport-security
expectations before it is considered safe for general use.

SSE and WebSocket APIs must also define ordering, lag, disconnect, and replay
behavior. A connection must not silently imply that transient events generated
before subscription will be replayed.
