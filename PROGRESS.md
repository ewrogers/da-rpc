# Progress

Last updated: 2026-07-31

This is the working task tracker for the
[roadmap](docs/src/roadmap.md). The roadmap defines milestone scope and
completion criteria. This file tracks current implementation work and should
remain concise.

## Current focus

M5, the minimal binary protocol, is complete. The next planned implementation
milestone is M6, direct IPC diagnostics through `darpc.exe`.

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

## Completed recently

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
- The stock client single-instance mutex keeps live M4 checks sequential.
  Multi-process loader behavior uses controlled targets until the planned
  startup patch exists.
- Parallels remote commands remain suitable for builds, controlled targets,
  inspection, and late attach. Live launch acceptance uses the active Windows
  interactive token and normal client exit.

## Updating this file

- Update the date and checkboxes when work starts or finishes.
- Keep only active and near-term tasks here.
- Move durable design details into the mdBook.
- Update the roadmap status table when a milestone changes state.
