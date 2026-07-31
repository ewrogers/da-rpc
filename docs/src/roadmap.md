# Roadmap

This roadmap starts with the smallest visible Windows result and grows one
working vertical slice at a time. Protocol and API work should support the next
demonstration instead of delaying all runtime integration until the end.

```text
load a harmless DLL
        |
        v
inject and launch with loader.exe
        |
        v
connect to the DLL over a named pipe
        |
        v
track it in rpcd and expose HTTP
        |
        v
add one carefully qualified hook
        |
        v
snapshot, updates, and typed actions
```

## How progress is measured

Every milestone has three parts:

- **Build:** the smallest new behavior.
- **See:** an observable demonstration that makes the progress tangible.
- **Done:** repeatable exit checks required before moving forward.

Milestones can be split when they stop feeling small. They should not be
combined merely because later code is nearby. Use Conventional Commits so each
working increment is easy to inspect or revert.

The [architecture](architecture.md), [state model](state.md),
[protocol](protocol.md), and [safety requirements](safety.md) remain the design
sources of truth. This page describes implementation order.

## Important DLL lifecycle rule

`DllMain` must remain minimal. It should not open a log file, start IPC, install
hooks, wait for threads, or perform other substantial work while the Windows
loader lock is held.

The loader lock serializes loader activity but does not suspend the other
process threads. Installing hooks from `DllMain` could therefore still race a
thread executing the code being patched.

The initial DLL can instead expose narrow `extern "system"` initialization and
shutdown functions. A local test host or `loader.exe` calls initialization after
`LoadLibraryW` completes and calls shutdown before `FreeLibrary`. Those exported
functions can safely create the lifecycle log and later start or stop worker
threads.

This still proves all four lifecycle points:

1. Windows loaded the module.
2. daRPC initialized successfully.
3. daRPC shut down successfully.
4. Windows unloaded the module.

Module enumeration verifies actual loaded state. The log verifies daRPC
initialization and shutdown. File output from `DLL_PROCESS_DETACH` should not be
treated as reliable evidence, especially during process termination.

## CLI roles

Two executables cover three command-line roles:

| Tool | Audience | Boundary |
| --- | --- | --- |
| `loader.exe` | Developer and `darpcd.exe` | Inspects, launches, injects, initializes, shuts down, and unloads `darpc.dll`. |
| `darpc.exe ipc ...` | Developer only | Connects directly to one DLL for bounded protocol diagnostics while `darpcd.exe` is disconnected. |
| Other `darpc.exe` commands | User and automation | Talk to the public `darpcd.exe` HTTP API and can print stable JSON results. |

The explicit `ipc` command group replaces a separate pipe-probe executable. It
shares `darpc-protocol` and proves the real named pipe and binary framing with
synthetic messages, but it is not a second production controller. Every other
[`darpc.exe` command](cli.md) goes through the daemon so discovery, aggregation,
action status, security, and multi-client behavior have one owner. If HTTP
request and response models later need to be shared, add a small API-model crate
only after real duplication appears.

## Progress

