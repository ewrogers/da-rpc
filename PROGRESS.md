# Progress

Last updated: 2026-08-01

This is the working task tracker for the
[roadmap](docs/src/roadmap.md). The roadmap defines milestone scope and
completion criteria. This file tracks current implementation work and should
remain concise.

## Current focus

M12, the late-attach client snapshot, is complete. The next planned
implementation milestone is M13, event-driven updates.

## Milestone snapshot

| Milestone | Status | Notes |
| --- | --- | --- |
| M0, workspace scaffold | Complete | Workspace and documentation checks pass. |
| M1, local DLL lifecycle | Verification pending | Functional and manually exercised; Windows CI and explicit failed-initialization host coverage remain. |
| M2, loader attach MVP | Complete | Inspect, attach, detach, JSON results, timeouts, and Windows integration coverage are implemented. |
| M3, loader launch MVP | Complete | Suspended launch, pre-resume initialization, argument forwarding, and owned-child cleanup are implemented and Windows-tested. |
| M4, client bootstrap without hooks | Complete | Exact validation, live launch and attach, lifecycle evidence, and normal-use acceptance pass. |
| M4.1, optional launch patches | Complete | Exact 7.41 patch contracts, strict endpoint selection, safe pre-resume application, and live-client acceptance pass. |
| M5, minimal binary protocol | Complete | Exact framing, identity handshake, diagnostics, checked codecs, and golden and boundary tests are implemented. |
| M6, direct IPC diagnostics | Complete | The DLL pipe worker and direct hello, ping, and echo commands pass controlled and live-client Windows verification. |
| M7, daemon client registry | Complete | Repeated explicit `--pid` targets, shared controller sessions, independent reconnecting workers, and identity-safe registry records pass controlled and live-client verification. |
| M8, read-only HTTP API | Complete | Loopback Axum routes, client identity snapshots, generated OpenAPI, and vendored Swagger UI pass controlled and live-client verification. |
| M9, discovery and managed launch | Complete | Exact-window discovery and API-managed load, unload, and constrained launch pass native Windows integration coverage. |
| M9.1, automatic managed loading | Complete | Opt-in one-shot loading covers existing and later discoveries while preserving explicit unload. |
| M10, hook qualification harness | Complete | Transactional x86 detours, relocated trampolines, rollback, concurrency, panic containment, and shutdown pass owned native tests. |
| M11, first client tick hook | Complete | Exact validation, safety hardening, direct health observation, repeated live attach and detach, and normal in-game acceptance pass. |
| M12, late-attach client snapshot | Complete | Main-thread capture, the full initial character and collection surface, protocol and API presentation, and live-client comparison pass. |

## Completed recently

- [x] Added a bounded main-thread state walk with validated roots, pointer
  chains, collection capacities, slots, and world-generation detection.
- [x] Added scalar character, map, inventory, equipment, spellbook, and
  skillbook models across the DLL, protocol, direct CLI, daemon, REST, and
  OpenAPI.
- [x] Kept allocation, text conversion, serialization, IPC, and logging off the
  hook path through a fixed-capacity publication handoff.
- [x] Verified the snapshot against a late-attached live client and measured
  capture duration and round-trip time.

## M12 completion evidence

- [x] Capture a complete current observation from an already logged-in client
  without a remote memory-reading thread or process-wide suspension.
- [x] Represent lifecycle-unavailable groups explicitly and omit empty slots.
- [x] Exclude the inventory currency display slot because character gold is the
  canonical currency value.
- [x] Validate strict snapshot codec round trips, malformed collection bounds,
  slots, duplicate slots, and string limits within protocol 1.0.
- [x] Expose equivalent human and JSON direct CLI output and
  `GET /clients/{pid}/snapshot` with generated OpenAPI schemas.
- [x] Split state walking, collection conversion, protocol codecs, CLI output,
  and HTTP models along focused domain boundaries.

- [x] Added the exact supported-client `event_dispatcher_tick` target contract
  and a minimal `thiscall` detour that always calls the original function.
- [x] Added protocol 1.0 tick-hook health messages, direct human and JSON CLI
  output, and worker-side file tracing without hook-path I/O.
- [x] Kept protocol 1.0 compatible and added strict round-trip,
  malformed-value, controlled-target, and native x86 coverage.
- [x] Reported thread-resume failures, retained installed-detour ownership
  across post-commit warnings, and prevented late-attach unload when rollback
  safety cannot be established.
- [x] Added deterministic target-instruction-pointer, active-call, resume
  warning, and unload-safety tests.

## M11 completion evidence

- [x] Validate the executable fingerprint, target relative virtual address, and
  entry bytes before hook installation.
- [x] Keep the hook path bounded to atomic activity and tick counters plus the
  original trampoline call.
