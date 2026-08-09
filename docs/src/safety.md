# Safety and security

Read this chapter before exposing the daemon beyond the local machine,
automating client actions, or changing injection, hook, and memory code.

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

`DllMain` does not provide a process-wide pause. The Windows loader lock
serializes loader activity, but unrelated client threads may continue executing.
Hook installation must therefore remain outside `DllMain` and occur through
explicit initialization.

Prepare and validate the complete hook plan before changing client code. Decode
the replaced instructions, allocate and populate every trampoline, and prepare
rollback data while client threads remain runnable. The commit phase should then
be as short as possible:

1. Suspend or enlist the affected threads.
2. Reject or safely redirect instruction pointers within a replaced range.
3. Change page protections and apply the complete patch set.
4. Flush the instruction cache and restore page protections.
5. Roll back every changed entry point if any commit step fails.
6. Resume every thread suspended by the transaction.

Do not suspend client threads across general allocation, logging, IPC, or other
unbounded initialization. A suspended thread may own a heap or synchronization
lock needed by the initialization thread. A hook-enabled launch should keep the
new process's primary thread suspended until the hook transaction commits. A
late attach requires the short transactional commit above.

Hook removal follows the same rules in reverse. Shutdown must prevent new hook
entries, drain in-flight callbacks, restore original code transactionally, and
prove that no thread can return through DLL-owned code before `FreeLibrary`.

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
