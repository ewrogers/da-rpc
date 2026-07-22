# Development

The Rust workspace is scaffolded, but it does not yet contain an implementation.
Its packages establish the intended component and library boundaries:

| Package | Role |
| --- | --- |
| `darpc-model` | Shared domain state, actions, and updates. |
| `darpc-protocol` | Versioned binary interprocess communication framing and codecs. |
| `darpc-win32` | Shared Windows platform boundaries. |
| `darpc-client-741` | Version-specific client layouts and application binary interface boundaries. |
| `rpc-dll` | Injected client component. |
| `loader` | Client launcher and injector. |
| `rpcd` | Client aggregator and web API daemon. |

The planned runtime targets are:

| Component | Rust target |
| --- | --- |
| `rpc.dll` | `i686-pc-windows-msvc` |
| `loader.exe` | `i686-pc-windows-msvc` |
| `rpcd.exe` | `x86_64-pc-windows-msvc` |

The shared crates can be checked independently of the Windows components:

```text
cargo check -p darpc-model -p darpc-protocol
```

Component builds and checks should specify their intended target. Detailed
commands will be added as platform-specific implementation is introduced.

## Documentation

The repository pins mdBook 0.5.4 for reproducible local and CI builds.

```text
cargo install mdbook --version 0.5.4 --locked
mdbook build docs
mdbook serve docs --open
```

Pull requests that change the book run the documentation build. Pushes to
`main` build the same sources and deploy the generated artifact to GitHub
Pages.

## Collaboration

The project owner writes implementation code. Agents act as reviewers,
mentors, debugging partners, and pair-programming partners. Agents may write
focused unit tests, but production implementation remains with the project
owner. See the repository's
[AGENTS.md](https://github.com/ewrogers/da-rpc/blob/main/AGENTS.md) for the full
guidance.

## Commits

Use Conventional Commits with focused imperative summaries:

```text
feat(protocol): add handshake negotiation
fix(loader): validate target process architecture
docs(book): explain daemon recovery
test(state): cover incomplete initial snapshots
```

Do not use emoji or em dashes in code, documentation, or commit messages.