- [x] Publish hook health only from the DLL worker and expose two-sample
  advancement through `darpc.exe ipc tick-health`.
- [x] Refuse the hook on the controlled unsupported-client fixture while
  retaining IPC diagnostics.
- [x] Observe advancing ticks through direct IPC on the supported live client.
- [x] Complete a repeatable live soak covering input, rendering, network
  activity, clean hook removal, DLL unload, and continued client operation.

- [x] Added the reusable `darpc-hook` x86 detour boundary without game-specific
  addresses or state logic.
- [x] Qualified a relocated relative call, original-call preservation,
  transactional rollback, concurrent calls, and safe removal in owned code.
- [x] Added native x86 unit tests plus debug and release harness coverage to
  Windows continuous integration.

- [x] Added opt-in daemon `--auto-load` for existing and later `not_loaded`
  clients.
- [x] Limited automatic loading to one background attempt per tracked process
  and suppressed repeated worker events while that attempt is active.
- [x] Preserved explicit unload and isolated automatic loader failures per
  client.

## M10 completion evidence

- [x] Added the x86-only `darpc-hook` crate with explicit prepared and installed
  detour states.
- [x] Decode complete target instructions and relocate them with `iced-x86`
  before changing target code.
- [x] Keep trampoline memory writable only during preparation, then seal it
  executable and read-only and flush the instruction cache.
- [x] Enlist every other process thread to commit or restore a complete x86
  jump while rejecting instruction pointers in protected code ranges.
- [x] Track active detour calls and require quiescence before removal.
- [x] Restore byte-exact original code after an injected post-write failure.
- [x] Verify repeated preparation, repeated removal, original-call recursion,
  panic containment, four concurrent callers, and no post-removal observations.
- [x] Run the controlled harness from native x86 debug and release builds
  without loading or modifying the game client.

## M9.1 completion evidence

- [x] Parse and document `--auto-load` while leaving default behavior explicit.
- [x] Reuse daemon-owned lifecycle control and mandatory loader validation.
- [x] Cover disabled, repeated, in-flight, already handled, explicit lifecycle,
  and removed-process policy behavior with focused tests.
- [x] Verify existing discovery, later discovery, automatic connection, and
  persistent explicit unload with controlled x86 processes on Windows while an
  incompatible candidate fails once without affecting them.

- [x] Added `darpc_initialize` and `darpc_shutdown` with a versioned ABI and
  structured statuses.
- [x] Added local repeated lifecycle loading, initialization, shutdown, and
  unloading.
- [x] Added x86 DLL validation and required lifecycle export discovery.
- [x] Added target process architecture, creation-time, and module inspection.
- [x] Added remote `LoadLibraryW` execution and loaded-module verification.
- [x] Added remote `darpc_initialize` execution.
- [x] Added rollback unloading after initialization returns a non-success
  status.
- [x] Separated validated DLL metadata, target process state, remote mechanics,
  remote DLL operations, and daRPC lifecycle orchestration.
- [x] Confirmed successful initialization and rejected-ABI rollback manually on
  Windows.
- [x] Added host tests and 32-bit Windows check and Clippy coverage for the
  loader source.
- [x] Added explicit remote shutdown followed by verified unloading.
- [x] Added stable structured errors, exit codes, and human and JSON results.
- [x] Added bounded remote-thread waits with conservative uncertain-completion
  behavior.
- [x] Added a Windows lifecycle and failure integration harness.
- [x] Added suspended process launch with initialization before primary-thread
  resume.
- [x] Added Windows argument quoting, executable-directory working directory,
  and structured launch results.
- [x] Added launch-owned child termination and waiting for every pre-resume
  failure.
- [x] Added native Windows launch ordering, lifecycle log, argument, path,
  handle, natural-exit, and failure-cleanup coverage.
- [x] Added exact Dark Ages 7.41 executable size and SHA-256 validation.
- [x] Rejected unsupported attach and launch attempts before injection or child
  creation.
- [x] Verified duplicate detection, late attach, suspended launch, lifecycle
  logging, unload, and post-unload liveness against the supported live client.
- [x] Verified independent DLL instances in two controlled target processes.
- [x] Proved release builds ignore the debug-only controlled-target bypass.
- [x] Confirmed normal login, movement, representative user interface behavior,
  and clean exit in baseline, late-attach, and interactive-token launch runs.

## M4 completion evidence

- [x] Run the automated live-client harness on 32-bit Windows.
- [x] Confirm the inert DLL does not read client memory, call client functions,
  install hooks, or start IPC.
- [x] Compare login, movement, representative user interface behavior, and
  normal exit in baseline, loader-launch, and late-attach runs.
- [x] Record M4 complete after the private manual comparison passes.

