# Repository guidance

This file defines the repository-wide expectations for human contributors and
coding agents. More specific `AGENTS.md` files may be added within the
workspace. When guidance conflicts, the file closest to the changed code takes
precedence.

## Project context

daRPC is a Rust workspace for integrating with the 32-bit Windows client of
*Dark Ages*. Its primary runtime components are:

- `darpc.dll`, a 32-bit x86 library injected into the game client.
- `loader.exe`, a 32-bit x86 executable that launches or injects the library.
- `darpc.exe`, a 64-bit x86-64 command-line client for diagnostics and the
  daemon API.
- `darpcd.exe`, a 64-bit x86-64 daemon that aggregates client state and real-time
  events and presents REST, SSE, and WebSocket APIs.

`darpc.dll` communicates with `darpcd.exe` through a versioned binary protocol over
process-specific Windows named pipes. `darpc.dll` owns the initial and updating
local state for its process. `darpcd.exe` is a query layer, aggregator, and
real-time event listener. Supporting crates should separate protocol types,
platform abstractions, client layouts, and shared behavior where that
separation creates a clear boundary.

## Engineering philosophy

Keep the project simple, intuitive, and idiomatic. Less is more.

- Prefer the smallest design that clearly satisfies the current requirement.
- Follow established Rust conventions before introducing project-specific
  patterns.
- Add abstractions only when they create a concrete boundary or remove proven
  repetition.
- Avoid speculative generality, unnecessary indirection, and framework-like
  internal APIs.
- Keep modules, types, and functions focused and name them after the concepts
  they represent.
- Organize code according to the runtime components, shared crates, and Dark
  Ages domain vocabulary documented by the project.
- Reuse confirmed client and game terminology. Do not invent alternate names
  for established concepts without a documented reason.

## Collaboration role and code ownership

The project owner writes the implementation code. Agents serve as reviewers,
mentors, and pair-programming partners, not as implementation authors.

- Do not create or modify production code, build scripts, manifests, migrations,
  or generated implementation files.
- Do not apply a suggested production-code fix, even when the required change
  appears mechanical. Explain the change and let the project owner implement
  it.
- When asked how to begin or proceed, explain the relevant concepts, identify
  tradeoffs, propose a small sequence of next steps, and point to appropriate
  Rust or Windows APIs.
- Prefer pseudocode, signatures, focused examples, and review comments over
  complete drop-in implementations. Make examples clearly illustrative rather
  than edits to the repository.
- Review code for correctness, safety, architecture, maintainability, and test
  coverage. Explain both the issue and the reasoning behind a recommendation.
- Ask questions that help the project owner make design decisions instead of
  silently making consequential choices on their behalf.
- Agents may write or modify focused unit tests while helping the project owner.
  This includes co-located `#[cfg(test)]` modules, but does not permit changes
  to the non-test implementation in the same file.
- Agents may run diagnostics, investigate failures, and suggest concrete fixes.
  They must leave production implementation changes for the project owner to
  type and review.
- Agents may change documentation, agent instructions, or repository
  configuration only when the project owner explicitly requests that specific
  kind of change.
- During reviews, use check-only formatter modes such as `cargo fmt --all --
  --check`. Do not let a formatter rewrite project files.

If a request could mean either advice or implementation, treat it as a request
for advice. The project owner retains authorship and ownership of the code.

## General workflow

1. Read the relevant code and documentation before editing.
2. Respect the collaboration and code-ownership boundary above.
3. Keep any explicitly requested non-production-code changes focused on the
   requested behavior.
4. Preserve unrelated user changes in a dirty worktree.
5. Recommend tests and documentation that should accompany behavioral changes.
6. Run the narrowest useful read-only checks during review, then all applicable
   workspace checks before handing off.
7. Report checks that could not be run and why.

Do not claim that a component, protocol operation, or client version is
supported until its implementation and verification exist in the repository.

## Rust conventions

- Follow stable Rust conventions unless a crate documents a justified nightly
  requirement.
- Prefer straightforward Rust that is easy to read and debug over clever or
  highly abstract code.
- Use standard Rust naming conventions and keep package, module, type, and
  function names aligned with workspace and game-domain terminology.
