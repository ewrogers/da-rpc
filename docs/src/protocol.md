# Binary protocol

`darpc.dll` communicates with a controller using a purpose-built binary
protocol. The codec is platform-independent; the Windows transport carries one
frame at a time over a process-specific named pipe.

All integers are unsigned and little-endian. The definitions below resemble
Rust for readability, but they describe serialized fields in order without
compiler padding. Rust memory layout is never copied directly onto the wire.

## Frame

Every frame begins with this fixed 20-byte header:

```rust,ignore
struct FrameHeader {
    magic: [u8; 4],       // offset 0: ASCII "DRPC"
    frame_version: u16,   // offset 4: currently 1
    message_type: u16,    // offset 6: MessageType discriminant
    sequence: u16,        // offset 8: per-sender wrapping counter
    flags: u16,           // offset 10: reserved, must be zero
    sender_tick_ms: u32,  // offset 12: wrapping Windows millisecond tick
    payload_len: u32,     // offset 16: bytes after this header
}
```

Payloads are limited to 65,536 bytes, making the largest complete frame 65,556
bytes. A receiver reads the header into a fixed-size buffer, validates it, and
only then reads the declared payload. A frame must contain exactly that payload
length. Truncation and trailing bytes are errors.

The frame version describes the envelope above and remains the simple integer
`1`. It is independent from the negotiated protocol version carried by
`Hello`.

## Protocol versions

A negotiated protocol version is one `u16` split into major and minor bytes:

```rust,ignore
let version: u16 = ((major as u16) << 8) | minor as u16;

const VERSION_1_0: u16 = 0x0100;
const VERSION_1_1: u16 = 0x0101;
const VERSION_2_0: u16 = 0x0200;
```

A major change may be incompatible. A minor change is additive and must remain
compatible with earlier minor versions in the same major line. Each peer
advertises an inclusive, continuous range and the controller selects the
highest version in the overlap. A peer must not advertise one continuous range
across an incompatible major boundary. No overlap rejects the connection.

The only currently supported version is 1.0 (`0x0100`). This encoding leaves
room for additive features without describing every change as an incompatible
"version 2."

## Message types

```rust,ignore
enum MessageType: u16 {
    Hello        = 1,
    HelloAck     = 2,
    Ping         = 3,
    Pong         = 4,
    EchoRequest  = 5,
    EchoResponse = 6,
}
```

The normal request direction is controller to DLL. Responses travel from DLL to
controller. `Hello` starts in the opposite direction because the DLL announces
its identity and capabilities immediately after a connection is established.

## Hello and HelloAck

```rust,ignore
struct Hello {
    protocol_min: u16,             // offset 0: inclusive 0xMMmm version
    protocol_max: u16,             // offset 2: inclusive 0xMMmm version
    dll_instance_id: [u8; 16],     // offset 4: one initialized DLL lifetime
    process_id: u32,                // offset 20
    process_creation_time: u64,     // offset 24: raw Windows FILETIME
    architecture: Architecture,     // offset 32: encoded as u8
    dll_version_major: u16,         // offset 33
    dll_version_minor: u16,         // offset 35
    dll_version_patch: u16,         // offset 37
    executable_fingerprint: [u8; 32], // offset 39: SHA-256
    layout_id: u32,                 // offset 71
} // 75 bytes

enum Architecture: u8 {
    X86    = 1,
    X86_64 = 2,
}

struct HelloAck {
    selected_version: u16,      // offset 0: negotiated 0xMMmm version
    dll_instance_id: [u8; 16],  // offset 2: copied from Hello
} // 18 bytes
```

The instance ID identifies one initialized DLL lifetime. The process ID and raw
creation time together distinguish PID reuse. The executable fingerprint,
architecture, and layout ID identify the supported client contract.

`HelloAck` copies the instance ID so an acknowledgement for one DLL instance
cannot complete another instance's handshake. Application messages are invalid
until this exchange completes:

```text
DLL                                      controller
 |---- Hello (version range, identity) ------>|
 |<--- HelloAck (selected version, identity) -|
 |                 ready                       |
```

These identity fields protect against stale or accidental connections. They are
not authentication against a malicious local process; transport-level peer
validation remains a separate responsibility.

## Ping, Pong, and echo

