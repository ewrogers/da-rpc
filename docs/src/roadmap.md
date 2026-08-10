# Roadmap

The original milestone roadmap guided daRPC from an empty workspace to its
first stable release. That sequence is complete and has been retired. This page
now records work that may follow 1.0 without implying that it is required for
the supported 1.0 feature set.

The [Web API](web-api.md), [Live events](events.md), and [Game data](state.md)
chapters describe current behavior. The [architecture](architecture.md),
[protocol](protocol.md), and [safety requirements](safety.md) remain the design
sources of truth.

## 1.0 foundation

Version 1.0 supports the exact Dark Ages 7.41 client build documented in the
README. Its stable foundation includes:

- validated x86 injection, launch, initialization, shutdown, and unload;
- direct typed commands over the versioned named-pipe protocol;
- multi-client discovery and aggregation through the local daemon;
- current state through REST and ordered changes through Server-Sent Events;
- bounded main-thread actions for movement, abilities, inventory, dialogs,
  groups, exchanges, communication, Who, and legend data;
- generated OpenAPI, vendored Swagger UI, and a versioned Windows binary
  release with SHA-256 checksums; and
- native Windows integration checks for the loader, hooks, protocol, daemon,
  and supported process lifecycle.

These capabilities define the 1.0 release. Later roadmap items extend or
harden them and do not change what 1.0 claims to support.

## Post-1.0 hardening

Potential hardening work includes:

- longer multi-client and daemon-restart soak tests;
- malformed-protocol corpus testing and parser fuzzing;
- dependency advisory and license checks in continuous integration;
- privacy-preserving crash diagnostics;
- a documented security-reporting process;
- signed Windows release binaries; and
- qualification of additional Dark Ages client builds without weakening exact
  executable validation.

## Post-1.0 capabilities

Potential capability work includes:

- immutable bounded local rules that always fail open;
- a derived shared-world view that preserves source, age, and uncertainty;
- additional direct CLI views where they improve automation;
- richer event replay only when a bounded consumer requirement is proven; and
- new typed actions built on confirmed client and game terminology.

## Remote access

The daemon remains loopback-only. Remote access is deferred until it has an
explicit authentication, authorization, request-limiting, and transport
security model. Exposing the current listener through a proxy or port forward
is outside the supported 1.0 configuration.

## Prioritization

Post-1.0 work should remain small and evidence-driven. A proposed feature
should identify the concrete user workflow, its safety boundary, and how it
will be tested before it expands the protocol or public API.
