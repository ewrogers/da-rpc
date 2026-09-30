# Changelog

## Unreleased

## 1.0.0

First release, establishing the complete current implementation as the baseline.

- Validated x86 DLL injection and client launch through `loader.exe`.
- Direct binary RPC through `darpc.exe`, with text and JSON output.
- Client discovery, state aggregation, REST, generated OpenAPI, and
  Server-Sent Events through `darpcd.exe`.
- Character, world-object, movement, inventory, equipment, ability, dialog,
  group, exchange, field-map, bulletin-board, and player-mail state and commands.
- Bounded hook diagnostics, protocol validation, and integration harnesses.

All workspace components use version 1.0.0 and negotiate binary protocol 1.0.
The frame envelope and DLL lifecycle ABI retain their independent version 1.
Development releases before this baseline are unsupported. Replace the DLL,
loader, direct client, and daemon together; early development builds that used
1.0 do not implement this complete schema.
