# `darpc.exe` command-line interface

The direct CLI talks to one injected DLL without going through the daemon. Use
it for diagnostics, simple one-client scripts, or protocol inspection. Use
[`darpcd.exe`](rpcd.md) when you need discovery, several clients, REST, or live
event streams.

> **Status:** The direct commands documented below are implemented.

`darpc.exe` is a direct, single-client command-line interface to an injected
`darpc.dll`. It connects to the process-specific named pipe, exchanges typed
binary protocol messages, and presents responses as human-readable text or
stable JSON.

The CLI does not call the `darpcd.exe` HTTP API, inject DLLs, or invoke
`loader.exe`. This keeps a useful standalone path for developers and automation
that need only `loader.exe`, `darpc.dll`, and `darpc.exe`.

The command-line boundaries are:

| Tool | Responsibility |
| --- | --- |
| `loader.exe` | Launch, inspect, attach, detach, and apply supported launch patches. |
| `darpc.exe` | Exchange typed protocol messages directly with one injected DLL. |
| `darpcd.exe` | Maintain multiple client connections and expose aggregate state through web APIs. |

## Command-line reference

Every direct command accepts the process selector `--pid <pid>`. Put the
optional global output selector before the command:

```text
darpc.exe [--output <table|json>] <command> [arguments]
```

| Flag | Meaning |
|---|---|
| `--output table` | Write the default human-readable output. |
| `--output json` | Write one stable JSON value to standard output for scripts. |
| `--pid <pid>` | Connect to the daRPC pipe owned by this game process. This command-level flag is required. |
| `--input <text>` | Supply text to a spell prompt. |
| `--target-id <id>` | Cast a spell at the specified numeric object identifier. |
| `--target <x> <y>` | Cast a spell at the specified map coordinates. |

For spell casting, `--target-id` and `--target` are mutually exclusive. Omit
both for a spell that does not require an explicit target. Item names supplied
to sell, deposit, withdraw, and repair commands are case-sensitive and must
preserve their punctuation and spacing exactly.

### Direct IPC commands

The implemented operations prove communication, expose hook health, read a
current client snapshot, and submit movement through the client:

```text
darpc hello --pid <pid>
darpc ping --pid <pid>
darpc echo --pid <pid> "hello"
darpc tick health --pid <pid>
darpc snapshot --pid <pid>
darpc diagnostic --pid <pid>
darpc diagnostics hooks --pid <pid>
darpc diagnostics enable --pid <pid>
darpc diagnostics disable --pid <pid>
darpc diagnostics reset --pid <pid>
darpc raw send --pid <pid> <client|server> <NN|0xNN> [hex-payload]
darpc assail --pid <pid>
darpc stat <strength|dexterity|intelligence|wisdom|constitution> --pid <pid>
darpc turn --pid <pid> <north|east|south|west>
darpc walk --pid <pid> <north|east|south|west>
darpc walk --pid <pid> <x> <y>
darpc walk --pid <pid> cancel
darpc skill use --pid <pid> <slot>
darpc skill swap --pid <pid> <source> <destination>
darpc spell cast --pid <pid> <slot>
darpc spell cast --pid <pid> <slot> --target-id <object-id>
darpc spell cast --pid <pid> <slot> --target <x> <y>
darpc spell cast --pid <pid> <slot> --input <text>
darpc spell swap --pid <pid> <source> <destination>
darpc item use --pid <pid> <slot>
darpc item drop --pid <pid> <slot> <x> <y> [quantity]
darpc item give --pid <pid> <slot> <object-id> [quantity]
darpc item swap --pid <pid> <source> <destination>
darpc gold drop --pid <pid> <amount> <x> <y>
darpc gold give --pid <pid> <amount> <object-id>
darpc item pickup --pid <pid> <x> <y>
darpc unequip --pid <pid> <slot-number>
darpc emote --pid <pid> <name|code>
darpc chant --pid <pid> <text>
darpc item sell --pid <pid> <item-name>
darpc item sell-all --pid <pid> <item-name>
darpc item deposit --pid <pid> <item-name>
darpc item withdraw --pid <pid> <item-name>
darpc item repair --pid <pid> <item-name>
darpc item repair-all --pid <pid>
darpc interact --pid <pid> <object-id>
darpc dialog select --pid <pid> <revision> <index> [quantity]
darpc dialog input --pid <pid> <revision> <text>
darpc dialog previous --pid <pid> <revision>
darpc dialog next --pid <pid> <revision>
darpc dialog close --pid <pid> <revision>
darpc field-map select --pid <pid> <revision> <destination-index>
darpc bulletin open --pid <pid>
darpc bulletin world --pid <pid> <x> <y>
darpc bulletin open-section --pid <pid> <revision> <section-id>
darpc bulletin select-section --pid <pid> <revision> <section-id>
darpc bulletin open-entry --pid <pid> <revision> <entry-id>
darpc bulletin select-entry --pid <pid> <revision> <entry-id>
darpc bulletin older --pid <pid> <revision>
darpc bulletin scroll --pid <pid> <revision> <position>
darpc bulletin back --pid <pid> <revision>
darpc bulletin forward --pid <pid> <revision>
darpc bulletin previous --pid <pid> <revision>
darpc bulletin next --pid <pid> <revision>
darpc bulletin compose-post --pid <pid> <revision>
darpc bulletin compose-mail --pid <pid> <revision>
darpc bulletin reply --pid <pid> <revision>
darpc bulletin update-post --pid <pid> <revision> <subject> <body>
darpc bulletin update-mail --pid <pid> <revision> <recipient> <subject> <body>
darpc bulletin submit --pid <pid> <revision>
darpc bulletin delete --pid <pid> <revision> <entry-id>
darpc bulletin highlight --pid <pid> <revision> <entry-id>
darpc bulletin close --pid <pid> <revision>
darpc message-dialog dismiss --pid <pid> <revision> <id>
darpc group toggle --pid <pid>
darpc group invite --pid <pid> <player>
darpc group accept --pid <pid> <invitation-id>
darpc group decline --pid <pid> <invitation-id>
darpc exchange item --pid <pid> <slot> [quantity]
darpc exchange gold --pid <pid> <amount>
darpc exchange accept --pid <pid>
darpc exchange cancel --pid <pid>
darpc who --pid <pid>
darpc legend --pid <pid>
darpc inspect --pid <pid> <object-id>
darpc command status --pid <pid> <command-id>
darpc command cancel --pid <pid> <command-id>
```

