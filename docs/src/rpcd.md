# `rpcd.exe`

`rpcd.exe` is a 64-bit x86-64 Windows daemon that makes injected clients easy
to use from local and remote applications.

Its responsibilities are to:

- Discover game client processes and their deterministic daRPC pipe endpoints.
- Identify uninjected processes that may be candidates for `loader.exe`.
- Connect and reconnect to available `rpc.dll` instances.
- Query and aggregate the state maintained by each injected client.
- Listen for real-time state changes and client events.
- Route valid actions from API consumers to the intended client.
- Expose REST, Server-Sent Events, and WebSocket interfaces.

The daemon is not the authority for client memory or local state. After every
connection, it obtains a fresh snapshot from `rpc.dll` and then follows later
updates from an ordered stream boundary.

## Failure isolation

A daemon restart must not end a game session. Each `rpc.dll` continues tracking
local state while disconnected. When `rpcd.exe` returns, daemon-driven
discovery finds the pipe again and the new snapshot restores the daemon's
current view.

Connections and queues must be bounded so one slow API consumer or game client
cannot starve the others.