- Format Rust code with `cargo fmt --all`.
- Run Clippy with `cargo clippy --workspace --all-targets --all-features --
  -D warnings` when the configured targets are installed.
- Run tests with `cargo test --workspace --all-features` where platform and
  target constraints permit.
- Prefer explicit, domain-specific types over primitive values with implicit
  meanings.
- Use checked conversions and arithmetic at process, protocol, memory, and ABI
  boundaries.
- Return structured errors with useful context. Reserve panics for violated
  internal invariants, not malformed input or environmental failures.
- Avoid blocking an async executor. Keep synchronous Windows IPC and process
  work behind an appropriate boundary.
- Keep public APIs small and document safety, ownership, threading, and wire
  compatibility expectations.

## Dependencies

Use the minimal set of blessed libraries. Before adding or enabling a
dependency:

- Prefer common, well-maintained Rust libraries with established use in the
  relevant problem domain.
- Confirm the standard library or an existing workspace dependency is not
  sufficient.
- Explain the need in the change description.
- Disable unused default features.
- Consider maintenance, licensing, binary size, compile time, and compatibility
  with the required 32-bit target.
- Keep dependency versions centralized in the workspace manifest when a
  dependency is shared.

Do not introduce a second library for a capability the workspace already has
without a documented reason. Do not add a dependency to avoid a small amount of
clear, conventional Rust code.

## Unsafe code and client memory

The injected library necessarily crosses unsafe Windows, process, and client
memory boundaries. Treat those boundaries as small audited interfaces.

- Deny unsafe operations in unsafe functions where practical and use explicit
  `unsafe` blocks.
- Precede each `unsafe` block with a `SAFETY:` comment that states the
  invariants making the operation valid.
- Never construct references from client memory until address, alignment,
  lifetime, size, and readability assumptions have been validated.
- Model client structures as version-specific layouts. Do not silently reuse
  offsets or RVAs for an unverified executable version.
- Validate module identity and supported client version before installing
  hooks or reading version-specific state.
- Check pointer chains and lengths at every trust boundary. Fail closed when an
  invariant cannot be established.
- Avoid doing allocation, IPC, logging, or other unbounded work while holding
  client locks or executing in time-sensitive hook paths.
- Define calling conventions, integer widths, packing, and ownership explicitly
  for every foreign function or client ABI boundary.
- Do not unwind across FFI or hook boundaries. Catch failures at the boundary
  and disable the affected integration safely where possible.

## Hooks and injection

- Hook installation and removal must be transactional and safe to repeat.
- Preserve original behavior unless an event is explicitly and validly
  blocked.
- Ensure hooks cannot use daemon-owned resources after disconnect or unload.
- Keep `loader.exe` compatible with the target process architecture.
  Cross-bitness assumptions must be documented and tested.
- Do not perform substantial work under the Windows loader lock. Keep
  `DllMain` minimal and defer initialization and cleanup appropriately.
- A daemon, client, or consumer disconnect must not terminate the game process.

## Protocol and IPC

- Treat all pipe input as untrusted, including input from the local machine.
- Frame messages with explicit sizes and enforce conservative upper bounds
  before allocating.
- Version the protocol and negotiate or reject incompatible peers explicitly.
- Specify byte order, integer widths, discriminants, optional fields, and
  string encoding. Never rely on Rust memory layout as a wire format.
- Keep parsing separate from dispatch. Malformed or unknown messages must not
  cause undefined behavior or desynchronize subsequent frames.
- Apply bounded queues and deliberate backpressure policies. A slow consumer
  must not block a game hook or grow memory without limit.
- Associate each pipe endpoint with the intended process and validate peer
  identity where the platform permits.
- Never log credentials, authentication material, private chat, or complete
  sensitive packet payloads by default.

## State model

- Keep `darpc.dll` state tracking independent of the presence or health of
  `darpcd.exe`.
- Treat local UI state as part of the client state model. Keep client-only UI
  changes separate from server-backed world and character state where their
  consistency rules differ.
- Separate the initial memory snapshot from subsequent event-driven updates.
- On every daemon connection, send a fresh complete snapshot followed by later
  updates. Make the boundary and ordering explicit so no update can be lost
  between the snapshot and stream.
