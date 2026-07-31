# Progress

Last updated: 2026-07-31

This is the working task tracker for the
[roadmap](docs/src/roadmap.md). The roadmap defines milestone scope and
completion criteria. This file tracks current implementation work and should
remain concise.

## Current focus

M2, loader attach MVP, is complete.

M3, loader launch MVP, is next. Launch must keep the new process primary thread
suspended through loader-owned startup patches and DLL initialization.

## Milestone snapshot

| Milestone | Status | Notes |
| --- | --- | --- |
| M0, workspace scaffold | Complete | Workspace and documentation checks pass. |
| M1, local DLL lifecycle | Verification pending | Functional and manually exercised; Windows CI and explicit failed-initialization host coverage remain. |
| M2, loader attach MVP | Complete | Inspect, attach, detach, JSON results, timeouts, and Windows integration coverage are implemented. |
| M3, loader launch MVP | Planned | Launch must keep the primary thread suspended through patches and initialization. |

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

## Next M3 work

- [ ] Define the `launch` command and structured result.
- [ ] Create the target process with its primary thread suspended.
- [ ] Reuse the existing load and lifecycle operations before resuming.
- [ ] Define cleanup for launch failures without leaving a suspended child.

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
- Target module enumeration is the source of truth for observed loaded state.
- A lifecycle relative virtual address is used only after the observed loaded
  module path matches the validated DLL path.

## Updating this file

- Update the date and checkboxes when work starts or finishes.
- Keep only active and near-term tasks here.
- Move durable design details into the mdBook.
- Update the roadmap status table when a milestone changes state.
