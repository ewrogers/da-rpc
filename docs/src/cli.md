# `darpc.exe` command-line interface

> **Status:** This command-line interface is a tentative design. No `darpc.exe`
> implementation exists yet. Command names, output fields, and exit codes may
> change as the binary IPC, HTTP API, and first real actions are implemented.

`darpc.exe` is the user-facing command-line client for daRPC. Normal commands
query and control clients through the public `darpcd.exe` HTTP API. The explicit
`ipc` command group connects directly to one `darpc.dll` named pipe for early
development and diagnostics. The CLI never injects DLLs or calls `loader.exe`.

Keeping normal commands behind the daemon gives discovery, client selection,
action status, security, and multi-client aggregation one owner. Direct IPC is
an intentionally narrow diagnostic path, not a second production controller.

## Direct IPC diagnostics

The first `darpc.exe` increment should prove communication before hooks or game
state exist:

```text
darpc ipc hello --pid <pid>
darpc ipc ping --pid <pid>
darpc ipc echo --pid <pid> "hello"
```

These commands use the real PID-based named pipe, binary framing, protocol
negotiation, request correlation, and reconnect behavior. Only their payloads
are synthetic:

- `hello` reports compatible DLL and process metadata.
- `ping` verifies a complete request and response round trip.
- `echo` returns its UTF-8 payload byte-for-byte, with a 4 KiB input limit.

The `ipc` group shares `darpc-protocol` with the DLL and daemon. It cannot read
game state, execute game actions, invoke the loader, or manage multiple clients.
It requires an explicit process ID and, while the pipe supports one controller,
requires `darpcd.exe` to be disconnected.

## Command shape

Commands follow one predictable grammar:

```text
darpc [global options] <domain> <action> [subject] [options]
```

Examples:

```text
darpc daemon health
darpc clients list
darpc --client name:SiLo character show
darpc --client name:SiLo spells list
darpc --client name:SiLo spells cast "Example Spell" --target self
darpc --client id:01J00000000000000000000000 inventory use --slot 4
```

The conventions are:

- Use plural nouns for collections and command groups, such as `clients`,
  `spells`, `skills`, and `actions`.
- Use short lowercase verbs, such as `list`, `show`, `cast`, `use`, and `move`.
- Put the primary subject in a positional argument when it is unambiguous.
- Use named options for modifiers, targets, timeouts, and alternate selectors.
- Use kebab-case for multiword command and option names.
- Give every subcommand its own help, examples, validation, and completion
  candidates where practical.

The CLI should not accept a generic command string such as:

```text
darpc -x "castSpell"
darpc --command "castSpell Example Spell"
```

String commands require a second parsing layer and lose structured help, shell
completion, typed validation, and clear API authorization. Each supported CLI
operation should map to one typed daemon query or action.

## Global options

The tentative global options are:

| Option | Short | Purpose |
| --- | --- | --- |
| `--client <selector>` | `-c` | Select one managed game client for a client-scoped command. |
| `--output <format>` | `-o` | Select `table`, `json`, or `jsonl` output. |
| `--daemon <url>` | | Override the local daemon URL. |
| `--wait` | | Wait for an asynchronous action to reach a terminal state. |
| `--timeout <duration>` | | Bound connection or action waiting time. |
| `--quiet` | `-q` | Suppress nonessential human-readable progress. |

Global options should be accepted before or after the domain when the parser
can do so without ambiguity:

```text
darpc -c name:SiLo spells list
darpc spells list --client name:SiLo
```

The initial implementation should avoid a persistent default client. Requiring
an explicit selector for client-scoped commands is predictable and prevents a
script from acting on a different client merely because connection order
changed. A persistent default can be considered later if real use demonstrates
the need.

## Client selectors

A selector identifies a daemon-managed client, not merely a character name.
The explicit forms are:

```text
id:<client-id>
name:<character-name>
pid:<process-id>
```

Examples:

```text
darpc -c id:01J00000000000000000000000 character show
darpc -c name:SiLo character show
darpc -c pid:1234 character show
```

The stable daemon client ID is authoritative. Character name and process ID are
conveniences whose meaning can change across reconnects, logins, or process
restarts.

A plain selector may be accepted as shorthand when it matches exactly one
client:

```text
darpc -c SiLo character show
```

An ambiguous selector must fail and list the matching client IDs. A mutating
command must never silently choose a client.

## Tentative command groups

The hierarchy should grow only when the corresponding daemon behavior exists.
The following groups describe the intended organization, not current support.

### Daemon and clients

```text
darpc daemon health
darpc daemon version

darpc clients list
darpc clients show <selector>
darpc clients candidates
darpc clients attach <selector>
darpc clients launch [configured-client]
```

Client launch and injection commands go to `darpcd.exe`. The daemon applies
policy and invokes the separate 32-bit `loader.exe`. The CLI does not accept an
arbitrary DLL path.

### Character and state

