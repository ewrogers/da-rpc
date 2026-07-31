# daRPC protocol

`darpc.dll` and `darpcd.exe` communicate using a purpose-built binary protocol over
a process-specific Windows named pipe. The detailed wire format has not yet
been finalized.

`darpc.dll` is the pipe server and `darpcd.exe` is the pipe client. This ownership
lets the injected library remain discoverable even when the daemon is absent.

## Message roles

The protocol is expected to support these message roles:

- Handshake and protocol-version negotiation.
- Client identity and supported-layout information.
- Request and response operations.
- Complete state snapshots.
- Ordered state updates and real-time events.
- Validated client actions.
- Errors, cancellation, and graceful disconnects.

These are roles, not a commitment to specific message names or encodings.

## Wire requirements

- Frame every message with an explicit size and enforce conservative limits
  before allocating.
- Define byte order, integer widths, discriminants, optional fields, and string
  encoding.
- Never use Rust's in-memory layout as the wire representation.
- Version the protocol and explicitly negotiate or reject incompatible peers.
- Treat all input as untrusted, including input from the local computer.
- Keep parsing separate from dispatch so malformed messages cannot
  desynchronize later frames.
- Use bounded queues and deliberate backpressure. IPC must not block a game
  hook or grow memory without limit.
- Associate the pipe and handshake with the expected process identity.

The handshake must establish enough identity and compatibility information for
`darpcd.exe` to reject a stale, unrelated, or unsupported endpoint before
accepting its state.

## Frame timing and sequence metadata

Every frame carries two diagnostic fields in its fixed header:

- `sequence: u16` starts at zero for each sender on each connection and advances
  with wrapping addition for every frame. Each direction has its own sequence.
- `sender_tick_ms: u32` records the sender's Windows uptime tick immediately
  before the transport sends the frame.

The Windows transport obtains `sender_tick_ms` from
[`timeGetTime`](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegettime),
the same millisecond clock used by the supported game client. The value wraps
about every 49.71 days, so elapsed-time calculations always use wrapping
subtraction. It is neither wall-clock time nor an expiration or authorization
source. daRPC does not change the Windows multimedia timer resolution merely for
protocol timestamps.

The `darpc-protocol` codec remains platform-independent: its caller supplies the
sequence and tick values. The Windows dependency belongs to the transport added
in M6. A receiver captures its own tick after reading a complete frame, allowing
same-machine latency and log sequencing to be compared across processes.

Sequence is diagnostic ordering metadata, not request correlation. Requests
carry a separate wrapping `u32` request ID that the corresponding response
echoes. This keeps correlation stable when one request produces multiple frames
or unsolicited events are interleaved.
