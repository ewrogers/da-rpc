# Binary protocol

M5 defines the platform-independent daRPC wire format. M6 will carry these
frames over the process-specific Windows named pipe. The format is manual and
little-endian; Rust struct layout is never copied onto the wire.

## Frame

Every frame starts with this fixed 20-byte header:

| Offset | Size | Field | Rule |
| ---: | ---: | --- | --- |
| 0 | 4 | Magic | ASCII `DRPC` |
| 4 | 2 | Frame version | `u16`, currently `1` |
| 6 | 2 | Message type | `u16`, listed below |
| 8 | 2 | Sequence | Per-sender wrapping `u16` |
| 10 | 2 | Flags | Reserved; must be zero |
| 12 | 4 | Sender tick | Wrapping milliseconds as `u32` |
| 16 | 4 | Payload length | `u32`, at most 65,536 bytes |

The maximum complete frame is 65,556 bytes. A receiver can read the header into
a fixed-size buffer, validate it, and only then read the declared payload. A
complete frame must contain exactly that payload length; truncation and trailing
bytes are errors.

Frame version and negotiated protocol version are separate. Frame version
describes the envelope above. Protocol version describes message semantics.
Both are currently `1`.

## Messages

| Value | Message | Normal direction | Payload size |
| ---: | --- | --- | ---: |
| 1 | `Hello` | DLL to controller | 75 bytes |
| 2 | `HelloAck` | Controller to DLL | 18 bytes |
| 3 | `Ping` | Controller to DLL | 4 bytes |
| 4 | `Pong` | DLL to controller | 4 bytes |
| 5 | `EchoRequest` | Controller to DLL | 6 to 4,102 bytes |
| 6 | `EchoResponse` | DLL to controller | 6 to 4,102 bytes |

### Hello

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 2 | Minimum supported protocol version |
| 2 | 2 | Maximum supported protocol version |
| 4 | 16 | Opaque DLL instance ID |
| 20 | 4 | Process ID |
| 24 | 8 | Raw Windows process creation `FILETIME` |
| 32 | 1 | Architecture: `1` is x86, `2` is x86-64 |
| 33 | 2 | DLL major version |
| 35 | 2 | DLL minor version |
| 37 | 2 | DLL patch version |
| 39 | 32 | Executable SHA-256 fingerprint |
| 71 | 4 | Client layout ID |

The version range is inclusive, nonzero, and ordered. The controller selects the
highest version in the overlap with its own supported range. No overlap rejects
the connection.

The instance ID identifies one initialized DLL lifetime. The process ID and raw
creation time together distinguish PID reuse. `HelloAck` contains the selected
`u16` protocol version followed by the same 16-byte instance ID, which prevents
an acknowledgement for one DLL instance from completing another handshake.

No `Ping`, `EchoRequest`, or future application message is valid until this
exchange completes:

```text
DLL                                      controller
 |---- Hello (supported range, identity) ---->|
 |<--- HelloAck (selected version, identity) -|
 |                 ready                       |
```

### Ping and echo

`Ping` and `Pong` contain only a wrapping `u32` request ID. `Pong` copies the
request ID from its `Ping`.

Echo payloads contain a wrapping `u32` request ID, a `u16` UTF-8 byte length,
and that many UTF-8 bytes. Text is limited to 4,096 bytes. `EchoResponse` copies
both the request ID and text from its `EchoRequest`.

Request IDs provide request/response correlation. They are deliberately
separate from frame sequence numbers because unsolicited events may be
interleaved and one future request may produce more than one frame.

## Ordering and time

Each sender maintains its own sequence counter for each connection. It starts
at zero, increments for every frame, and wraps from 65,535 to zero. A receiver
expects the same progression. A mismatch is a connection-level protocol error;
it does not silently resynchronize.

On Windows, M6 supplies the sender tick using
[`timeGetTime`](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegettime),
which is compatible with the clock used internally by the supported client. It
is elapsed time since Windows started, not wall-clock time. It wraps every
2^32 milliseconds, about 49.71 days, so elapsed time is always calculated as
wrapping subtraction: `end.wrapping_sub(start)`.

The timestamp is diagnostic metadata for sequencing and same-machine latency.
It is not an expiration, authorization, or high-resolution timing source. daRPC
does not change the system multimedia timer resolution merely to stamp frames.
The codec accepts a tick from its caller and has no Windows dependency.

## Validation rules

The decoder fails closed on invalid magic, unsupported frame or protocol versions,
unknown message types, nonzero flags, invalid architecture values, invalid
version ranges, invalid UTF-8, truncated fields, oversized lengths, arithmetic
overflow, and any trailing bytes. Lengths are checked before allocation.

Malformed input returns a structured error. It must never panic, read past the
provided bytes, or cause the receiver to guess where the next frame begins.

## Golden Hello frame

This is the exact 95-byte fixture used by the tests. It contains a 20-byte
header and a 75-byte `Hello` payload.

```text
44 52 50 43 01 00 01 00 34 12 00 00 12 34 56 78
4b 00 00 00 01 00 01 00 00 01 02 03 04 05 06 07
08 09 0a 0b 0c 0d 0e 0f 44 33 22 11 08 07 06 05
04 03 02 01 01 00 00 01 00 00 00 a0 a1 a2 a3 a4
a5 a6 a7 a8 a9 aa ab ac ad ae af b0 b1 b2 b3 b4
b5 b6 b7 b8 b9 ba bb bc bd be bf e5 02 00 00
```

The fixture uses sequence `0x1234`, sender tick `0x78563412`, process ID
`0x11223344`, process creation time `0x0102030405060708`, DLL version `0.1.0`,
and layout ID `741`. The tests both encode to these bytes and decode these bytes
back to the expected values.

## Human review checklist

These are the choices worth vetting before M6 makes the protocol live:

- Is a fixed 20-byte header clear enough to inspect and extend?
- Is a 64 KiB payload ceiling comfortably above expected state messages while
  still conservative for untrusted local input?
- Are the Hello identity fields sufficient to reject the wrong process,
  executable, architecture, layout, protocol version, and DLL lifetime?
- Is a per-sender `u16` sequence appropriate for ordering diagnostics while a
  separate `u32` request ID handles correlation?
- Is `timeGetTime` the right shared diagnostic clock given its coarse resolution
  and documented wrap behavior?
- Is strict rejection of gaps, unknown values, and trailing bytes preferable to
  permissive forward compatibility at this stage?
- Is UTF-8 with a 4,096-byte echo limit an acceptable first variable-length
  field?

The easiest code-level audit is to compare this chapter with
`crates/protocol/src/frame.rs`, `message.rs`, and the golden fixture in
`crates/protocol/tests/golden.rs`. The malformed-input and state-machine cases
are in `codec.rs` and `session.rs` beside that fixture.
