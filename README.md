# daRPC

[![Documentation](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml)

daRPC, short for Dark Ages Remote Procedure Call, is a Rust workspace for
integrating developer tools with the 32-bit Windows client of *Dark Ages*.
The project is in early development and does not yet provide a working client
integration.

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
| `darpc.exe` | 64-bit Windows x86-64 | Provides direct IPC diagnostics and a user-facing daemon client. |
| `darpcd.exe` | 64-bit Windows x86-64 | Discovers clients, aggregates state and events, and exposes web APIs. |

The DLL remains independent of the daemon. If `darpcd.exe` is stopped or
restarted, an injected client must continue operating normally and accept a new
daemon connection later.

## Workspace

```text
components/
  rpc-client/   64-bit command-line client
  loader/       32-bit launcher and injector
  rpc-dll/      32-bit injected library
  rpc-daemon/   64-bit daemon and web API

crates/
  client-741/   Dark Ages 7.41 layouts, addresses, and client ABI boundaries
  model/        shared domain state, actions, and updates
  protocol/     versioned binary IPC framing and codecs
  win32/        shared Windows platform boundaries

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

## Development

The workspace uses Rust 2024. The injected-process components target 32-bit
Windows, while the daemon and command-line client target 64-bit Windows:

```text
rpc-dll, loader:  i686-pc-windows-msvc
rpc-client, rpc-daemon: x86_64-pc-windows-msvc
```

The shared crates can be checked together on a supported development host:

```text
cargo check -p darpc-model -p darpc-protocol
```

Platform component checks should specify their intended Windows target. Build
and test instructions will grow alongside the implementation.

The project owner writes implementation code. Coding agents act as reviewers,
debugging partners, and mentors, and may help with tests when requested. See
[AGENTS.md](AGENTS.md) for the complete collaboration and engineering rules.

All commits should follow the [Conventional Commits](https://www.conventionalcommits.org/)
format with a focused, imperative summary.

## Documentation

The [daRPC Book](https://ewrogers.github.io/da-rpc/) contains the detailed
architecture, state model, discovery design, safety requirements, IPC protocol,
and planned HTTP, Server-Sent Events, and WebSocket interfaces.

The [development roadmap](docs/src/roadmap.md) divides the work into small
increments with a visible demonstration and exit checks for each milestone.
The [tentative command-line interface](docs/src/cli.md) describes the planned
`darpc.exe` command hierarchy, client selection, typed actions, and JSON output.

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
