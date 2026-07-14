# Safety and security

daRPC crosses several sensitive boundaries: injected code, client memory,
Windows application binary interfaces, local IPC, and potentially remote web
access. Those boundaries must remain explicit and small.

## Unsafe Rust and client memory

- Put unsafe operations behind audited interfaces and use explicit `unsafe`
  blocks.
- Document every unsafe block with the invariants that make it valid.
- Validate address, alignment, lifetime, size, and readability assumptions
  before constructing references from client memory.
- Model client layouts as version-specific. Never reuse offsets or relative
  virtual addresses for an unverified executable.
- Check pointer chains and lengths at every trust boundary.
- Define calling conventions, integer widths, packing, and ownership for every
  foreign or client ABI boundary.
- Do not unwind across a foreign function or hook boundary.

## Hooks and process stability

Hook installation and removal must be transactional and safe to repeat.
Original client behavior should be preserved unless a valid request explicitly
blocks an event. Injected code must not retain daemon-owned resources after a
disconnect or unload.

Substantial allocation, logging, IPC, or cleanup must not occur inside
time-sensitive hooks or under the Windows loader lock. A daemon, consumer, or
protocol failure must not terminate the game process.

## IPC and web boundaries

Named pipes should be local-only and restricted through an explicit security
descriptor. Protocol and API inputs require size limits, validation, bounded
queues, and useful errors. Logs must not expose credentials, authentication
material, private chat, or complete sensitive packet payloads by default.

Remote web access requires an explicit security model. Listening beyond the
local machine without authentication and transport protection should not be a
default configuration.

## Test data

Do not commit copyrighted client binaries, game assets, secrets, personal
data, or live-server captures containing private information. Prefer synthetic
fixtures for client layouts, state transitions, and protocol parsing.