```rust,ignore
struct Ping {
    request_id: u32,  // offset 0
} // 4 bytes

struct Pong {
    request_id: u32,  // offset 0: copied from Ping
} // 4 bytes

struct EchoRequest {
    request_id: u32,  // offset 0
    text_len: u16,    // offset 4: UTF-8 byte count, at most 4,096
    text: utf8[text_len], // offset 6
}

struct EchoResponse {
    request_id: u32,  // offset 0: copied from EchoRequest
    text_len: u16,    // offset 4: copied from EchoRequest
    text: utf8[text_len], // offset 6: copied from EchoRequest
}
```

Request IDs wrap as `u32` and provide request/response correlation. They are
separate from frame sequence numbers because unsolicited events may be
interleaved and one future request may produce more than one frame.

## Ordering and time

Each sender maintains its own sequence counter for each connection. It starts
at zero, increments for every frame, and wraps from 65,535 to zero. A receiver
expects the same progression. A mismatch is a connection-level protocol error;
it does not silently resynchronize.

On Windows, the sender tick comes from
[`timeGetTime`](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegettime),
the millisecond clock also used by the supported client's dispatchers. It is
elapsed time since Windows started, not wall-clock time. It wraps every 2^32
milliseconds, about 49.71 days, so elapsed time is calculated as
`end.wrapping_sub(start)`.

The tick is diagnostic metadata for event comparison, sequencing, round-trip
time, and other observability. Millisecond resolution is sufficient for these
uses. daRPC does not change the system multimedia timer resolution merely to
stamp frames. The codec accepts the tick from its caller and has no Windows
dependency.

## Validation rules

Protocol handling is deliberately strict. The codec rejects invalid magic,
unsupported frame versions, unknown message types, nonzero flags, invalid
architecture values, invalid version ranges, invalid UTF-8, truncated fields,
oversized lengths, arithmetic overflow, and trailing bytes. Lengths are checked
before allocation. The session layer rejects unsupported negotiated versions,
invalid message order, mismatched instance IDs, and sequence gaps.

Malformed input returns a structured error. It must never panic, read past the
provided bytes, guess where another frame begins, or silently accept a value it
does not understand.

## Golden Hello frame

The codec tests use this exact 95-byte frame: a 20-byte header followed by a
75-byte `Hello` payload.

```text
44 52 50 43 01 00 01 00 34 12 00 00 12 34 56 78
4b 00 00 00 00 01 00 01 00 01 02 03 04 05 06 07
08 09 0a 0b 0c 0d 0e 0f 44 33 22 11 08 07 06 05
04 03 02 01 01 00 00 01 00 00 00 a0 a1 a2 a3 a4
a5 a6 a7 a8 a9 aa ab ac ad ae af b0 b1 b2 b3 b4
b5 b6 b7 b8 b9 ba bb bc bd be bf e5 02 00 00
```

The fixture uses protocol range 1.0 through 1.0, sequence `0x1234`, sender tick
`0x78563412`, process ID `0x11223344`, process creation time
`0x0102030405060708`, DLL version `0.1.0`, and layout ID `741`. Tests both encode
to these bytes and decode them back to the expected values.

## Accepted design decisions

- The 20-byte header is intentionally fixed. No current field justifies making
  it variable or larger.
- The 64 KiB payload cap is conservative and sufficient for known messages. It
  should be measured in real use and revisited only when a legitimate payload
  approaches it.
- Hello identity is sufficient for stale and accidental pipe connections. It is
  not treated as security authentication.
- The `u16` sequence supports ordering diagnostics now and can support a future
  bounded buffer or replay design. Request correlation remains a separate
  `u32` value.
- `timeGetTime` is the shared diagnostic clock because it matches the client and
  provides adequate millisecond resolution for round-trip and sequencing data.
- Unknown values, gaps, and trailing bytes remain strict errors unless real
  interoperability evidence shows that a specific rule should be relaxed.
- Echo text remains limited to 4 KiB. A future domain field with a known larger
  bound, such as message-board content near `0x8000` bytes, should receive an
  explicit field-specific limit or chunking design rather than silently lifting
  every string limit.

The implementation maps directly to this chapter: framing is in
`crates/protocol/src/frame.rs`, message fields are in `message.rs`, handshake and
sequence rules are in `session.rs`, and the exact fixture and malformed-input
coverage are under `crates/protocol/tests/`.
