# `loader.exe`

`loader.exe` is the 32-bit x86 entry point for starting or attaching daRPC. It
owns two implemented workflows:

- Attach, inspect, and detach `darpc.dll` in an already-running compatible
  client.
- Launch a compatible client and arrange for `darpc.dll` to be loaded before
  its primary thread resumes.

Late injection is what allows daRPC to attach without requiring the game
session to have started behind a proxy.

## Validation

A matching window or process name only identifies a candidate. Before
injection, the loader must validate the target architecture, executable
identity, supported version, and whether `darpc.dll` is already loaded or still
initializing.

Injection should fail closed when compatibility cannot be established. A
failed attach must not terminate an existing client. A failed launch terminates
only the child created by that loader invocation before it can run normally.
Repeated requests should be safe and must not load duplicate copies of the
library. Before calling a lifecycle export in an existing process by its
validated relative virtual address, the observed loaded-module path must match
the selected DLL.

The supported client is Dark Ages 7.41 with a 3,112,960-byte executable and
SHA-256 fingerprint
`054A5D6ADC56099C6BFD9D2A58675AFF62DC788B63209A3D906492F5B89E96C6`.
Both attach and launch canonicalize and fingerprint the executable before any
remote operation or child creation. Any other executable fails with
`unsupported_client`.

The caller supplies the intended DLL path explicitly. The loader canonicalizes
that path, requires the file name `darpc.dll`, validates its x86 Portable
Executable headers, and resolves the required lifecycle exports. It does not
search the current directory or select among multiple DLLs implicitly.

Repository-owned integration targets can opt into an unsupported-client bypass
with `DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT=1`. This escape hatch exists
only in debug builds. Release builds ignore it and always require the exact
supported fingerprint.

## Commands

The M4 command surface is:

```text
loader [--json] inspect <pid>
loader [--json] attach <pid> <dll-path>
loader [--json] detach <pid> <dll-path>
loader [--json] launch <executable-path> <dll-path> [-- <argument>...]
```

Arguments after the `--` separator are forwarded to the launched executable.
The executable path is also supplied explicitly as its `argv[0]`.

Human mode writes progress diagnostics to standard error and one final result
to standard output. `--json` keeps the same diagnostics on standard error and
writes exactly one JSON result to standard output.

A successful JSON result always contains `ok`, `command`, `pid`,
`creation_time`, `changed`, `darpc_loaded`, and `module_base`.
`creation_time` is a decimal string so its 64-bit Windows `FILETIME` value
remains exact in JavaScript consumers. `module_base` is an x86 address number
or `null`.

An error result contains `ok`, `command`, `pid`, and an `error` object with
stable `kind` and `message` fields. `command` is `null` when argument parsing
could not identify a command. `pid` identifies a launch-owned child when
failure occurred after process creation and is otherwise `null`. The process
exit codes are:

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
| 15 | `launch_failed` |
| 16 | `unsupported_client` |

## Implementation boundaries

The loader keeps its current responsibilities in small, domain-specific
modules:

- `pe.rs` validates the selected `darpc.dll` file and produces a `DarpcDll`
  descriptor containing its canonical path and required lifecycle export
  relative virtual addresses (RVAs).
- `process.rs` owns target process handles, architecture inspection, process
  identity, executable-path discovery, and loaded-module discovery.
- `darpc-client-741` owns the exact supported executable fingerprint and its
  canonical-path validation.
- `launch.rs` owns suspended process creation, Windows argument quoting,
  primary-thread resumption, and child-only failure cleanup.
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
3. Resolves and validates the target's exact Dark Ages 7.41 executable.
4. Refuses to load a duplicate `darpc.dll`.
5. Loads the DLL and verifies its module base through target module
   enumeration.
6. Calls `darpc_initialize` with the supported ABI version.
7. Unloads the DLL when initialization completes with a non-success status.
8. Re-inspects the target module list before reporting success.

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

## Launch lifecycle

The implemented launch path:

1. Validates the selected DLL before creating a process.
2. Resolves and validates the exact Dark Ages 7.41 executable before creating
   a process.