## M4.1 completion evidence

- [x] Validate every selected 7.41 patch site before writing any replacement
  bytes.
- [x] Verify the 32-bit Windows patch application and rollback tests.
- [x] Verify `--server` resolves and forwards an explicit endpoint, enables the
  command-line parser, and disables compiled-endpoint fallback in the live
  client.
- [x] Verify `--skip-intro` in the live client.
- [x] Verify `--allow-multiple` with an existing live client.
- [x] Verify `--skip-notice` hides the notice, enables early title-menu pointer
  input, and removes the fixed transfer pause in the live client.
- [x] Verify all options together, including strict loopback routing through the
  Arbiter local proxy, and record M4.1 complete.

## M5 completion evidence

- [x] Define the fixed little-endian frame header, discriminants, sizes, and
  conservative limits.
- [x] Encode and decode `Hello`, `HelloAck`, `Ping`, `Pong`, `EchoRequest`, and
  `EchoResponse` without relying on Rust memory layout.
- [x] Enforce version negotiation, instance matching, handshake order, and
  per-sender wrapping sequence progression.
- [x] Match the documented 95-byte Hello fixture in both directions.
- [x] Cover all message round trips, every truncated Hello prefix, malformed
  fields, hostile lengths, overflow, trailing bytes, and sequence and tick
  wraparound.
- [x] Document the complete wire contract and a human review checklist.

## M6 completion evidence

- [x] Start the deterministic PID-based pipe worker during DLL initialization
  and stop it before successful shutdown.
- [x] Use local-only, single-instance, overlapped pipe I/O with bounded reads,
  writes, accepts, and cancellation.
- [x] Implement direct `darpc.exe ipc hello`, `ping`, and byte-exact `echo`
  commands with human and JSON output.
- [x] Enforce handshake ordering, per-sender sequences, request correlation,
  protocol compatibility, and conservative payload bounds.
- [x] Distinguish missing, busy, denied, timed-out, incompatible, malformed, and
  other I/O failures with stable JSON names and exit codes.
- [x] Verify malformed-client isolation, reconnect, no-client operation, and
  shutdown during pending accept and connected I/O on 32-bit Windows.
- [x] Verify late attach, all three diagnostics, clean unload, and continued
  process liveness against the supported client.

## M7 completion evidence

- [x] Extract the controller handshake, sequencing, and framing into one shared
  session used by `darpc.exe` and `darpcd.exe`.
- [x] Accept repeated nonzero unique `--pid <pid>` targets and retry missing or
  busy endpoints.
- [x] Give each target an independent worker with bounded periodic health pings
  and reconnect behavior.
- [x] Key accepted records by PID, process creation time, and DLL instance ID;
  replace changed identities and ignore stale disconnect events.
- [x] Keep incompatible identity visible without accepting it as a client.
- [x] Require the supported x86 architecture, executable fingerprint, and
  client version before accepting a release connection.
- [x] Verify daemon-first startup, two controlled targets, exclusive ownership,
  daemon restart, one-client replacement, and other-client independence.
- [x] Late-attach release builds to two logged-in 7.41 clients, register both,
  restart the daemon without reinjection, unload both DLLs, and confirm both
  client processes remain alive.
- [x] At the M7 boundary, keep the registry limited to identity and connection
  health with no hooks, client-memory reads, state snapshots, or web API.

## M8 completion evidence

- [x] Bind Axum to `127.0.0.1:2626` by default and accept one validated
  `--port <port>` override without silently falling back.
- [x] Publish immutable registry snapshots to an isolated HTTP thread without
  holding the live registry across network I/O.
- [x] Expose `/health` and `/clients` with dedicated response models and
  explicit connecting, not-loaded, connected, busy, disconnected, and
  incompatible states.
- [x] Expose each observed PID, exact decimal process `created_time`, DLL
  `instance_id`, and connection compatibility metadata.
- [x] Generate an OpenAPI 3.1 document at `/openapi.json` and serve vendored,
  offline Swagger UI assets with an Ayu-inspired dark theme at `/docs`.
- [x] Reject request bodies on the read-only routes, invalid port options,
  duplicate port options, and occupied listeners with bounded, explicit
  failures.
- [x] Verify the default and overridden ports, two controlled targets, daemon
  restart, one-client replacement, OpenAPI, Swagger assets, and failure
  isolation on Windows.
- [x] Late-attach release builds to two supported clients, match both HTTP
  creation identities to loader inspection, validate compatibility metadata,
  unload both DLLs, and confirm both processes remain alive.

## M9 completion evidence

- [x] Discover the exact `Darkages` top-level window class at startup and once
  per second while retaining repeated explicit PID targets.
- [x] Add and remove independent workers as discovered windows appear and
  disappear, with a bounded grace period for newly launched processes.
