# daRPC context

## Language

**Client observation**:
An identity-scoped view of one injected client at a specific revision and event
sequence. A complete snapshot establishes it, and contiguous state events
advance it.
_Avoid_: registry cache, cached state

**Observation commit**:
The single validated transition that updates the daemon's retained client
observation and supplies the matching changes for publication to consumers.
_Avoid_: event replay, state publication pass