```text
darpc -c <client> character show
darpc -c <client> character status
darpc -c <client> equipment list
darpc -c <client> inventory list
darpc -c <client> spells list
darpc -c <client> skills list
darpc -c <client> world show
darpc -c <client> world entities
```

Read commands display the daemon's current replicated state. They do not cause
an ad hoc client-memory read for every invocation.

### Typed actions

```text
darpc -c <client> spells cast <spell> [target options]
darpc -c <client> skills use <skill> [target options]
darpc -c <client> inventory use --slot <slot>
darpc -c <client> inventory drop --slot <slot> [--quantity <count>]
darpc -c <client> equipment remove --slot <slot>
darpc -c <client> world move <direction>
darpc -c <client> world turn <direction>
darpc -c <client> chat say <message>
darpc -c <client> chat tell <name> <message>
```

Stable identifiers or slots are preferred where the client exposes them. Names
are convenient but must fail when they are ambiguous.

Tentative target forms include:

```text
--target self
--target entity:<entity-id>
--target tile:<x>,<y>
```

The exact target kinds belong to each typed action. A command should reject an
unsupported target instead of forwarding an unchecked string to the DLL.

### Action status

Mutating commands are asynchronous because IPC work is queued and later
executed on the game thread. The initial response should include an action ID
and distinguish at least:

```text
queued
delivered
executed
failed
cancelled
timed-out
observed
```

`executed` means the client-thread operation ran. It does not necessarily mean
the game server accepted the action. `observed` may be used when a later client
or server event confirms an expected outcome.

Action commands return after submission unless `--wait` is present:

```text
darpc -c name:SiLo spells cast --slot 4 --target self
darpc -c name:SiLo spells cast --slot 4 --target self --wait
darpc -c name:SiLo actions show <action-id>
darpc -c name:SiLo actions cancel <action-id>
```

A local wait timeout does not imply that the underlying action was cancelled.
The CLI should print the action ID so the caller can inspect it later.

### Events and rules

```text
darpc -c <client> events watch
darpc -c <client> rules list
darpc -c <client> rules show <rule-id>
darpc -c <client> rules apply <file>
darpc -c <client> rules delete <rule-id>
```

Event watching should use Server-Sent Events initially. Rule commands operate
through the daemon, which validates and publishes a complete immutable rule
snapshot to the DLL. A CLI command never waits inside a hook for authorization.

Raw packet submission is not part of the initial general-purpose hierarchy. If
it is exposed later, it should live under a clearly restricted diagnostic
namespace rather than masquerading as an ordinary typed action.

## Output formats

Human-readable output is the interactive default:

```text
darpc clients list
darpc -c name:SiLo inventory list
```

Automation requests a structured format explicitly:

```text
darpc clients list --output json
darpc -c name:SiLo character show --output json
darpc -c name:SiLo events watch --output jsonl
```

Output rules are:

- `table` is concise human-readable output for finite commands.
- `json` writes one complete JSON document for a finite command.
- `jsonl` writes one JSON value per line for streams.
- Structured data goes to standard output.
- Diagnostics and progress go to standard error.
- JSON fields use stable snake_case names.
- A command must not mix commentary into structured standard output.

The CLI output model may differ from both internal wire types and raw HTTP
transport details. Compatibility should be deliberate once scripts depend on
it.

## Exit behavior

Exact numeric values remain tentative, but exit categories should distinguish:

| Category | Meaning |
| --- | --- |
| Success | The query completed or the action was accepted as requested. |
| Usage | Command syntax, option, or local validation failed. |
| Service unavailable | The daemon or direct IPC endpoint could not be reached or negotiated. |
| Selection failed | The requested client was missing or ambiguous. |
| Action rejected | The daemon or DLL rejected a typed action. |
| Wait timed out | The local wait expired, although the action may still complete. |
| Internal failure | An unexpected CLI or daemon error occurred. |

Shell scripts should be able to rely on both the exit category and structured
error output without parsing human prose.

## Initial implementation slices

The first slice follows the loader and exercises only direct IPC:

```text
darpc ipc hello --pid <pid>
darpc ipc ping --pid <pid>
darpc ipc echo --pid <pid> "hello"
```

It should support human-readable and `json` output, deterministic exit behavior,
and clean reconnection. It has no dependency on hooks, client memory, or state.

After the daemon exposes HTTP, the same executable gains its first normal
commands:

```text
darpc daemon health
darpc daemon version
darpc clients list
darpc clients show <selector>
```

These commands add `table` output and a daemon URL override. Client-scoped state
groups and actions should be added only when their matching daemon endpoints and
lifecycle tests exist. The explicit `ipc` group remains available as a
developer diagnostic when the daemon is disconnected.

## Open decisions

The following details should remain tentative until the first implementation:

- Whether global options can appear at every command depth without confusing
  help output.
- Exact JSON schemas and compatibility guarantees.
- Exact numeric exit codes.
- Whether a configured default client becomes useful after real multi-client
  use.
- Confirmation policy for destructive or unusually broad actions.
- Authentication and configuration behavior when the daemon permits remote
  access.