- [x] Keep window matches as untrusted candidates and require loader validation
  plus a daRPC handshake before reporting a compatible connection.
- [x] Expose API-managed load, unload, and launch operations while keeping the
  loader and DLL paths under daemon configuration.
- [x] Limit launch requests to a validated client executable path,
  allow-multiple, skip-intro, skip-notice, and an optional server endpoint
  whose port defaults to 2610, without arbitrary arguments.
- [x] Document the request, result, status, and error models in generated
  OpenAPI and the standalone mdBook chapters.
- [x] Verify automatic discovery, not-loaded state, load, unload, relaunch,
  strict request rejection, independent candidates, and daemon reconnect on
  native Windows with controlled x86 processes.
- [x] Launch the supported client with rapid-test options through the HTTP API,
  observe its returned PID connect, unload and reload it, and confirm unload
  leaves the process alive.

## M2 completion evidence

- [x] Added a `detach` command for an existing target process.
- [x] Resolve and call `darpc_shutdown` using the validated shutdown RVA.
- [x] Unload only after shutdown completes successfully.
- [x] Leave the DLL loaded when shutdown fails or its completion is uncertain.
- [x] Verify through module enumeration that a successful detach removed
  `darpc.dll`.
- [x] Repeated detach succeeds without a state change; repeated attach returns
  `already_loaded`.
- [x] Add bounded remote-thread timeout handling.
- [x] Distinguish missing process, exited process, access denied, timeout,
  already loaded, initialization failure, and shutdown failure results.
- [x] Add structured result types and `--json` output.
- [x] Add the complete Windows integration sequence against
  `injection-target.exe`: inspect, attach, inspect, detach, inspect.
- [x] Exercise repeated attach and detach requests and controllable failure
  paths without terminating the target process.

## M3 completion evidence

- [x] Added `launch <executable-path> <dll-path> [-- <argument>...]`.
- [x] Validate the DLL before creating a suspended x86 child.
- [x] Load and initialize the DLL before resuming the primary thread.
- [x] Confirm the real DLL lifecycle log exists when the test target enters
  `main`.
- [x] Forward spaced, trailing-backslash, and Unicode arguments and cover quote
  and empty-argument encoding with focused unit tests.
- [x] Use the executable parent as current directory and disable inherited and
  standard handles.
- [x] Return the owned child PID in structured post-creation failures.
- [x] Terminate and wait for the owned child after initialization failure.
- [x] Confirm a successfully launched process can exit normally.
- [x] Run unit tests and the complete M3 integration suite natively on 32-bit
  Windows through Parallels.

## M1 completion evidence

- [ ] Add Windows CI that builds the x86 DLL and lifecycle host.
- [ ] Exercise a rejected ABI version in `lifecycle-host.exe` and verify the DLL
  can still unload.
- [ ] Record automated evidence that the DLL is x86 and repeated lifecycle
  cycles leave no module or worker thread behind.

## Established decisions

- `DllMain` remains minimal and does not perform initialization, shutdown, IPC,
  logging, or hook work.
- A completed non-success initialization rolls itself back internally, after
  which the loader may call `FreeLibrary`.
- Uncertain remote lifecycle completion leaves the DLL loaded.
- daRPC-owned hooks and trampolines belong to `darpc_initialize` and
  `darpc_shutdown`.
- Future launch-time client patches run while the launched primary thread is
  suspended. The thread resumes only after patches and DLL initialization
  succeed.
- Existing-process module enumeration is the source of truth for observed
  loaded state. Suspended launch uses the remote load result until Windows
  user-mode loader startup makes enumeration available.
- A lifecycle relative virtual address is used only after the observed loaded
  module path matches the validated DLL path.
- Unflagged launches preserve the stock single-instance behavior. Explicit
  `--allow-multiple` launches apply the validated startup patch before resume.
- Parallels remote commands remain suitable for builds, controlled targets,
  inspection, and late attach. Live launch acceptance uses the active Windows
  interactive token and normal client exit.
- `darpc.exe` talks directly to one DLL and remains usable without the daemon.
  Multi-client aggregation is consumed through the `darpcd.exe` web API.
- Loader and DLL lifecycle paths are server-side configuration. Launch requests
  select the supported client executable but expose no arbitrary argument
  forwarding.
- The daemon aggregates client identity, connection health, and the latest
  complete client snapshot.
- Character and user interface state remain per-client. A future shared-world
  projection must preserve observation source, last-seen freshness, and stale
  or uncertain status.

## Updating this file

- Update the date and checkboxes when work starts or finishes.
- Keep only active and near-term tasks here.
- Move durable design details into the mdBook.
- Update the roadmap status table when a milestone changes state.
