# Development

The Rust workspace has not yet been introduced. Build and test instructions
will be added when there is an implementation to build.

The planned runtime targets are:

| Component | Rust target class |
| --- | --- |
| `rpc.dll` | 32-bit Windows x86 |
| `loader.exe` | 32-bit Windows x86 |
| `rpcd.exe` | 64-bit Windows x86-64 |

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
mentors, and pair-programming partners. Agents may write unit tests only when
explicitly requested. See the repository's
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