| ID | Increment | Visible result | Status |
| --- | --- | --- | --- |
| M0 | Workspace scaffold | Cargo recognizes the packages and the book builds. | Complete |
| M1 | Local DLL lifecycle | A 32-bit host loads, initializes, shuts down, and unloads `darpc.dll`. | Verification pending |
| M2 | Loader attach MVP | `loader.exe` injects and unloads the DLL in an existing test host. | Complete |
| M3 | Loader launch MVP | `loader.exe` launches a new process with the DLL initialized before normal execution. | Complete |
| M4 | Client bootstrap without hooks | The 7.41 client runs normally with the inert DLL loaded. | Complete |
| M4.1 | Optional launch patches | Explicit flags safely patch supported 7.41 startup and endpoint behavior before the client resumes. | Complete |
| M5 | Minimal binary protocol | A `Hello` frame has exact tested bytes and compatibility rules. | Complete |
| M6 | Direct IPC diagnostics | `darpc.exe` exchanges `Hello`, `Ping`, and `Echo` messages with one injected DLL. | Complete |
| M7 | Daemon client registry | `darpcd.exe` connects, tracks the DLL, and recovers after restart. | Complete |
| M8 | Read-only HTTP API | A browser or HTTP client lists the injected client. | Planned |
| M9 | Daemon-backed CLI | The existing `darpc.exe` adds client listing through the daemon HTTP API. | Planned |
| M10 | Discovery and managed launch | `darpcd.exe` reconciles candidates and invokes the loader explicitly. | Planned |
| M11 | Hook qualification harness | The hook mechanism preserves a controlled test function exactly. | Planned |
| M12 | First client tick hook | The daemon reports client ticks while the game behaves normally. | Planned |
| M13 | Minimal late-attach snapshot | The API and CLI expose a small real-client state slice. | Planned |
| M14 | Event-driven updates | One normal game event updates state without another snapshot. | Planned |
| M15 | Main-thread command queue | A diagnostic command completes on a client tick. | Planned |
| M16 | First typed action | One low-risk action executes through a native client path. | Planned |
| M17 | Packet observation and local rules | Bounded plaintext telemetry and fail-open decisions work locally. | Planned |
| M18 | Multi-client hardening and preview | Failure and soak evidence support a preview release. | Planned |
| M19 | WebSocket and remote access | Added only for a proven use case and defined security model. | Deferred |

