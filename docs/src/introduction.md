# Introduction

daRPC, short for Dark Ages Remote Procedure Call, is a planned Rust workspace
for directly integrating tools with the Windows client of *Dark Ages*.

The project is in early development. This book records the intended
architecture and engineering constraints. It distinguishes design decisions
from implemented behavior so documentation does not imply support that does
not yet exist.

## Why direct client integration

Existing automation and monitoring tools commonly proxy the game's network
connection. That approach works, but it introduces several limitations:

- Packets must be decrypted, decoded, encoded, and encrypted again.
- Every connection gains an additional network hop and failure point.
- A tool cannot attach after the game client has already started.
- A proxy or bot failure can terminate the entire game session.
- Network packets cannot expose every local user interface state change or
  reproduce every local action.

daRPC instead works inside the client process. The injected library integrates
with the client's internal event dispatch system, where input and network
events pass through the user interface hierarchy. This boundary makes it
possible to observe, block, and inject events while leaving packet
serialization and encryption in the client.

Direct integration also makes client-only state observable. In addition to the
character and game world, daRPC can track panes, dialogs, selections, focus,
and other user interface state as the relevant structures and events become
understood.

## Portable access

The injected integration is necessarily Windows-specific, but its consumers do
not need to be. A local daemon translates the binary daRPC protocol into REST,
Server-Sent Events, and WebSocket APIs. Applications may run on the same
Windows computer or connect remotely from another operating system.
