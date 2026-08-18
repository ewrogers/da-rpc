# `loader.exe`

Use the loader when you want direct control over launching a game client,
attaching daRPC to an existing client, or unloading it cleanly. If the daemon
should manage clients for you, start with [`darpcd.exe`](rpcd.md) instead.

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

## Command-line reference

The command surface is:

```text
loader [--json] inspect <pid>
loader [--json] attach [--diagnostics hook-timing] <pid> <dll-path>
loader [--json] detach <pid> <dll-path>
loader [--json] launch [--allow-multiple] [--diagnostics hook-timing] [--server <host[:port]>] \
    [--skip-intro] [--skip-notice] [--skip-exchange-alerts] \
    <executable-path> <dll-path> [-- <argument>...]
```

Arguments after the `--` separator are forwarded to the launched executable.
The executable path is also supplied explicitly as its `argv[0]`.

| Command | Purpose |
|---|---|
| `inspect` | Validate a running process and report whether the supported DLL is loaded. |
| `attach` | Validate a running process, inject the DLL, and confirm the loaded module. |
| `detach` | Ask the DLL to unload cleanly and confirm that the module is gone. |
| `launch` | Create a supported client in a suspended state, apply the default bootstrap fix and selected startup options, inject the DLL, then resume it. |

`--json` is a global output flag and must precede the command. It writes one
machine-readable result to standard output while keeping diagnostics on
standard error.

```text
loader.exe inspect 3780
loader.exe --json attach 3780 .\darpc.dll
loader.exe detach 3780 .\darpc.dll
```

The five optional launch patches are independent, may be combined, and are
disabled by default. Every supported-client launch also applies a mandatory
bootstrap sequence patch before the child resumes. It resets the outgoing
encrypted-packet sequence in the communications worker immediately before
`CHello` is encrypted, then removes the original producer-side late reset. This
keeps `CHello` at sequence zero and the later `CMulti` at sequence one during
both initial startup and a return from a game server to the main login server.

All startup patches apply only to a new suspended child. `attach` never
modifies client startup behavior. The loader validates the exact 7.41
executable fingerprint, both packet-encryption calls, and the original late
reset call. It writes and verifies an executable 21-byte bridge, restores
code-page protections, and flushes the instruction cache before resuming. Any
mismatch or incomplete write terminates the still-suspended child rather than
starting it partially patched.

| Launch option | Behavior |
| --- | --- |
| `--allow-multiple` | Bypasses the local `Nexon.SingleInstance` result check. |
| `--server <host[:port]>` | Resolves the host to IPv4, enables the client's positional endpoint parser, and disables fallback to the official endpoint. The default port is 2610. |
| `--skip-intro` | Enters the client's normal post-video state directly. |
| `--skip-notice` | Hides both notice-window paths, enables early title-menu pointer input, and removes the fixed one-second transfer delay while preserving normal notice and transfer processing. |
| `--skip-exchange-alerts` | Replaces the one-button alert shown after a player exchange completes or is cancelled with the same text in the floating game-message bar. Exchange state and item or gold transfers are unchanged. |

`--diagnostics hook-timing` enables runtime hook timing before the DLL installs
its hooks. It is accepted by both `attach` and `launch`. The same mode can be
enabled or disabled later over IPC, so reinjection is not required for routine
diagnosis. Omitting the option keeps timing disabled.

### Standard launch profile

The standard project profile passes all five launch options explicitly:

```text
loader.exe launch --allow-multiple --server <host[:port]> \
    --skip-intro --skip-notice --skip-exchange-alerts \
    <executable-path> <dll-path>
```

