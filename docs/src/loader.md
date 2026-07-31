# `loader.exe`

`loader.exe` is the 32-bit x86 entry point for starting or attaching daRPC. It
owns two workflows:

- Attach, inspect, and detach `darpc.dll` in an already-running compatible
  client. This workflow is implemented.
- Launch a compatible client and arrange for `darpc.dll` to be loaded before
  normal execution. This workflow is planned for M3.

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
library. Before calling a lifecycle export by its validated relative virtual
address, the observed loaded-module path must match the selected DLL.

## Attach commands

The M2 command surface is:

```text
loader [--json] inspect <pid>
loader [--json] attach <pid> <dll-path>
loader [--json] detach <pid> <dll-path>
```

Human mode writes progress diagnostics to standard error and one final result
to standard output. `--json` keeps the same diagnostics on standard error and
writes exactly one JSON result to standard output.

A successful JSON result always contains `ok`, `command`, `pid`,
`creation_time`, `changed`, `darpc_loaded`, and `module_base`.
`creation_time` is a decimal string so its 64-bit Windows `FILETIME` value
remains exact in JavaScript consumers. `module_base` is an x86 address number
or `null`.

An error result contains `ok`, `command`, and an `error` object with stable
`kind` and `message` fields. `command` is `null` when argument parsing could
not identify a command. The process exit codes are:

| Exit | Error kind |
| ---: | --- |
| 2 | `invalid_arguments` |
| 3 | `unsupported_platform` |
| 4 | `invalid_dll` |
| 5 | `process_missing` |
| 6 | `process_exited` |
| 7 | `access_denied` |
| 8 | `wrong_architecture` |
| 9 | `already_loaded` |
| 10 | `timeout` |
| 11 | `initialization_failed` |
| 12 | `shutdown_failed` |
| 13 | `remote_operation_failed` |
| 14 | `internal` |

## Implementation boundaries

The loader keeps its current responsibilities in small, domain-specific
modules:

- `pe.rs` validates the selected `darpc.dll` file and produces a `DarpcDll`
  descriptor containing its canonical path and required lifecycle export
  relative virtual addresses (RVAs).
- `process.rs` owns target process handles, architecture inspection, process
  identity, and loaded-module discovery.
- `remote.rs` contains low-level remote allocation, memory writing, and remote
  thread execution, including the bounded wait.
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
7. Re-inspects the target module list before reporting success.

If completion of the initialization thread is uncertain, the loader leaves the
DLL loaded rather than risk unloading code that may still be executing.

## Detach lifecycle

The implemented detach path:

1. Validates the selected DLL and opens the x86 target.
2. Finds the loaded `darpc.dll` through target module enumeration and verifies
   that its path matches the selected DLL.
3. Calls `darpc_shutdown(0)` at the validated shutdown relative virtual
   address.
4. Calls `FreeLibrary` only after shutdown returns success.
5. Re-inspects the target and reports success only when `darpc.dll` is absent.

A shutdown failure or uncertain completion leaves the DLL loaded. Every remote
thread wait is bounded to 10 seconds. A timeout is reported separately and the
loader avoids cleanup that could free memory or code still in use by the
target.

Command repetition is deliberate:

- `inspect` is read-only and reports `changed=false`.
- A repeated `attach` fails with `already_loaded` and does not create another
  DLL instance.
- A repeated `detach` succeeds with `changed=false` when the DLL is already
  absent.

## Verification

The Windows integration test builds a real x86 loader, DLL, and inert target,
then exercises the complete lifecycle and required failure classifications. A
small controllable fixture DLL is used only for initialization failure,
shutdown failure, and timeout cases.

From a Windows PowerShell shell:

```powershell
cargo build `
  -p loader -p rpc-dll -p injection-target -p loader-fixture-dll `
  --target i686-pc-windows-msvc

./tools/injection-target/test-loader.ps1 `
  -TargetDir ./target/i686-pc-windows-msvc/debug
```

The same sequence runs in the Windows workflow.

## Planned launch

Launch will create the client suspended, validate its exact version, apply any
requested loader-owned startup patches, load and initialize `darpc.dll`, and
resume the primary thread only after those steps succeed. Hooks and trampolines
owned by daRPC remain the responsibility of `darpc_initialize` and
`darpc_shutdown`.
