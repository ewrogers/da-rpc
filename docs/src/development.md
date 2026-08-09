# Development

This chapter is for contributors building, testing, documenting, or reviewing
daRPC. Player-facing API usage is documented under [Using daRPC](web-api.md).

The Rust workspace separates runtime components from shared domain and platform
boundaries:

| Package | Role |
| --- | --- |
| `darpc-model` | Shared domain state, actions, and updates. |
| `darpc-protocol` | Versioned binary interprocess communication framing and codecs. |
| `darpc-win32` | Shared Windows platform boundaries. |
| `darpc-game-client` | Supported game-client layouts and application binary interface boundaries. |
| `rpc-client` | Direct single-client binary protocol command-line interface. |
| `rpc-dll` | Injected client component. |
| `loader` | Client launcher and injector. |
| `rpc-daemon` | Client aggregator and web API daemon. |

The project supports one exact game-client build at a time. `darpc-game-client`
keeps its verified fingerprint, layouts, addresses, and application binary
interface assumptions together. Supporting another build requires updating or
forking that contract rather than adding parallel version-named crates.

The runtime targets are:

| Component | Rust target |
| --- | --- |
| `darpc.dll` | `i686-pc-windows-msvc` |
| `loader.exe` | `i686-pc-windows-msvc` |
| `darpc.exe` | `x86_64-pc-windows-msvc` |
| `darpcd.exe` | `x86_64-pc-windows-msvc` |

The shared crates can be checked independently of the Windows components:

```text
cargo check -p darpc-model -p darpc-protocol
```

Component builds and checks should specify their intended target.

On Windows Arm, a native Arm64 Rust toolchain needs the matching Arm64 MSVC
libraries to link dependency build scripts and procedural macros. When the VM
has only the x64 MSVC tools, install Rust's x64 host toolchain and run it under
Windows x64 emulation from an x64 Developer Command Prompt:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc `
    --profile minimal `
    --force-non-host
rustup +stable-x86_64-pc-windows-msvc target add i686-pc-windows-msvc

cargo +stable-x86_64-pc-windows-msvc build -p rpc-daemon -p rpc-client
cargo +stable-x86_64-pc-windows-msvc build `
    -p loader -p rpc-dll -p injection-target `
    --target i686-pc-windows-msvc
```

This workaround is unnecessary on native x64 Windows or when the Arm64 MSVC
workload is installed.

The controlled IPC integration test requires both architectures. Keep build
outputs under one stable Windows-local target root and reuse it across builds.
Cargo separates the explicit target architectures below that root, so
milestone- or test-specific target trees are unnecessary. Then pass the two
artifact directories to the script:

```powershell
$env:CARGO_TARGET_DIR = "C:\cargo-target\da-rpc"

cargo build -p loader -p rpc-dll -p injection-target `
    --target i686-pc-windows-msvc
cargo build -p rpc-client --target x86_64-pc-windows-msvc

& .\tools\test-ipc.ps1 `
    -X86TargetDir "$env:CARGO_TARGET_DIR\i686-pc-windows-msvc\debug" `
    -X64TargetDir "$env:CARGO_TARGET_DIR\x86_64-pc-windows-msvc\debug"
```

The script uses the inert `injection-target.exe` and a debug-only unsupported
client bypass. It verifies hello, ping, byte-exact echo, tick-hook
health, missing and busy pipe errors, malformed-client isolation, reconnect,
and bounded cancellation during shutdown. The controlled target reports the
hook as not installed. Its DLL log must contain the skipped-hook and health
sample records. The bypass is unavailable in release builds and is never a
substitute for validation against the supported client.

The daemon registry integration test uses two controlled targets and both
runtime architectures:

```powershell
cargo build -p loader -p rpc-dll -p injection-target `
    --target i686-pc-windows-msvc
cargo build -p rpc-client -p rpc-daemon `
    --target x86_64-pc-windows-msvc

& .\tools\test-daemon.ps1 `
    -X86TargetDir "$env:CARGO_TARGET_DIR\i686-pc-windows-msvc\debug" `
    -X64TargetDir "$env:CARGO_TARGET_DIR\x86_64-pc-windows-msvc\debug"
```

It starts the daemon before injection, connects both targets, verifies exclusive
pipe ownership, inspects both identities through `/clients`, checks
`/health`, OpenAPI 3.1, and vendored Swagger assets, and exercises the default
and overridden HTTP ports. It then restarts the daemon, replaces one DLL
instance, confirms the other client stays connected, and verifies occupied-port
failure. Incompatible negotiation is exercised by the native Windows
controller-session test.

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

Agents may implement requested changes and also act as reviewers, mentors,
debugging partners, and pair-programming partners. The project owner sets
product direction and retains ownership of the repository. See the repository's
[AGENTS.md](https://github.com/ewrogers/da-rpc/blob/main/AGENTS.md) for the full
guidance.

## Commits

Use Conventional Commits with short, focused imperative summaries:

```text
feat(protocol): add handshake negotiation
fix(loader): validate target process architecture
docs(book): explain daemon recovery
test(state): cover incomplete initial snapshots
```

Do not use emoji or em dashes in code, documentation, or commit messages.
