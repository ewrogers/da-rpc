# `loader.exe`

`loader.exe` is the 32-bit x86 entry point for starting or attaching daRPC. It
supports two workflows:

- Launch a compatible client and arrange for `darpc.dll` to be loaded.
- Inject `darpc.dll` into an already-running compatible client.

Late injection is what allows daRPC to attach without requiring the game
session to have started behind a proxy.

## Validation

A matching window or process name only identifies a candidate. Before
injection, the loader must validate the target architecture, executable
identity, supported version, and whether `darpc.dll` is already loaded or still
initializing.

Injection should fail closed when compatibility cannot be established. A
failed attempt must not terminate or leave the game client partially modified.
Repeated requests should be safe and must not load duplicate copies of the
library.