For raw packets, quote a nonempty space-separated payload, for example `darpc
raw send --pid 3780 client 7E "00 03 02"`. The command byte accepts two
hexadecimal digits with an optional `0x` prefix. The payload accepts at most 255
payload bytes. See [Raw packets](raw.md) before using this low-level interface;
malformed packets can disconnect sessions or crash the game client or server.

`chant` sends its text through the client's spell-chant channel. The item
convenience commands build the NPC phrases documented in [Inventory](inventory.md).
Item names are case-sensitive and are preserved verbatim, including punctuation,
repeated spaces, and leading or trailing spaces. Quote names at the shell so the
entire name reaches `darpc.exe` as one argument.

Related operations use a domain and subcommand. Examples include `skill use`,
`item swap`, and `dialog select`. These commands use the real PID-based named
pipe, binary framing, protocol negotiation, request correlation, sequencing,
and connection lifecycle. Their behavior is:

- `hello` reports compatible DLL and process metadata.
- `ping` verifies a complete request and response round trip and reports its
  elapsed time.
- `echo` returns its UTF-8 payload byte-for-byte, with a 4 KiB input limit.
- `tick health` samples the client tick counter twice, 250 milliseconds apart,
  and reports installation metadata, both counter values, their wrapping
  difference, and whether the counter advanced.
- `snapshot` schedules a bounded capture on the client main thread and reports
  lifecycle, character, map, inventory, equipment, spellbook, skillbook,
  active spell-effect, dialog, field-map, bulletin, group roster, invitation,
  and complete native planned-route state plus event, capture timing, and
  request round-trip metadata.
- `diagnostic` submits a no-op command to the bounded main-thread queue, waits
  up to one second, and reports its state, queue delay, execution duration, and
  client main-thread ID.
- `diagnostics hooks` queries runtime hook timing. `diagnostics enable` and
  `diagnostics disable` change the mode without reinjection. `diagnostics reset`
  clears counters without changing the current mode. Each stage reports its
  budget, calls, total, average, maximum, last duration, and over-budget count
  in microseconds.
- `assail` submits the client's native `0x13` basic-attack packet. The resulting
  client observations can emit `player.animated` and `sound.played` events.
- `stat` spends one available stat point by sending native packet `0x47` with
  the selected strength, dexterity, intelligence, wisdom, or constitution flag.
  The corresponding short aliases are `str`, `dex`, `int`, `wis`, and `con`.
- `turn` cancels any queued native route and asks the client to face one of the
  four cardinal directions.
- `walk` with a direction cancels any queued route and attempts one native,
  collision-checked step. `walk` with x/y asks the client's native pathfinder to
  follow a route to that zero-based map tile.
- `skill use` invokes a learned one-based skill slot through the client's native
  activation routine. It does not select the skill panel, change focus, or
  synthesize keyboard or mouse input. `skill swap` exchanges two one-based
  skillbook slots.
- `spell cast` invokes a learned one-based spell slot through the matching
  native client routine. Its optional argument is one visible object ID, one
  zero-based map tile, or 1 through 100 ASCII bytes. The DLL checks that the
  selected spell expects that argument shape. A targeted spell defaults to the
  casting character when no target is supplied. A new cast may replace a
  delayed cast already in progress. `spell swap` exchanges two one-based
  spellbook slots.
- `item use` activates a live one-based inventory slot through the client's
  ordinary item path.
- `item drop` and `item give` submit a validated quantity from a live slot.
  Quantity defaults to 1. Giving begins the game's ordinary exchange flow.
- `item swap` exchanges two one-based inventory slots.
- `gold drop` and `gold give` submit a nonzero amount to a tile or object ID.
- `item pickup` asks the server for the top ground item at a zero-based tile
  and uses the first empty inventory slot available at execution time.
