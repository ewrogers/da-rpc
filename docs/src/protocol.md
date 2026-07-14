# daRPC protocol

`rpc.dll` and `rpcd.exe` communicate using a purpose-built binary protocol over
a process-specific Windows named pipe. The detailed wire format has not yet
been finalized.

`rpc.dll` is the pipe server and `rpcd.exe` is the pipe client. This ownership
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
`rpcd.exe` to reject a stale, unrelated, or unsupported endpoint before
accepting its state.
