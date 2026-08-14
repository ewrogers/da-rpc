# daRPC

[![Documentation](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml)
[![Windows](https://github.com/ewrogers/da-rpc/actions/workflows/windows.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/windows.yml)

daRPC is a Windows integration toolkit for observing and controlling the
32-bit *Dark Ages* client. It attaches a small Rust DLL to the client, reads
the state the client already knows, and performs actions through the client's
own code.

The [daRPC Book](https://ewrogers.github.io/da-rpc/) is the complete user and
developer reference. This README covers the project at a glance and the
shortest path to a working installation.

> daRPC 1.0 supports one exact 7.41 client build. It validates that build before
> using version-specific addresses or installing hooks.

## Overview

daRPC provides typed access to character state, inventory, equipment, skills,
spells, effects, world objects, dialogs, groups, exchanges, online players,
legend marks, chat, movement, and supported client actions. Applications can
use a direct command-line interface or a local REST and Server-Sent Events API.

The daemon publishes an OpenAPI 3.1 document and hosts a vendored Swagger UI at
`http://127.0.0.1:2626/docs`. The documentation UI works without an internet
connection and can execute requests against the local daemon.

## Goals

- Expose useful client state and actions through small, typed interfaces.
- Preserve the client's native behavior instead of reimplementing game logic.
- Validate every supported executable before touching version-specific memory.
- Isolate failures so a tool or daemon disconnect cannot terminate the client.
- Keep local use simple, observable, and scriptable.

## Components

| Component | Architecture | Role |
|---|---:|---|
| `darpc.dll` | 32-bit x86 | Runs inside the supported client, owns local state, and exposes a PID-specific named pipe. |
| [`loader.exe`](https://ewrogers.github.io/da-rpc/loader.html) | 32-bit x86 | Inspects, attaches to, detaches from, or launches a supported client. |
| [`darpc.exe`](https://ewrogers.github.io/da-rpc/cli.html) | 64-bit x86-64 | Sends one direct command to one injected client. |
| [`darpcd.exe`](https://ewrogers.github.io/da-rpc/rpcd.html) | 64-bit x86-64 | Discovers clients and serves aggregate REST, events, OpenAPI, and Swagger UI. |

The [executable components
chapter](https://ewrogers.github.io/da-rpc/executables.html) explains which
program to choose. Each executable has a separate guide with complete syntax,
flags, examples, output behavior, and operational notes.

## Prerequisites

To use a release you need:

- 64-bit Windows running the supported 32-bit *Dark Ages* 7.41 client build.
- Permission to launch or attach to a client process owned by your account.
- The complete daRPC release directory. Keep its files together.

To build daRPC, also install:

- Rust stable with the `i686-pc-windows-msvc` and
  `x86_64-pc-windows-msvc` targets.
- Visual Studio Build Tools with the Desktop development with C++ workload.
- PowerShell for repository verification and release packaging scripts.

## Getting started

### Download a release

1. Download the Windows archive and its `.sha256` file from the [latest
   release](https://github.com/ewrogers/da-rpc/releases/latest).
2. Verify the archive checksum:

   ```powershell
   Get-FileHash .\da-rpc-v1.4.2-windows.zip -Algorithm SHA256
   Get-Content .\da-rpc-v1.4.2-windows.zip.sha256
   ```

3. Extract the archive to a directory you control.
4. Start and sign in to the supported game client.
5. From the extracted directory, run:

   ```powershell
   .\darpcd.exe --auto-load
   ```

6. Open `http://127.0.0.1:2626/docs` to explore the API in Swagger UI.

The daemon discovers supported client windows, loads `darpc.dll` when needed,
and serves on the local loopback interface by default. Use `loader.exe` when
you want explicit process lifecycle control, or `darpc.exe` for one-off
terminal and script commands.

The release directory contains:

- `darpc.dll`, `loader.exe`, `darpc.exe`, and `darpcd.exe`
- `openapi.json`, the static OpenAPI 3.1 document for code generation and tools
- `README.md`, `LICENSE`, and `SHA256SUMS`

`SHA256SUMS` covers every file in the extracted bundle. The adjacent
`.zip.sha256` file covers the archive itself.

Windows binaries are currently unsigned. Microsoft Defender SmartScreen may
show an unrecognized-app warning because the release has no code-signing
reputation. Verify the published checksum before running it. Do not disable
antivirus or security protections to use daRPC.

### Build from source

Install the targets once:

```powershell
rustup target add i686-pc-windows-msvc x86_64-pc-windows-msvc
```

Build the 32-bit injected components and 64-bit tools:

```powershell
cargo build -p loader -p rpc-dll --target i686-pc-windows-msvc --release
cargo build -p rpc-client -p rpc-daemon --target x86_64-pc-windows-msvc --release
```

See the [development
chapter](https://ewrogers.github.io/da-rpc/development.html) for the full build,
test, documentation, Windows verification, and packaging workflow.

## API and documentation

While `darpcd.exe` is running:

| Resource | URL |
|---|---|
| Swagger UI | `http://127.0.0.1:2626/docs` |
| OpenAPI JSON | `http://127.0.0.1:2626/openapi.json` |
| Client registry | `http://127.0.0.1:2626/clients` |
| Server-Sent Events | `http://127.0.0.1:2626/events` |

The same specification is included as `openapi.json` in release bundles. It can
also be exported directly from the matching daemon binary:

```powershell
.\darpcd.exe --print-openapi > openapi.json
```

To access the API from a host or another virtual machine on a trusted network,
bind an explicit IPv4 interface, for example `darpcd.exe --listen
0.0.0.0:2626`. This exposes every API route, including Swagger UI. There is no
authentication or TLS, so restrict the port with Windows Firewall and do not
expose it to an untrusted network.

Detailed references:

- [Web API and Swagger UI](https://ewrogers.github.io/da-rpc/web-api.html)
- [Live events](https://ewrogers.github.io/da-rpc/events.html)
- [Raw packets and safety](https://ewrogers.github.io/da-rpc/raw.html)
- [Executable components](https://ewrogers.github.io/da-rpc/executables.html)
- [Game data](https://ewrogers.github.io/da-rpc/state.html)
- [Architecture](https://ewrogers.github.io/da-rpc/architecture.html)
- [Safety and security](https://ewrogers.github.io/da-rpc/safety.html)

## Version support

Version-specific memory layouts and hooks are deliberate safety boundaries.
daRPC refuses unsupported executables instead of guessing. See [runtime hooks
and compatibility](https://ewrogers.github.io/da-rpc/hooks.html) for the exact
validation model.

## Development

Repository-wide contributor and agent guidance lives in [AGENTS.md](AGENTS.md).
Changes should remain focused, include appropriate tests and documentation, and
use [Conventional Commits](https://www.conventionalcommits.org/).

## License

daRPC is licensed under the [MIT License](LICENSE).

## Legal disclaimer

*Dark Ages* is copyright Nexon Korea Corporation and is licensed to KRU
Interactive in the United States and Canada. All rights reserved.

daRPC is an independent project for educational, research, and interoperability
purposes. It is not affiliated with or endorsed by Nexon Korea Corporation or
KRU Interactive.