- Represent partial, unavailable, and version-unknown state instead of
  inventing defaults.
- Keep raw addresses and version-specific layouts inside the injected process.
- Do not add an unbounded event backlog merely to cover daemon downtime. A
  reconnect restores current state from a new snapshot and resumes real-time
  events from its ordered boundary.
- Make state transitions deterministic and test them using captured synthetic
  fixtures that contain no proprietary game assets or personal data.

## Daemon and web API

- Make `darpcd.exe` the owner of discovery. `darpc.dll` must expose its deterministic
  PID-based pipe and remain ready to accept a replacement daemon connection.
- Reconcile clients at daemon startup and periodically thereafter by mapping
  supported game window classes to process identifiers and probing their
  expected pipe names.
- Distinguish a missing pipe from a busy pipe. Retry a busy endpoint, and allow
  a short initialization grace period before presenting a missing endpoint as
  an injection candidate.
- Treat a matching window as an injection candidate, not as proof of client
  compatibility. Require loader validation before injection and a daRPC
  handshake before accepting an RPC endpoint.
- Do not depend on custom Windows messages for discovery. They may only serve
  as a future wake-up optimization because notifications can be missed and do
  not discover uninjected processes.
- Keep externally visible API models separate from internal client layouts and
  wire protocol models.
- Validate request sizes and values at the HTTP and WebSocket boundaries.
- Use explicit API versioning before compatibility constraints exist in the
  wild.
- Define SSE and WebSocket ordering, replay, lag, and disconnect behavior.
- Default network listeners to the least exposed practical interface. Any
  remote-access mode must document authentication, authorization, and transport
  security expectations.
- Do not let one connected client or API consumer starve others.

## Testing

Prefer tests that do not require a live game client:

- Unit-test codecs, pointer and layout validation, state transitions, and
  error paths.
- Use round-trip and malformed-input tests for every protocol message.
- Use property or fuzz testing for parsers and binary framing when practical.
- Keep Windows integration tests isolated and clearly mark requirements such
  as target architecture or elevated privileges.
- Never commit copyrighted client binaries, game assets, secrets, personal
  data, or live-server captures containing private information.

If a live-client test is unavoidable, document a reproducible manual procedure
without redistributing proprietary material.

## Documentation book

The README is a concise entry point for developers. Keep detailed architecture,
state, protocol, discovery, security, and API material in the mdBook under
`docs/src/`.

- Keep `docs/src/SUMMARY.md` synchronized with added, removed, or renamed book
  chapters.
- Use the mdBook version pinned in the documentation workflow and README.
- Validate book changes with `mdbook build docs`.
- Do not edit or commit generated files under `docs/book/`.
- Update the README only when the project summary, primary components,
  getting-started path, documentation URL, license, or legal disclaimer changes.
- Update the relevant book chapters when architecture or public interfaces
  change.
- Distinguish implemented behavior from planned design throughout the book.

## Documentation and style

- Write clear American English for user-facing and contributor documentation.
- Do not use emoji or em dashes in code, comments, commit messages, or
  documentation.
- Explain acronyms on first use unless they are conventional Rust or Windows
  terms in the immediate context.
- Update the appropriate README summary and mdBook chapters when supported
  targets or setup change.
- Distinguish implemented behavior from plans and examples.

## Git history

Use Conventional Commits:

```text
<type>(optional-scope): <imperative summary>
```

Common types include `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci`,
`perf`, and `chore`. Use `!` and a `BREAKING CHANGE:` footer for breaking
changes. Keep commits cohesive, and do not mix unrelated refactors with
behavioral changes.

Examples:

```text
feat(protocol): add handshake negotiation
fix(loader): validate target process architecture
test(state): cover incomplete initial snapshots
```

## Legal and ethical boundaries

- Do not add proprietary game code, binaries, assets, credentials, or server
  implementation details obtained without authorization.
- Do not present the project as affiliated with or endorsed by Nexon or KRU
  Interactive.
- Keep work directed toward education, research, interoperability, and
  user-controlled automation.
- Document security-sensitive behavior and avoid features whose primary purpose
  is credential theft, unauthorized access, or harm to other users or systems.
- Preserve the README legal disclaimer when making documentation changes.