Use `--server 127.0.0.1:2610` when intentionally routing through
[Arbiter](https://github.com/ewrogers/Arbiter), a Dark Ages network analyzer and
local proxy. Arbiter must be configured to listen there and forward the
connection; strict endpoint selection does not fall back when the loopback
connection fails. Keeping the options explicit allows individual behaviors to
be omitted during diagnosis without changing the loader's unflagged behavior.

For `--server`, human diagnostics show the resolved IPv4 address and port. If
no additional client arguments were forwarded, they also show the exact game
command line. When additional arguments exist, the diagnostic omits them to
avoid recording potentially sensitive values.

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

- `pe.rs` validates the selected `darpc.dll` file and loaded module image and
  produces their Portable Executable identity and required lifecycle export
  relative virtual addresses (RVAs).
- `process.rs` owns target process handles, architecture inspection, process
  identity, executable-path discovery, and loaded-module discovery.
- `darpc-game-client` owns the exact supported executable fingerprint,
  canonical-path validation, and version-specific launch patch contracts.
- `launch.rs` owns suspended process creation, Windows argument quoting,
  primary-thread resumption, and child-only failure cleanup.
- `patch.rs` selects requested launch patches, finds the suspended main image,
  validates original bytes, and coordinates protected writes.
- `remote.rs` contains low-level remote allocation, memory reading and writing,
  and remote thread execution, including the bounded wait.
- `dll.rs` owns the Windows details for loading and unloading a DLL,
  including forwarded `LoadLibraryW` and `FreeLibrary` export resolution.
- `lifecycle.rs` coordinates the daRPC lifecycle and decides when rollback is
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
7. Unloads the DLL when initialization completes with an ordinary non-success
   status and the DLL reports that unloading is safe.
8. Re-inspects the target module list before reporting success.

If completion of the initialization thread is uncertain, the loader leaves the
DLL loaded rather than risk unloading code that may still be executing. The DLL
also has a distinct `UNLOAD_UNSAFE` lifecycle status for a hook commit whose
rollback or thread resumption could not be proven safe. Late attach reports the
failure but deliberately skips `FreeLibrary` for that status. A suspended child
that returns the same status is terminated by the launch owner.

## Detach lifecycle

The implemented detach path:

1. Validates the selected DLL and opens the x86 target.
2. Finds the loaded `darpc.dll` through target module enumeration and verifies
   that its path matches the selected DLL.
3. Reads the loaded module's bounded Portable Executable headers and export
   table, then requires its timestamp, image size, and lifecycle export RVAs to
   match the selected DLL exactly.
4. Calls `darpc_shutdown(0)` only after that identity check succeeds.
5. Calls `FreeLibrary` only after shutdown returns success.
6. Re-inspects the target and reports success only when `darpc.dll` is absent.

If the DLL file is replaced while an older build remains mapped in the client,
detach fails before creating a remote thread. Restore the matching file at the
same path to unload that build safely, or restart the client.

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
5. Replaces any inherited processor affinity with the complete system affinity
   mask while the child remains suspended. Launch fails and cleans up the owned
   child if Windows cannot apply the mask.
6. Validates the child as x86 and records its creation time without requiring
   module enumeration before Windows user-mode loader startup.
7. Resolves a selected server to dotted IPv4 and prepends the address and
   explicit port to the child arguments before process creation.
8. Reads the loaded main-module base from the child process environment block
   and validates every original instruction for the default runtime patch and
   any selected launch patches before writing anything.
9. Applies complete instructions with temporary writable protection, flushes
   the instruction cache, restores protection, and reads back each result.
10. Loads `darpc.dll` and calls `darpc_initialize` while the primary thread
   remains suspended.
11. Resumes the primary thread only after patching and initialization succeeds.
12. Starts a detached monitor for the bounded startup window and returns without
   waiting for it. The monitor reapplies the complete system affinity mask if
   client startup restores a single-processor mask. Direct loader launches and
   daemon REST launches share this path.
13. Terminates and waits for only that owned child if any launch operation
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

The exact 7.41 contracts follow the documented
[translucent-walk-commit](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/translucent-walk-commit.md),
[multiple-client](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/multiple-clients.md),
[command-line-endpoint](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/command-line-endpoint.md),
[disable-endpoint-fallback](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/disable-endpoint-fallback.md),
[skip-intro](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/skip-intro.md),
[hide-notice](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/hide-stipulation.md),
[early-continue](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/early-continue.md), and
[fast-server-transfer](https://github.com/ewrogers/darkages-741-re/blob/main/docs/appendix/runtime-patches/fast-server-transfer.md)
targets. The translucent-walk commit is always applied to a validated 7.41
client before launch; it routes a preserved translucent refresh through the
full appearance update so the walk finishes, the accepted destination commits,
and the object-owned translucency state updates together. The early-continue
patch enables the existing pointer hit-testing path
while the initial menu gate is set; keyboard input remains unchanged. Fast
server transfer changes the fixed post-connect sleep from one second to a yield;
the actual blocking connection can still pause the animation. The executable is
never modified on disk. A byte mismatch or failed write leaves the primary
thread suspended and enters owned-child cleanup.

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
the child uses the complete system processor affinity mask, handles were not
inherited, a normal process can exit, and a failed initialization leaves no
suspended child. The same sequence runs in the Windows workflow.

The live-client checks are intentionally local and require a legally obtained
Dark Ages 7.41 installation. They never copy the executable, enter credentials,
or record game data. Build the x86 artifacts, close every running client, and
run:

```powershell
./tools/test-game-client.ps1 `
  -ClientPath "C:\path\to\Darkages.exe" `
  -TargetDir ./target/i686-pc-windows-msvc/debug
```

This script proves unsupported-client rejection, two independent controlled
target processes, live late attach, live suspended launch, duplicate detection,
lifecycle logging, unload, and client liveness after unload. It force-stops only
the client processes it starts, so it is not evidence of normal interactive
exit behavior.

When orchestrating a Parallels guest from macOS, use direct current-user guest
execution for live launches as well as builds, controlled targets, inspection,
and attach. A scheduled task or other launch intermediary is unnecessary:

```sh
prlctl exec "<vm-name>" --current-user powershell.exe -NoProfile -Command \
  "& '<loader-path>' launch --allow-multiple --skip-intro --skip-notice '<client-path>' '<dll-path>'"
```

Add `--server '<host[:port]>'` when endpoint selection is part of the check. A
loopback endpoint such as `127.0.0.1:2610` can route through Arbiter when its
guest-local proxy is already listening and forwarding. Automated checks may
launch the client and inspect non-sensitive process state, but must not enter
credentials, record private game data, or force-terminate a client they do not
clearly own.

Complete the interactive portion of behavioral acceptance privately:

1. Close every running client. Start the client directly, log in, move, open
   and close representative user interface panels, and exit normally.
2. With no client running, use `loader launch <client-path> <dll-path>`. Repeat
   the same actions and exit normally with the inert DLL still loaded.
3. With no client running, start the client directly, use
   `loader attach <pid> <dll-path>`, repeat the same actions, and exit normally
   with the inert DLL still loaded.
4. Record whether all three runs behaved the same. Do not put credentials,
   private chat, or packet data in the record.

Verify optional launch patches with automated current-user launches where
practical. Exercise each option independently, then launch two clients
concurrently with `--allow-multiple --skip-intro --skip-notice` and, when needed,
`--skip-exchange-alerts` or `--server <host[:port]>`. Confirm that the intro and
notice are absent, both clients reach normal login, terminal exchange alerts
are absent only when requested, the selected endpoint is used, and ordinary
login and exit behavior remain intact. An unflagged launch remains the
comparison case. An explicit server is strict: if that connection fails, the
client follows its normal disconnected cleanup and does not retry the compiled
official endpoint.

For the mandatory bootstrap patch, also enter a game server, use the client's
normal exit-to-login action, and confirm that the main login server remains
connected through the next `CHello` and `CMulti` exchange.

The loader-owned startup patches run between suspended process validation and
DLL initialization. The bootstrap sequence patch is always included for a
validated 7.41 launch; the flags above control only their named optional
patches. Hooks and trampolines owned by daRPC remain the responsibility of
`darpc_initialize` and `darpc_shutdown`.