M2 through M7 are complete. M8 is the next planned implementation milestone.
M1 has been exercised manually, but its separate evidence checklist remains
open until the lifecycle-host Windows continuous-integration coverage is
present. The working checklist is maintained in the [repository progress
tracker](https://github.com/ewrogers/da-rpc/blob/main/PROGRESS.md).

## Phase 1: prove the DLL and loader

### M0: workspace scaffold

Build:

- Cargo workspace and package boundaries.
- Minimal crate roots with no implementation behavior.
- Developer README, mdBook, and repository guidance.

See:

- `cargo metadata` lists all workspace packages.
- The dependency tree contains only internal packages.

Done:

- Formatting, Clippy, tests, and the documentation build pass on an available
  development host.

### M1: local DLL lifecycle

Current state:

- The x86 DLL, minimal `DllMain`, lifecycle exports, deterministic log, and
  repeated local lifecycle host are implemented and have been exercised
  manually.
- Completion remains pending for Windows CI and an explicit lifecycle-host
  initialization-failure and unload check.

Build:

- The smallest 32-bit `darpc.dll` with a minimal `DllMain`.
- Exported initialization and shutdown functions with an explicit system ABI,
  fixed-width parameters, structured status values, and no unwinding.
- A tiny owned 32-bit test host that uses `LoadLibraryW`, resolves the exports,
  calls them, and then uses `FreeLibrary`.
- A deterministic per-process lifecycle log under a user-writable daRPC data
  directory.
- A Windows continuous-integration check that builds the x86 DLL and test host.

See:

- Run the test host and observe initialization and shutdown records containing
  the process ID and DLL version.
- Observe that the module is present after loading and absent after unloading.

Done:

- The produced DLL is verified as x86.
- The host completes repeated load and unload cycles without leaked handles or
  worker threads.
- Initialization failure returns a useful status and the host can still unload
  the DLL.
- No file I/O, waiting, IPC, or hook installation occurs in `DllMain`.

### M2: loader attach MVP

Current state:

- `inspect`, `attach`, and `detach` are implemented with DLL validation, target
  x86 and creation-time checks, module discovery, explicit initialization and
  shutdown, rollback after rejected initialization, and verified unload.
- Remote memory operations, DLL operations, process state, PE validation, and
  daRPC lifecycle orchestration now have separate internal modules.
- Remote threads have bounded waits, command outcomes have stable human and
  JSON forms, and repeated attach and detach behavior is deliberate.
- The Windows integration harness covers the lifecycle and the required
  controllable failure paths without terminating the target process.

Build:

- `inspect`, `attach`, and `detach` commands for an existing 32-bit test host.
- Target architecture and process creation-time checks.
- Remote DLL loading followed by an explicit initialization call.
- Explicit shutdown before remote unloading.
- Human-readable diagnostics and a `--json` result mode.

See:

- Start the test host, attach by PID, inspect the loaded module, detach it, and
  inspect the process again.

Done:

- Already-loaded, missing, exited, wrong-architecture, access-denied, timeout,
  initialization-failed, and shutdown-failed results are distinct.
- The loader verifies loaded state through target module information, not only
  the log file.
- Failed attach or detach leaves the existing test process running.
- Repeating a command has deliberate behavior and never creates duplicate DLL
  instances.

### M3: loader launch MVP

Current state:

- `launch` creates an x86 process suspended, loads and initializes the selected
  DLL, and resumes the primary thread only after initialization succeeds.
- Arguments use explicit Windows quoting, the executable parent is the working
  directory, and inherited and standard handles are disabled.
- A failed pre-resume operation terminates and waits for only the child created
  by that invocation. Structured failures include its PID.
- Focused unit and native Windows integration tests cover lifecycle ordering,
  paths, quoting, working directory, handles, normal exit, and failure cleanup.

Build:

- A `launch` command that creates a controlled process suspended.
- Injection and initialization before normal process execution resumes.
- Argument forwarding and structured launch results.
- Cleanup limited to the child process created by this loader invocation.

See:

- Run one command that starts the test host, initializes the DLL, resumes the
  host, and produces the expected lifecycle log.

Done:

- Initialization succeeds before the main thread resumes.
- Injection failure does not leave a forgotten suspended child.
- Paths, quoting, current directory, inherited handles, and process exit are
  covered by focused tests.

### M4: client bootstrap without hooks

Current state:

- Exact executable validation is implemented for the supported 7.41 client.
- Unsupported attach and launch attempts are rejected before injection or
  process creation.
- Automated Windows checks cover live late attach, suspended launch, lifecycle
  logging, duplicate detection, unload, and post-unload liveness.
- Private manual acceptance confirmed normal login, movement, representative
  user interface behavior, and clean exit for baseline, late-attach, and
  interactive-token suspended-launch runs.
- The stock single-instance mutex currently limits live verification to
  sequential runs. Separate-process loader behavior is covered by controlled
  targets until a later startup-patch milestone.

Build:

- Exact Dark Ages 7.41 executable fingerprint validation.
- Canonical configured DLL selection.
- Attach and launch workflows against the real client with no hooks, client
  memory reads, IPC, or native client calls.

See:

- Launch and late-attach the inert DLL, confirm its lifecycle log, and use the
  client normally.

Done:

- Unsupported executables refuse injection.
- Existing injection is detected safely.
- Multiple separate client processes can each load one DLL instance.
- A documented manual test confirms login, movement, UI, and clean client exit
  behave the same with and without the inert DLL.

At this point the loader MVP is complete. Later loader work should be limited to
integration evidence, clearer diagnostics, and security requirements discovered
by daemon orchestration. Discovery and ongoing client management belong in
`rpcd`, not the loader.

Unloading is intentionally proven while the DLL is inert. Once hooks and
client-facing workers exist, unload must remain disabled until shutdown can
transactionally disable hooks, stop workers, drain callbacks, and prove that no
thread can execute DLL code after `FreeLibrary`.

### M4.1: optional launch patches

Build:

- Explicit `--allow-multiple`, `--server <host[:port]>`, `--skip-intro`, and
  `--skip-notice` options on `loader launch`, disabled by default.
- Hostname-to-IPv4 resolution and explicit positional endpoint arguments, with
  port 2610 used when `--server` omits a port.
- Strict endpoint selection that uses normal disconnected cleanup instead of
  falling back when an explicit server connection fails.
- Exact 7.41 relative virtual addresses, expected bytes, and replacement bytes
  owned by the `darpc-game-client` crate.
- Checked remote writes while the launched primary thread remains suspended,
  before DLL initialization and normal execution.

See:

- Launch with each option independently and together. Observe a second client
  starting, the intro videos being skipped, the notice window being hidden,
  early title-menu pointer input being available, the fixed transfer pause being
  removed, and the selected endpoint reaching normal login.

Done:

- Attach never accepts or applies startup patches, and an unflagged launch is
  unchanged.
- The loader uses the loaded main-module base, validates every requested
  original byte before any write, changes complete instructions, flushes the
  instruction cache, restores memory protection, and reads back replacements.
- A mismatch or failed write terminates only the owned suspended child and
  never resumes a partially patched process.
- Unit and controlled native Windows checks cover flag parsing, patch
  definitions, independent and combined requests, and fail-closed behavior.
- Private live-client verification confirms every launch option independently
  and together, including strict loopback routing through the Arbiter local
  proxy, with normal login and the inert DLL loaded.

## Phase 2: prove direct DLL communication

### M5: minimal binary protocol

Status: complete.

Build:

- Fixed little-endian frame header with a per-sender wrapping `u16` sequence,
  `u32` sender tick in milliseconds, message discriminants, and payload limits.
- A minimal `Hello` and `HelloAck` containing protocol range, DLL instance ID,
  process ID, process creation time, architecture, DLL version, executable
  fingerprint, and layout ID.
- Minimal `Ping`, `Pong`, `EchoRequest`, and `EchoResponse` messages with
  explicit request correlation.
- Checked manual encoding and decoding in `darpc-protocol`.
- Platform-independent codecs that accept sequence and tick values from their
  caller rather than reading a Windows clock.

See:

- A golden fixture encodes to exact documented bytes and decodes to the expected
  values without using Windows or a game client.

Done:

- Truncation, oversized lengths, overflow, invalid message order, unsupported
  versions, and trailing bytes have tests.
- The decoder validates lengths before allocating.
- Rust memory layout is never the wire layout.
- Golden and boundary tests cover sequence wrap and timestamp wrap arithmetic.
- The protocol chapter documents every field, discriminant, limit, ordering
  rule, exact golden bytes, and the choices that should be reviewed before M6.

### M6: direct IPC diagnostics through `darpc.exe`

Build:

- A deterministic PID-based named-pipe server started by DLL initialization and
  stopped by DLL shutdown.
- Bounded overlapped I/O owned by a DLL worker, independent from game code.
- Windows transport stamping with `timeGetTime` immediately before sending each
  frame, plus wrapping sequence advancement in each direction.
- A `darpc.exe ipc` command group that shares `darpc-protocol` and connects
  directly to one DLL by process ID.
- `hello`, `ping`, and `echo` commands using the real framing and named pipe.
- A UTF-8 echo payload limited to 4 KiB and returned byte-for-byte.

See:

- Inject the DLL and run `darpc --output json ipc hello --pid <pid>`.
- Measure a request round trip with `darpc ipc ping --pid <pid>`.
- Receive `hello` from `darpc ipc echo --pid <pid> "hello"`.
- Run commands over replacement connections, then unload the DLL cleanly.

Done:

- These commands use synthetic protocol data only. They install no hooks, read
  no client memory, maintain no game state, and call no client functions.
- Malformed input disconnects only that IPC client.
- The DLL remains usable with no IPC client connected.
- Pipe reads, writes, and cancellation have bounded shutdown behavior.
- Request IDs and echoed bytes are verified by tests.
- The command clearly reports that `darpcd.exe` must be disconnected before it
  can own the DLL pipe.

## Phase 3: establish the daemon and public tools

### M7: daemon client registry

Build:

- A shared controller connection that lets `darpc.exe` and `darpcd.exe` reuse
  the working pipe handshake, sequencing, and compatibility checks.
- A repeatable `--pid <pid>` target option, for example
  `darpcd.exe --pid 3780 --pid 6244`, until automatic discovery is introduced
  later. Zero and duplicate PIDs are rejected.
- One independent connection worker per target PID with bounded health checks,
  disconnect detection, and retry.
- An in-memory registry keyed by process ID, process creation time, and DLL
  instance ID.
- Visible connecting, connected, disconnected, busy, and incompatible status
  transitions.

See:

- Start the DLL and daemon in either order and observe one registered client.
- Restart the daemon without reinjecting or restarting the game client.
- Late-attach two logged-in clients and observe two independent registry
  records.

Done:

- Stale process IDs and changed DLL instance IDs cannot reuse old state.
- Incompatible handshakes remain visible but are not accepted as clients.
- One broken connection cannot terminate another client or the daemon.
- Stopping the daemon releases each pipe, and restarting it reconnects to the
  same DLL instances without reinjection.
- The registry contains connection identity and status only. This increment
  installs no hooks, reads no game state, and defines no snapshot messages.

### M8: read-only HTTP API

Build:

- A loopback-only Axum server with unversioned `/health` and `/clients` routes.
- HTTP models separate from binary wire and client layout types.
- A `utoipa`-generated OpenAPI document at `/openapi.json` and vendored Swagger
  UI at `/docs`.
- Bounded requests and explicit unavailable states.

See:

- Open `/docs` in a browser and inspect the two-client registry through the
  interactive API.
- Import `/openapi.json` into a standard OpenAPI consumer.

Done:

- Missing, disconnected, incompatible, and connected clients are distinct.
- The UI works without runtime internet access and describes every public
  route.
- HTTP failure cannot block or corrupt the DLL connection.

### M9: daemon-backed CLI commands

Build:

- HTTP-backed commands added to the existing `darpc.exe` parser and output
  model.
- Human-readable output by default and stable JSON output for automation.
- Initial commands limited to daemon health and client listing.

See:

- Run `darpc clients --json` and receive the same clients represented by the
  HTTP endpoint.

Done:

- Commands outside the explicit `ipc` group never connect directly to DLL
  pipes.
- Exit codes distinguish connection failure, invalid input, and daemon errors.
- JSON output is separated from diagnostics so scripts can parse it reliably.

### M10: discovery and managed launch

Build:

- Periodic top-level-window reconciliation using a verified game window class.
- Candidate PID and expected pipe derivation.
- Missing, busy, initializing, connected, and incompatible candidate states.
- Explicit daemon invocation of `loader.exe` for inspect, attach, or launch.

See:

- Start an uninjected client, list it as a candidate, explicitly attach it, and
  watch it become connected through the daemon and CLI.

Done:

- A window match alone never proves compatibility.
- Handshake failure never causes automatic reinjection.
- The daemon cannot ask the loader to inject an arbitrary DLL.
- Multiple candidates remain independent.

This completes the first useful process-management vertical slice. The daemon,
loader, DLL, HTTP API, and CLI now work together without any game hook.

## Phase 4: qualify hooks before reading state

### M11: hook qualification harness

Build:

- A known x86 function in the owned test host with deterministic inputs and
  outputs.
- Transactional detour installation, original-call preservation, and rollback.
- Preparation of complete trampolines before a short thread-enlisted commit
  that changes executable code and flushes the instruction cache.
- Tests for repeated installation, failed installation, recursion, concurrent
  calls, and shutdown.

See:

- Exercise the test function before, during, and after the hook and compare
  identical results while recording bounded observations.

Done:

- Every trampoline used by the harness has reviewed instruction relocation.
- Partial installation rolls back completely.
- Concurrent threads cannot observe a partially written hook, and instruction
  pointers inside a replaced range are safely rejected or redirected.
- No panic or unwind crosses the native boundary.
- Shutdown proves no thread can call the detour after unload.

### M12: first client tick hook

Build:

- Exact fingerprint and relative virtual address validation for
  `event_dispatcher_tick`.
- A minimal detour that increments a bounded counter and always calls the
  original function.
- Hook health published later by the DLL worker, never through I/O in the hook.

See:

- `darpc.exe` reports advancing client ticks while input, rendering, and network
  activity continue normally.

Done:

- An incorrect fingerprint refuses hook installation.
- The hook takes no IPC lock, performs no I/O, and does not call arbitrary game
  functions.
- A repeatable live-client soak shows no behavior change or crash.

## Phase 5: expose real state incrementally

### M13: minimal late-attach snapshot

Build:

- A deliberately small state slice, such as client identity, character name,
  and current map when available.
- Validated roots, offsets, buffers, and root-generation tracking.
- Snapshot messages with an explicit sequence boundary.
- Matching daemon state, HTTP response, and CLI output.

See:

- Late-attach after login and read the real state slice with
  `darpc client <id> state --json`.

Done:

- Unsupported lifecycle states produce partial or unavailable values.
- Snapshot work has a measured client-thread budget.
- Restarting the daemon obtains a fresh equivalent snapshot without reinjection.
- The daemon retains each snapshot as a client observation and does not treat
  one client's visible map region as complete world truth.

### M14: event-driven updates

Build:

- Observe one narrow, understood event family through `event_dispatch`.
- Copy bounded pointer-free values and update the local state model.
- Ordered absolute updates plus Server-Sent Events from `rpcd`.

See:

- Perform one normal game action and watch state and event output change without
  requesting another snapshot.

Done:

- A new snapshot after scripted actions equals incrementally maintained state
  for the fields in scope.
- Sequence gaps and queue overflow trigger resynchronization.
- A slow event subscriber cannot block the game or another subscriber.
- Shareable map and entity observations retain their source client and
  observation time so a later aggregate can represent freshness explicitly.

## Phase 6: add control one safe action at a time

### M15: main-thread command queue

Build:

- Bounded IPC command validation and enqueueing.
- A fixed per-tick drain budget.
- Accepted, executed, failed, cancelled, and timed-out action states.
- A diagnostic command that changes no game state.

See:

- Request the diagnostic command through HTTP and watch it complete on a client
  tick.

Done:

- Queue-full returns busy immediately.
- IPC workers never call client functions.
- Disconnect and timeout cannot leave queued client pointers or unbounded work.

### M16: first typed action

Build:

- One low-risk action through a confirmed native client producer.
- State and argument validation before main-thread execution.
- CLI submission plus asynchronous action status.

See:

- Request the action and observe its normal client-visible effect and later
  state or server result.

Done:

- Unsupported states reject the action without a native call.
- Execution is distinct from a later observed server outcome.
- Non-idempotent actions are not automatically retried.

### M17: packet observation and local rules

Build:

- Bounded observation of opcode-first bodies at
  `net_submit_client_packet`.
- Redaction of sensitive data by default.
- Typed packet-producing actions through the native submission boundary.
- Immutable bounded rules evaluated locally without IPC waits.

See:

- Observe a controlled action before encryption, then apply a local test rule
  and see its revision and result.

Done:

- Original sequence, transform, and submission behavior remain unchanged.
- Telemetry loss cannot block the hook.
- Rule evaluation has deterministic priority and bounded cost.
- Invalid rules, missing controllers, and internal errors fail open.

## Phase 7: harden what is actually useful

### M18: multi-client hardening and preview

Build:

- Multiple-client aggregation and independent action routing.
- A derived shared-world view for compatible client observations, preserving
  map identity, source clients, last-seen time, and stale or uncertain status.
- Malformed-protocol corpus and parser fuzzing.
- Queue saturation, daemon restart, pipe failure, injection failure, hook
  rollback, and client-lifecycle tests.
- Crash diagnostics that avoid sensitive data and do no work from hooks.
- Reproducible build and manual live-client validation instructions.

See:

- Run the documented verification suite and an extended multi-client soak while
  stopping and restarting the daemon.

Done:

- Every supported behavior has repeatable automated or manual evidence.
- Known unsafe boundaries have reviewed invariants.
- Failures leave the game usable whenever the operating system still permits
  recovery.
- The preview release states exactly what is and is not supported.

### M19: WebSocket and remote access

This remains deferred until a real application demonstrates that REST plus
Server-Sent Events cannot express its interaction cleanly. Remote listening
also requires authentication, authorization, transport security, request
limits, and administrative capability boundaries before it is supported.

## Immediate next increment

M8 should expose the working registry without changing DLL behavior:

1. Add a small loopback-only HTTP server to `darpcd.exe`.
2. Use Axum for unversioned `/health` and `/clients` routes.
3. Keep HTTP response types separate from registry and wire-protocol types.
4. Represent connecting, connected, disconnected, busy, and incompatible
   targets explicitly.
5. Generate `/openapi.json` with `utoipa` and serve a vendored Swagger UI at
   `/docs`.
6. Bound request parsing and ensure HTTP failure cannot block a client worker.
7. Inspect the two-client registry in Swagger UI and import its OpenAPI
   document into a standard client tool.

Snapshots remain M13. M8 exposes only the identity and connection-health data
already owned by the daemon.
