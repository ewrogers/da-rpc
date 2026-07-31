# Progress

Last updated: 2026-07-31

This is the working task tracker for the
[roadmap](docs/src/roadmap.md). The roadmap defines milestone scope and
completion criteria. This file tracks current implementation work and should
remain concise.

## Current focus

M2, loader attach MVP.

The immediate goal is to complete the lifecycle for an existing 32-bit test
process: inspect, attach, initialize, shut down, detach, and verify the observed
module state.

## Milestone snapshot

| Milestone | Status | Notes |
| --- | --- | --- |
| M0, workspace scaffold | Complete | Workspace and documentation checks pass. |
| M1, local DLL lifecycle | Verification pending | Functional and manually exercised; Windows CI and explicit failed-initialization host coverage remain. |
| M2, loader attach MVP | In progress | Inspect and attach work; detach and the remaining result model are next. |
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

## Active M2 work

- [ ] Add a `detach` command for an existing target process.
- [ ] Resolve and call `darpc_shutdown` using the validated shutdown RVA.
- [ ] Unload only after shutdown completes successfully.
- [ ] Leave the DLL loaded when shutdown fails or its completion is uncertain.
- [ ] Verify through module enumeration that a successful detach removed
  `darpc.dll`.
- [ ] Define deliberate repeated-detach behavior.

## Remaining M2 work

- [ ] Add bounded remote-thread timeout handling.
- [ ] Distinguish missing process, exited process, access denied, timeout,
  already loaded, initialization failure, and shutdown failure results.
- [ ] Add structured result types and `--json` output.
- [ ] Run the complete Windows integration sequence against
  `injection-target.exe`: inspect, attach, inspect, detach, inspect.
- [ ] Exercise repeated attach and detach requests and every required failure
  path without terminating the target process.

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

## Updating this file

- Update the date and checkboxes when work starts or finishes.
- Keep only active and near-term tasks here.
- Move durable design details into the mdBook.
- Update the roadmap status table when a milestone changes state.
