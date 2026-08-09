# daRPC

[![Documentation](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml)

daRPC is a Rust toolkit for observing and controlling the 32-bit Windows client
of *Dark Ages*. It attaches a small DLL to the client, reads the state the client
already knows, and performs actions through the client's own code.

Start with the [daRPC Book](https://ewrogers.github.io/da-rpc/) for the complete
documentation, or continue below for a quick overview.

> daRPC is in active development and currently supports one exact 7.41 client
> build. It is intended for education, research, interoperability, and
> user-controlled automation.

## Why daRPC?

Network proxies are excellent tools for inspecting game traffic, but the wire
only tells part of the story. A proxy may need to redirect the client, recreate
state from packets, and guess at details that exist only inside the client or
its user interface.

daRPC takes a different approach. Its DLL can attach to a running client and
detach cleanly when it is no longer needed. This gives tools direct access to
the client's current state without requiring all traffic to pass through a
proxy. You can still use a network analyzer such as
[Arbiter](https://github.com/ewrogers/Arbiter) alongside daRPC when packet-level
visibility is useful.

Actions also travel through the client's native methods instead of being
created as injected network packets. This has a few important benefits:

- The client user interface reflects actions normally.
- Client-side timing and validation stay in the normal execution path.
- Built-in pathfinding handles movement instead of an external controller
  sending every step.
- Player input can naturally interrupt or replace a path without competing
  with a separate movement loop.

The result is an integration that behaves more like part of the client and less
like a second client trying to imitate it from the outside.

## Highlights

- Attach to an existing client or launch a new one.
- Detach safely without closing the game client.
- Read character status, inventory, equipment, spellbook, skillbook, effects,
  visible objects, and recent messages.
- Follow changes as they happen through an ordered event stream.
- Turn, walk, use items and skills, cast spells, move items and gold, unequip
  gear, pick up ground items, and emote through normal client behavior.
- Observe and answer merchant and pursuit dialogs through native client UI
  methods.
- Read group rosters, invite visible players, and answer group invitations.
- Manage several clients from one daemon.
- Query state and submit actions through REST.
- Subscribe to live events through Server-Sent Events (SSE).
- Explore the API through its built-in Swagger UI and OpenAPI document.
- Talk directly to one DLL with a lightweight command-line client when a daemon
  is unnecessary.

REST and SSE are the supported web interfaces. REST handles current state and
bounded actions, while SSE delivers live changes without requiring a second
bidirectional protocol.

## How it works

```text
                         direct commands
                    +---------------------- darpc.exe
                    |
Darkages.exe <-> darpc.dll <-> named pipe <-> darpcd.exe <-> REST / SSE
     ^                                                       OpenAPI / Swagger
     |
 loader.exe
```

`loader.exe` launches a supported client or attaches `darpc.dll` to one that is
already running. Each DLL maintains the state for its own client and exposes a
process-specific named pipe. `darpc.exe` can use that pipe directly, while
`darpcd.exe` discovers multiple clients and presents them through a web API.

Small hooks observe the places where the client receives new state. They copy
only the information needed and hand off the heavier work. Actions that depend
on the client's main thread are scheduled there, so native methods run in the
context they expect. During detach, daRPC stops new work, removes its hooks, and
then unloads the DLL.

This keeps the timing-sensitive parts short and lets the client continue to run
if the daemon disconnects or restarts.

## Components

| File | Purpose |
| --- | --- |
| `darpc.dll` | Lives inside one game client, tracks its state, and runs native actions. |
| `loader.exe` | Launches clients and attaches or detaches the DLL. |
| `darpc.exe` | Talks directly to one injected client and prints text or JSON. |
| `darpcd.exe` | Discovers clients and exposes their state and actions through web APIs. |

The DLL and loader are 32-bit x86 programs because the game client is 32-bit.
The command-line client and daemon are 64-bit x86-64 programs.

## Getting started

### Requirements

- Windows with the supported *Dark Ages* client
- A current stable Rust toolchain
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/)
  with the Desktop development with C++ workload and a current Windows SDK
- The `i686-pc-windows-msvc` and `x86_64-pc-windows-msvc` Rust targets

Install the Rust targets from a Windows shell:

```text
rustup target add i686-pc-windows-msvc x86_64-pc-windows-msvc
```

Build the 32-bit client components from a Visual Studio x86 developer shell:

```text
cargo build -p loader -p rpc-dll --target i686-pc-windows-msvc
```

Build the 64-bit tools from a Visual Studio x64 developer shell:

```text
cargo build -p rpc-client -p rpc-daemon --target x86_64-pc-windows-msvc
```

See the book's [development guide](https://ewrogers.github.io/da-rpc/development.html)
for testing, macOS with Parallels, and repository-specific build guidance.

## Usage

### Attach to a running client

```text
loader.exe attach <pid> <path-to-darpc.dll>
```

Inspect or detach it later:

```text
loader.exe inspect <pid>
loader.exe detach <pid> <path-to-darpc.dll>
```

### Launch a client

```text
loader.exe launch --allow-multiple --skip-intro --skip-notice \
    <path-to-Darkages.exe> <path-to-darpc.dll>
```

The loader can also select a server endpoint for local development and network
analysis. See the [loader guide](https://ewrogers.github.io/da-rpc/loader.html)
for every launch option.

### Use one client directly

`darpc.exe` is the simplest way to inspect or control one injected client. It
uses the binary named-pipe protocol directly and does not require the daemon.

```text
darpc.exe hello --pid <pid>
darpc.exe snapshot --pid <pid>
darpc.exe turn --pid <pid> north
darpc.exe walk --pid <pid> 120 85
darpc.exe item-drop --pid <pid> 1 120 85
darpc.exe item-give --pid <pid> 1 <object-id>
darpc.exe skill-use --pid <pid> 5
darpc.exe spell-cast --pid <pid> 2 --target-id <object-id>
darpc.exe --output json snapshot --pid <pid>
```

See the [`darpc.exe` guide](https://ewrogers.github.io/da-rpc/cli.html) for the
full command reference.

### Run the daemon

Start `darpcd.exe` to discover and aggregate clients:

```text
darpcd.exe
```

To inject the DLL into discovered clients automatically, provide the loader and
DLL paths and enable auto-load:

```text
darpcd.exe --auto-load --loader-path <loader.exe> --dll-path <darpc.dll>
```

The daemon listens on `127.0.0.1:2626` by default. Open
`http://127.0.0.1:2626/docs` for Swagger UI or
`http://127.0.0.1:2626/openapi.json` for the OpenAPI document.

## Web API

The web API is designed for scripts, desktop tools, dashboards, and other local
integrations.

- **REST** reads current state and submits actions.
- **SSE** streams ordered state, message, object, and action events as they
  happen.
- **OpenAPI** describes the REST surface for tools such as Postman and API
  client generators.
- **Swagger UI** provides an interactive API browser without extra setup.
- **REST and SSE** provide real-time interaction without duplicating command
  validation, flow control, reconnection, and ordering across another transport.

Clients can be addressed by process ID or, while in game, by character name.
Some representative routes are:

```text
GET  /clients
GET  /clients/{client}/status
GET  /clients/{client}/items
GET  /clients/{client}/skills
GET  /clients/{client}/spells
GET  /clients/{client}/objects
GET  /clients/{client}/events
POST /clients/{client}/turn
POST /clients/{client}/walk
POST /clients/{client}/skills/use
POST /clients/{client}/spells/cast
POST /clients/{client}/items/use
POST /clients/{client}/items/drop
POST /clients/{client}/items/pickup
POST /clients/{client}/gold/drop
POST /clients/{client}/equipment/unequip
POST /clients/{client}/emote
POST /clients/launch
POST /clients/{client}/load
POST /clients/{client}/unload
```

The [web API guide](https://ewrogers.github.io/da-rpc/web-api.html) explains
routes, requests, and errors. The [live event reference](https://ewrogers.github.io/da-rpc/events.html)
documents every SSE event, payload, ordering rule, and reconnect procedure.

## Project status

daRPC is not a general-purpose game injection framework. It is deliberately
specific to one supported client build and validates that build before using
version-specific addresses or hooks.

The current implementation includes client lifecycle management, local state,
event-driven updates, native movement, skill and spell actions, REST, SSE, and
direct binary IPC. See the [roadmap](https://ewrogers.github.io/da-rpc/roadmap.html)
for current work and planned features.

## Development

The repository is a Rust workspace organized by runtime component and shared
domain boundary:

```text
components/   DLL, loader, command-line client, and daemon
crates/       game layout, hooks, models, protocol, and Windows support
tools/        lifecycle, injection, hook, and integration test harnesses
docs/         mdBook source
```

Before contributing, read the
[development guide](https://ewrogers.github.io/da-rpc/development.html) and
[repository conventions](AGENTS.md). Use short
[Conventional Commits](https://www.conventionalcommits.org/) and include focused
tests and documentation with behavioral changes.

## License

daRPC is available under the [MIT License](LICENSE).

## Legal disclaimer

*Dark Ages* is copyright Nexon Korea Corporation and is licensed to KRU
Interactive in the United States and Canada. All rights reserved.

daRPC is an independent project for educational, research, and interoperability
purposes. It is not affiliated with or endorsed by Nexon Korea Corporation or
KRU Interactive.
