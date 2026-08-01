# daRPC

[![Documentation](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml)

daRPC, short for Dark Ages Remote Procedure Call, is a Rust workspace for
integrating developer tools with the 32-bit Windows client of *Dark Ages*.
The project is in early development and does not yet provide a working client
state integration. Injection, launch-time patches, direct named-pipe
diagnostics, automatic client discovery, and daemon-managed client lifecycle
operations are implemented. The x86 detour mechanism is qualified against an
owned concurrent test harness, and the first game-client tick hook is
implemented and qualified through controlled and live-client testing.

Read the [daRPC Book](https://ewrogers.github.io/da-rpc/) for the architecture,
current implementation status, protocol, safety requirements, and development
guidance.

daRPC is designed around an injected library instead of a network proxy. The
library can attach to an existing client, observe internal events, maintain a
separate state model, and submit actions through the client's native paths. A
daemon discovers and manages connected clients and exposes their state through
portable web APIs.

## Components

| Component | Target | Responsibility |
| --- | --- | --- |
| `darpc.dll` | 32-bit Windows x86 | Integrates with one client, maintains local state, and hosts its named-pipe endpoint. |
| `loader.exe` | 32-bit Windows x86 | Launches a compatible client or injects `darpc.dll` into an existing one. |
| `darpc.exe` | 64-bit Windows x86-64 | Talks directly to one DLL through the binary protocol and presents human-readable or JSON results. |
| `darpcd.exe` | 64-bit Windows x86-64 | Discovers clients, aggregates state and events, and exposes web APIs. |

daRPC supports one exact game-client build at a time. The `darpc-game-client`
crate owns that build's fingerprint, layouts, addresses, and application binary
interface boundaries. Supporting a different client is a fork-level change, not
an in-tree matrix of versioned layout crates.

The DLL remains independent of the daemon. If `darpcd.exe` is stopped or
restarted, an injected client must continue operating normally and accept a new
daemon connection later.

## Developer harnesses

| Harness | Target | Purpose |
| --- | --- | --- |
| `lifecycle-host.exe` | 32-bit Windows x86 | Loads `darpc.dll` locally, exercises its lifecycle contract, and verifies repeated loading and unloading. |
| `injection-target.exe` | 32-bit Windows x86 | Provides an inert, persistent process for safe loader attach and detach testing. |
| `hook-harness.exe` | 32-bit Windows x86 | Qualifies transactional detours, relocated trampolines, concurrent calls, rollback, and removal without touching the game client. |

These harnesses support local development and integration testing. They are not
runtime components distributed to end users.

## Workspace

```text
components/
  rpc-client/   64-bit command-line client
  loader/       32-bit launcher and injector
  rpc-dll/      32-bit injected library
  rpc-daemon/   64-bit daemon and web API

crates/
  game-client/  Supported game layouts, addresses, and client ABI boundaries
  hook/         transactional in-process x86 detours and trampolines
  model/        shared domain state, actions, and updates
  protocol/     versioned binary IPC framing and codecs
  win32/        shared Windows platform boundaries

tools/
  hook-harness/     controlled x86 detour qualification harness
  injection-target/ inert process for loader integration testing
  lifecycle-host/   local DLL lifecycle integration harness
  loader-fixture-dll/ controlled failure DLL for loader integration testing
  test-daemon.ps1   daemon discovery, lifecycle, and reconnect integration test
  test-hook.ps1     debug and release hook qualification test
  test-ipc.ps1      direct IPC and shutdown integration test

docs/           architecture and developer documentation
```

Reusable library packages use the `darpc-` prefix. Component packages use
concise role names, while their manifests define the intended artifact names.

## Design priorities

- Preserve the stability and normal behavior of the game client.
- Keep hooks bounded, nonblocking, and fail-open.
- Keep client memory and native calls on validated, version-specific boundaries.
- Keep IPC independent from game loops and native client locks.
- Prefer simple, idiomatic Rust over premature abstractions.
- Use a minimal set of common, well-maintained dependencies.

## Requirements

Install Rust with `rustup`. Windows builds also require the current
[Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/)
with the **Desktop development with C++** workload. Ensure the installer
includes the MSVC C++ x64/x86 build tools and a current Windows 11 SDK. The
full Visual Studio IDE is not required.

Install the Rust targets used by the runtime components from a Windows shell:

```text
rustup target add i686-pc-windows-msvc x86_64-pc-windows-msvc
```

Developers working natively on Windows can build and test directly from their
normal checkout. On Apple silicon, Windows 11 Arm in a virtual machine can
build and run the x86 artifacts. macOS can run platform-independent tests and
cross-target checks, but MSVC builds and executable integration tests should
run inside Windows.

### Optional macOS and Parallels workflow

Parallels Desktop can provide the native Windows verification environment while
the repository remains in a macOS checkout. This is optional and is not
required for developers already working on Windows.

Share or mount the checkout into the guest, but discover its guest path rather
than relying on a fixed drive letter. Mapped drives, virtual machine names, and
Windows usernames vary by developer and session. Keep Cargo-generated files on
the Windows-local filesystem. For example, set an environment-specific local
directory in PowerShell before building:

```powershell
$env:CARGO_TARGET_DIR = "C:\cargo-target\da-rpc"
Set-Location "<guest-path-to-repository>"
```

Parallels Pro can invoke guest commands from macOS. First discover the virtual
machine, then execute in the logged-in Windows user context so mapped drives and
the user's Rust environment are available:

```text
prlctl list -a
prlctl exec "<vm-name>" --current-user powershell.exe <arguments>
```

Use direct current-user remote commands for building, repository-owned
integration targets, inspection, attach, and automated live-client launch. No
scheduled task or other launch intermediary is required. A rapid launch from
macOS has this general form:

```sh
prlctl exec "<vm-name>" --current-user powershell.exe -NoProfile -Command \
  "& '<loader-path>' launch --allow-multiple --skip-intro --skip-notice '<client-path>' '<dll-path>'"
```

Add `--server '<host[:port]>'` when endpoint selection is part of the test.
[Arbiter](https://github.com/ewrogers/Arbiter) is a Dark Ages network analyzer
and local proxy; use `--server 127.0.0.1:2610` when Arbiter is configured to
listen there and forward the connection. Automated checks must not enter
credentials, record private game data, or force-terminate a client they do not
clearly own.

The intended development loop is:

1. Run formatting, platform-independent tests, and cross-target checks on
   macOS.
2. Build the required MSVC target inside Windows with a Windows-local
   `CARGO_TARGET_DIR`.
3. Run the repository-owned Windows integration script inside the guest.
4. Launch live-client checks through direct current-user guest execution when
   the milestone requires them.
5. Treat both host and native guest results as completion evidence.

## Loader CLI

`loader.exe` supports process inspection, late attach, detach, and suspended
launch:

```text
loader [--json] inspect <pid>
loader [--json] attach <pid> <dll-path>
loader [--json] detach <pid> <dll-path>
loader [--json] launch [--allow-multiple] [--server <host[:port]>] \
    [--skip-intro] [--skip-notice] \
    <executable-path> <dll-path> [-- <argument>...]
```

The standard launch profile combines all four launch options. It allows another
client instance, selects a strict IPv4 endpoint, skips the intro, hides the
notice, enables early title-menu pointer input, and removes the fixed one-second
transfer delay. Launch options remain explicit so each behavior can be omitted
during diagnosis.

```text
loader.exe launch --allow-multiple --server <host[:port]> \
    --skip-intro --skip-notice <executable-path> <dll-path>
```

`--server` uses port 2610 when omitted. Arbiter can be selected by passing
`--server 127.0.0.1:2610`, but its local proxy must already be listening and
forwarding the connection. Arguments after `--` are forwarded unchanged to the
client. See the [loader documentation](https://ewrogers.github.io/da-rpc/loader.html)
for the detailed lifecycle, safety behavior, and result contract.

## Direct IPC diagnostics

With `darpc.dll` initialized in a process and `darpcd.exe` disconnected, the
64-bit client can exercise the PID-based pipe directly:

```text
darpc.exe ipc hello --pid <pid>
darpc.exe ipc ping --pid <pid>
darpc.exe ipc echo --pid <pid> "hello"
darpc.exe ipc tick-health --pid <pid>
darpc.exe --output json ipc hello --pid <pid>
```

These diagnostic commands perform the real binary handshake and validate
ordering, correlation, and timing. `tick-health` samples the installed client
tick hook twice and reports whether its bounded counter advances. See the
[`darpc.exe` documentation](https://ewrogers.github.io/da-rpc/cli.html) for
output fields and exit codes.

`darpc.exe` does not call the daemon HTTP API. It remains usable with only the
loader and an injected DLL. Multi-client aggregation is available directly
through the `darpcd.exe` web API and its Swagger UI.

## Daemon client registry

`darpcd.exe` discovers running clients by their verified `Darkages` top-level
window class and persistently connects to every available daRPC pipe. Explicit
PIDs remain available for controlled targets or clients without a window:

```text
darpcd.exe
darpcd.exe --pid 3780 --pid 6648
darpcd.exe --auto-load
darpcd.exe --loader-path <loader.exe> --dll-path <darpc.dll>
```

The daemon prints connection status transitions and reconnects when a pipe or
DLL returns. It aggregates identity and connection health only until game-state
snapshots are implemented. While it owns a pipe, direct `darpc.exe ipc`
commands report that the endpoint is busy.

`--auto-load` asks the daemon to load its configured DLL into each `not_loaded`
client once per tracked process. It applies to clients present at startup and
clients discovered later. An explicit unload remains unloaded for the rest of
that tracked process lifetime; restarting the daemon with `--auto-load` makes
the process eligible again. Validation and failures remain isolated per client.

The loader and DLL default to files beside `darpcd.exe` and can be overridden
only through daemon configuration. Each launch request supplies the full path
to that installation's `Darkages.exe`; the loader uses its parent as the client
working directory, so no installation directory is assumed.

## Web API

The daemon uses Axum and listens on `127.0.0.1:2626` by default. A single
`--port <port>` option overrides the port while keeping the listener on
loopback. It exposes one current, unversioned API:

```text
GET /health
GET /clients
POST /clients/launch
POST /clients/{pid}/load
POST /clients/{pid}/unload
GET /openapi.json
GET /docs
```

The default interactive documentation URL is
`http://127.0.0.1:2626/docs`. Startup fails clearly if the selected port is
unavailable rather than silently choosing another one. `/clients` reports each
discovered or explicitly configured PID and status, plus the DLL `instance_id`
and process `created_time` once identity is available.

Managed launch requires `client_path` and accepts `allow_multiple`,
`skip_intro`, `skip_notice`, and an optional `server` string in `host` or
`host:port` form. A missing port defaults to `2610`. Arbitrary client arguments
and request-selected loader or DLL paths are intentionally not part of the API.

`utoipa` generates the OpenAPI document from the Rust HTTP models. A vendored
Swagger UI serves the same contract at `/docs` without requiring internet
access and uses an Ayu-inspired dark theme. `/openapi.json` can be imported
into tools such as Postman and Apidog.

## Development

The workspace uses Rust 2024. The injected-process components target 32-bit
Windows, while the daemon and command-line client target 64-bit Windows:

```text
rpc-dll, loader, lifecycle-host, injection-target: i686-pc-windows-msvc
rpc-client, rpc-daemon:                          x86_64-pc-windows-msvc
```

The shared crates can be checked together on a supported development host:

```text
cargo check -p darpc-model -p darpc-protocol
```

Platform component checks should specify their intended Windows target. Build
and test instructions will grow alongside the implementation.

Coding agents may implement requested changes and also act as reviewers,
debugging partners, and mentors. See [AGENTS.md](AGENTS.md) for the complete
collaboration and engineering rules.

All commits should follow the [Conventional Commits](https://www.conventionalcommits.org/)
format with a short, focused, imperative summary.

## Documentation

The book contains the detailed state model, discovery design, IPC and HTTP
protocols, and planned Server-Sent Events and WebSocket interfaces.

The [development roadmap](docs/src/roadmap.md) divides the work into small
increments with a visible demonstration and exit checks for each milestone.
The [command-line interface](docs/src/cli.md) documents implemented direct IPC
diagnostics and the planned daemon command hierarchy.

Build and serve it locally with the pinned mdBook version:

```text
cargo install mdbook --version 0.5.4 --locked
mdbook serve docs --open
```

## License

daRPC is available under the [MIT License](LICENSE).

## Legal disclaimer

*Dark Ages* is copyright Nexon Korea Corporation and is licensed to KRU
Interactive in the United States and Canada. All rights reserved.

daRPC is an independent project for educational, research, and interoperability
purposes. It is not affiliated with or endorsed by Nexon Korea Corporation or
KRU Interactive.
