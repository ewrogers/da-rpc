# Raw packets

daRPC exposes a low-level escape hatch for protocol research and testing:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"direction":"client","command":"0x7E","payload":"00 03 02"}' \
  "http://127.0.0.1:2626/clients/ZiLo/raw/send"
```

> **Warning:** Raw packets bypass daRPC's normal game-specific validation.
> Incorrect commands, lengths, fields, or state assumptions can disconnect the
> session, corrupt client state, crash the game client, or trigger server-side
> failures. Use this interface only when the exact packet format and required
> client state are known.

The JSON request has three required string fields:

```json
{
  "direction": "client",
  "command": "0x7E",
  "payload": "00 03 02"
}
```

| Field | Format |
|---|---|
| `direction` | `client` sends a client packet to the connected game server. `server` dispatches a synthetic server packet inside the game client. |
| `command` | Exactly `0x` followed by two hexadecimal digits. This becomes the first byte of the packet body. |
| `payload` | Zero or more two-digit hexadecimal bytes separated by ASCII whitespace. The maximum is 255 bytes. Use an empty string for no payload. |

The endpoint joins `command` and `payload`; it does not accept encrypted wire
frames, transport headers, lengths, or checksums. For the example above, the
native body is `7E 00 03 02`.

## API examples

Send a custom client packet to the game server:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"direction":"client","command":"0x7E","payload":"00 03 02"}' \
  "http://127.0.0.1:2626/clients/ZiLo/raw/send"
```

Dispatch a custom server packet to the client:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"direction":"server","command":"0x3A","payload":""}' \
  "http://127.0.0.1:2626/clients/ZiLo/raw/send"
```

Like other native actions, a raw send returns a command status. The action is
queued and executed on the client's main thread. A `200 OK` response means it
reached a terminal command state; `202 Accepted` means the returned command ID
can be polled through the normal command-status route.

## Direct CLI

The direct client provides the same operation for one injected process:

```text
darpc.exe raw send --pid 3780 client 0x7E "00 03 02"
darpc.exe raw send --pid 3780 server 0x3A
darpc.exe --output json raw send --pid 3780 client 0x7E "00 03 02"
```

Quote a nonempty payload so it is passed as one argument. Omitting the payload
means zero payload bytes.

## Direction behavior

`client` calls the supported client's normal plaintext packet-submission
function. The body enters the same outbound observation hook as native client
actions before the game applies its transport framing and encryption.

`server` creates the minimal decoded server-event shape expected by the
supported client and calls its central event dispatcher. It does not contact
the game server. The synthetic event enters daRPC's normal server-event hook,
so any recognized state update is observed just like a received packet.

daRPC validates only the hexadecimal syntax, one-byte command, payload bound,
supported client build, and command queue. It intentionally cannot validate
the semantics of an arbitrary packet. Non-loopback API access makes this
surface especially sensitive because the HTTP API has no authentication or
Transport Layer Security (TLS).
