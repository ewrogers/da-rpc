# Introduction

daRPC, short for Dark Ages Remote Procedure Call, lets other programs observe
and control a running *Dark Ages* client.

It can read the character and map state the client already knows, follow live
changes, and ask the client to perform actions such as walking, using a skill,
or casting a spell. One daemon can bring several clients together behind a
local web API.

The project is in active development and supports one exact 7.41 client build.
It is intended for education, research, interoperability, and user-controlled
automation.

## Why work inside the client?

Many Dark Ages tools watch the game through a network proxy. A proxy is very
useful for studying packets, but it sees only what travels between the game and
the server. It may need to rebuild state from packet history, and it cannot
always see client-only details such as an open dialog, an active path, or a
local action in progress.

daRPC attaches a small Dynamic Link Library (DLL) to the game client instead.
This gives it a direct view of the state the client is using right now. It can
also call the same native client methods used by the game interface, rather
than creating network packets from scratch.

That matters for actions. The client still performs its usual checks, updates
its interface, and keeps its normal timing. Native pathfinding can build a
route, and ordinary player input can interrupt that route without fighting a
separate movement loop. An external planner can install an exact native route,
while per-map policy exclusions keep the built-in pathfinder from choosing
application-defined tiles to avoid.

The DLL can attach to an already running client and detach without closing it.
A packet analyzer can still run alongside daRPC when both views are useful.

## Ways to use daRPC

`loader.exe` launches a client or attaches and detaches the DLL.

`darpc.exe` talks directly to one injected client. It is useful for quick
checks, scripts, and setups that do not need a central daemon.

`darpcd.exe` discovers several clients and exposes them through:

- REST for current state and actions
- Server-Sent Events (SSE) for live changes
- OpenAPI and Swagger UI for exploring the API

REST and SSE are the complete supported web transport model: REST handles
bounded commands and state reads, and SSE streams live changes. WebSockets are
intentionally unsupported because they would add a second command and
connection lifecycle without a demonstrated need.

## Where to begin

Choose the path that matches what you are building:

| Goal | Start here |
| --- | --- |
| Read client state or submit actions | [Web API](web-api.md) |
| React to changes as they happen | [Live events](events.md) |
| Understand character and world fields | [Game data](state.md) |
| Compare the three command-line programs | [Executable components](executables.md) |
| Work directly with one DLL without the daemon | [`darpc.exe`](cli.md) |
| Launch, attach, or automatically load clients | [`darpcd.exe`](rpcd.md) and [`loader.exe`](loader.md) |
| Learn how the pieces fit together | [How it works](architecture.md) |
| Understand hooks and main-thread safety | [Runtime hooks](hooks.md) |

The Swagger UI at `http://127.0.0.1:2626/docs` is also a useful place to
explore REST routes and try requests against a running daemon.