- `unequip` accepts the client's one-based equipment slot number from 1 through
  18. `emote` accepts a confirmed case-insensitive name such as `wave`, or a
  normal client UI emote code. See [Emotes](emotes.md) for
  the named list.
- `interact` starts a conversation with one visible Mundane object ID.
  `dialog select` submits a zero-based displayed row and optional nonzero
  quantity. `dialog input` submits nonempty ASCII text. Dialog selection,
  input, navigation, and close commands require the current dialog revision so
  stale actions fail closed in the DLL.
- `field-map select` submits one zero-based destination from the active field
  map. It requires the current field-map revision and uses the retained
  checksum and travel coordinates. See [Field maps](field-maps.md).
- `bulletin` commands open global or world-tile boards; select, open, page, and
  scroll lists and entries; navigate native dialog history; compose board
  articles or player mail; and submit deletion or highlight requests. Every
  command except `open` and `world` requires the revision reported by the
  active bulletin state. See [Bulletin boards and player mail](bulletins.md).
- `message-dialog dismiss` closes one active native message dialog by the
  revision and opaque ID returned by current state. See
  [Message dialogs](message-dialogs.md).
- `group toggle` uses the native client toggle. It opens or closes invitations
  while solo and leaves or disbands an active group. `group invite` sends a
  validated ASCII player name. `group accept` and `group decline` answer one
  retained invitation ID. Direct `snapshot` exposes retained group state, but
  the daemon API adds visible-name resolution, REST resources, and live events.
- `exchange item` adds a live inventory slot to an already open player
  exchange. Quantity defaults to 1 and is limited to 255. `exchange gold` sets
  one nonzero amount no greater than the current character gold. `exchange
  accept` and `exchange cancel` wait for the server to finish or close the
  ordinary exchange window.
- `who` requests the server-ordered online-player list, waits up to three
  seconds, and suppresses only its own client panel. Requests within one second
  share an in-flight or recently completed result.
- `command status` reads a retained command result by its nonzero ID.
- `command cancel` atomically cancels a command that is still accepted. A
  command that already started retains its completed state.
- `legend` requests a fresh SelfLook from the server and prints every legend
  mark with its text, tag, color, and friendly icon name. Requests share the
  same one-second coalescing window as the REST endpoint.
- `inspect` refreshes one visible player's profile by object ID, waits up to
  three seconds, and suppresses only its correlated other-player information
  pane. It returns identity, group-open state, equipment, and legend metadata.

The commands share `darpc-protocol` with the DLL and daemon. Each requires an
explicit nonzero process ID and cannot manage multiple clients in one command.

## Output

Human-readable output is the default. Put `--output json` before the command to
emit one stable JSON value on standard output:

```text
darpc --output json hello --pid <pid>
darpc --output json ping --pid <pid>
darpc --output json echo --pid <pid> "hello"
darpc --output json tick health --pid <pid>
darpc --output json snapshot --pid <pid>
darpc --output json diagnostic --pid <pid>
darpc --output json turn --pid <pid> north
darpc --output json walk --pid <pid> 120 85
darpc --output json skill use --pid <pid> 5
darpc --output json skill swap --pid <pid> 5 6
darpc --output json spell cast --pid <pid> 7 --input "nothing"
darpc --output json item swap --pid <pid> 1 2
darpc --output json dialog select --pid <pid> 7 0
darpc --output json field-map select --pid <pid> 11 1
darpc --output json bulletin open-entry --pid <pid> 15 4280
darpc --output json group invite --pid <pid> ZiLo
darpc --output json who --pid <pid>
darpc --output json legend --pid <pid>
darpc --output json inspect --pid <pid> <object-id>
darpc --output json command status --pid <pid> <command-id>
```

Diagnostics belong on standard error so scripts can parse JSON from standard
output without filtering it. Exit codes distinguish invalid input, missing or
busy endpoints, protocol incompatibility, malformed responses, and other I/O
failures.

## Connection ownership

The DLL pipe currently accepts one controller at a time. `darpc.exe` and
`darpcd.exe` are alternative consumers of that pipe, not layers in the same
request path. A direct CLI command reports the endpoint as busy when the daemon
owns the connection. It does not fall back to the daemon or disconnect it.

## Future commands

New CLI commands should be added only when the DLL exposes the matching typed
protocol operation. Each command should:

- Target exactly one explicit PID.
- Validate arguments before opening the pipe.
- Use typed protocol messages rather than arbitrary byte or command strings.
- Preserve equivalent human-readable and stable JSON representations.
- Remain usable without `darpcd.exe`.

Additional game-state reads and actions can extend the existing `ipc` hierarchy
as their protocol messages become real. The CLI should not grow daemon discovery,
aggregation, web configuration, or multi-client policy.

## Daemon access

Consumers that need aggregated multi-client state use the `darpcd.exe` HTTP
API directly. The daemon publishes an OpenAPI document at `/openapi.json` and
an interactive Swagger UI at `/docs`, so another command-line HTTP wrapper is
not part of the planned architecture.