3. Uses the executable parent directory as the child working directory.
4. Creates the child with `CREATE_SUSPENDED`, general handle inheritance
   disabled, and no copied standard handles.
5. Validates the child as x86 and records its creation time without requiring
   module enumeration before Windows user-mode loader startup.
6. Loads `darpc.dll` and calls `darpc_initialize` while the primary thread
   remains suspended.
7. Resumes the primary thread only after initialization returns success.
8. Terminates and waits for only that owned child if any pre-resume operation
   fails.

The loader and launched child are the same architecture and run in the same
Windows session. The suspended-load path therefore uses the shared x86
`kernel32.dll` `LoadLibraryW` address before target module enumeration becomes
available. Native Windows tests exercise this boundary. Once the process has
started normally, later inspection and detach operations use target module
enumeration as usual.

Windows quoting doubles backslashes where required around embedded quotes and
at quoted argument boundaries. Every argument is quoted independently, so
spaces, empty values, quotes, trailing backslashes, and Unicode are preserved.

## Verification

The Windows integration test builds a real x86 loader, DLL, and inert target,
then exercises attach, detach, suspended launch, and required failure
classifications. A small controllable fixture DLL is used only for
initialization failure, shutdown failure, and timeout cases.

From a Windows PowerShell shell:

```powershell
cargo build `
  -p loader -p rpc-dll -p injection-target -p loader-fixture-dll `
  --target i686-pc-windows-msvc

./tools/injection-target/test-loader.ps1 `
  -TargetDir ./target/i686-pc-windows-msvc/debug
```

The launch checks confirm that initialization was logged before the target
entered `main`, arguments and the executable working directory were preserved,
handles were not inherited, a normal process can exit, and a failed
initialization leaves no suspended child. The same sequence runs in the
Windows workflow.

The live-client checks are intentionally local and require a legally obtained
Dark Ages 7.41 installation. They never copy the executable, enter credentials,
or record game data. Build the x86 artifacts, close every running client, and
run:

```powershell
./tools/test-client-741.ps1 `
  -ClientPath "C:\path\to\Darkages.exe" `
  -TargetDir ./target/i686-pc-windows-msvc/debug
```

This script proves unsupported-client rejection, two independent controlled
target processes, live late attach, live suspended launch, duplicate detection,
lifecycle logging, unload, and client liveness after unload. It force-stops only
the client processes it starts, so it is not evidence of normal interactive
exit behavior.

When orchestrating a Parallels guest from macOS, do not use a Parallels Tools
remote command as the parent of a live client behavioral-acceptance launch.
Remote execution remains appropriate for building, controlled-target tests,
inspection, and attaching to a client the user started interactively. Run the
launch acceptance command from PowerShell opened inside the Windows desktop, or
use an equivalent limited-privilege process created from the active interactive
logon token. Let the client perform its normal exit rather than terminating it
from the harness. Dark Ages 7.41 login redirect behavior was verified with this
interactive-token method; the same login was unreliable when the client was a
descendant of the Parallels Tools remote-command process.

Complete the behavioral acceptance check manually and privately:

1. Close every running client. Start the client directly, log in, move, open
   and close representative user interface panels, and exit normally.
2. With no client running, use `loader launch <client-path> <dll-path>`. Repeat
   the same actions and exit normally with the inert DLL still loaded.
3. With no client running, start the client directly, use
   `loader attach <pid> <dll-path>`, repeat the same actions, and exit normally
   with the inert DLL still loaded.
4. Record whether all three runs behaved the same. Do not put credentials,
   private chat, or packet data in the record.

The stock client currently enforces a single instance with a startup mutex, so
these live checks must run sequentially. Until a later milestone implements the
planned startup patch, multi-process loader behavior is verified with the
repository-owned controlled target instead of concurrent live clients.

Future loader-owned startup patches fit between suspended process validation
and DLL initialization. Hooks and trampolines owned by daRPC remain the
responsibility of `darpc_initialize` and `darpc_shutdown`.
