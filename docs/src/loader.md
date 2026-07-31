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

## Implementation boundaries

The loader keeps its current responsibilities in small, domain-specific
modules:

- `pe.rs` validates the selected `darpc.dll` file and produces a `DarpcDll`
  descriptor containing its canonical path and required lifecycle export
  relative virtual addresses (RVAs).
- `process.rs` owns target process handles, architecture inspection, process
  identity, and loaded-module discovery.
- `remote.rs` contains low-level remote allocation, memory writing, and remote
  thread execution.
- `remote_dll.rs` owns the Windows details for loading and unloading a DLL,
  including forwarded `LoadLibraryW` and `FreeLibrary` export resolution.
- `inject.rs` coordinates the daRPC lifecycle and decides when rollback is
  safe.

These are internal loader boundaries, not a general-purpose injection or
Portable Executable framework.

## Attach lifecycle

The implemented attach path:

1. Validates the selected DLL and its required lifecycle exports.
2. Opens and validates the target process as 32-bit x86.
3. Refuses to load a duplicate `darpc.dll`.
4. Loads the DLL and verifies its module base through target module
   enumeration.
5. Calls `darpc_initialize` with the supported ABI version.
6. Unloads the DLL when initialization completes with a non-success status.

If completion of the initialization thread is uncertain, the loader leaves the
DLL loaded rather than risk unloading code that may still be executing.

## Planned detach and launch

Detach will find the loaded module, call `darpc_shutdown`, and unload only after
shutdown succeeds. A failed or uncertain shutdown will leave the DLL loaded.

Launch will create the client suspended, validate its exact version, apply any
requested loader-owned startup patches, load and initialize `darpc.dll`, and
resume the primary thread only after those steps succeed. Hooks and trampolines
owned by daRPC remain the responsibility of `darpc_initialize` and
`darpc_shutdown`.
