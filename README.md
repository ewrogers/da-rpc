# daRPC

[![Documentation](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml/badge.svg)](https://github.com/ewrogers/da-rpc/actions/workflows/docs.yml)

daRPC, short for Dark Ages Remote Procedure Call, is a Rust workspace for
directly integrating tools with the Windows client of *Dark Ages*. It replaces
the traditional network-proxy approach with an injected client library and a
portable web API.

The project is in early development.

## Why daRPC exists

Network proxies must decode and re-encode traffic, add another failure point,
cannot attach to an existing client, and cannot observe client-only state.
daRPC instead works at the client's internal event boundary. This allows it to
observe and inject events while leaving network serialization and encryption
to the client.

Working inside the client also makes it possible to track both game-world state
and local user interface state, including panes, dialogs, selections, and focus
that may never be represented by a network packet.

## Components

| Component | Target | Purpose |
| --- | --- | --- |
| `rpc.dll` | 32-bit Windows x86 | Integrates with the client and maintains local game and UI state. |
| `loader.exe` | 32-bit Windows x86 | Launches a client with daRPC or injects `rpc.dll` into a compatible running client. |
| `rpcd.exe` | 64-bit Windows x86-64 | Discovers clients, aggregates state and real-time events, and exposes REST, Server-Sent Events, and WebSocket APIs. |

## Documentation

The full architecture and development documentation is available in the
[daRPC Book](https://ewrogers.github.io/da-rpc/).

To build and serve the book locally:

```text
cargo install mdbook --version 0.5.4 --locked
mdbook serve docs --open
```

The documentation is built for pull requests and deployed to GitHub Pages from
`main`.

## Development

Build instructions will be added as the Rust workspace is introduced. Changes
must follow Rust conventions and use Conventional Commits. See
[AGENTS.md](AGENTS.md) for repository-wide engineering and collaboration
guidance.

## License

daRPC is available under the [MIT License](LICENSE).

## Legal disclaimer

*Dark Ages* is copyright Nexon Korea Corporation and is licensed to KRU
Interactive in the United States and Canada. All rights reserved.

daRPC is an independent project for educational, research, and interoperability
purposes. It is not affiliated with or endorsed by Nexon Korea Corporation or
KRU Interactive.
