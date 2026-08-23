# Refresh and resynchronization

Use refresh when the game client looks out of step with the server. Pressing F5
in the game and calling `POST /clients/{client}/resync` use the same daRPC path.
The game still sends its normal opcode-only `0x38` refresh packet.

## One refresh at a time

daRPC keeps at most one user refresh active for each client. More F5 presses
and more HTTP requests join the active refresh instead of sending another
packet. This matters because the game server can combine refresh packets that
arrive close together into one redraw.

An HTTP request that joins active work returns the active `resync_id` and sets
`coalesced` to `true`:

```text
{
    pid: u32,
    instance_id: string,
    resync_id: u32,
    coalesced: bool,
    resync: {
        phase: idle | waiting_to_send | awaiting_response,
        active_resync_id: u32?,
        pending_count: u32,
    },
}
```

`pending_count` is always zero in 1.7.0 because daRPC does not queue a second
refresh. `waiting_to_send` covers movement settling and packet submission.
`awaiting_response` begins after daRPC observes the outgoing `0x38` packet.
`client.resync` always carries a nonzero `resync_id`. HTTP refreshes use the
returned ID, while an in-game F5 receives a DLL-local ID.

Inside the DLL, one refresh transaction owns movement gating, packet
submission, object reconciliation, completion and fallback ordering, and
deferred snapshot recovery. Packet hooks report observations to that module;
they do not advance individual parts of the transaction themselves.

The usual response is `200 OK`. A request can return `202 Accepted` while work
is still active or when it joins an active refresh. A full general command
queue returns `command_queue_full`. A narrow race where the DLL has already
seen an in-game F5 but the daemon has not seen its outgoing event can return
`409 resync_busy`. Retry after the active refresh completes.

## Movement safety

Refreshing during the walking animation can make the native committed tile and
the tile being animated disagree. That is the source of the common one-tile
position error.

Before a user refresh, daRPC clears queued route movement but lets an accepted
step finish. It waits until the staged destination becomes the committed native
position, then sends `0x38`. A corrected committed position must remain stable
for one more client tick. This uses the client's real transition state, not a
fixed animation sleep.

The HTTP response can arrive before this safe point. `client.resync` is the
signal that the packet was actually sent. Consumers that pause movement should
release it only after the matching `client.resync_completed` event.

## Refresh window

The server normally redraws the user and nearby objects, then may send the
payload-free `0x22` `RefreshUserOK` packet. The stock 7.41 client does not use
`0x22`, and the packet is not guaranteed to arrive. daRPC therefore treats it
as an early end marker, not as the only proof of completion.

The refresh window closes in either of these ways:

1. daRPC observes authoritative position or redraw activity followed by
   `RefreshUserOK`.
2. One second passes after the outgoing `0x38` packet.

Both paths publish `client.resync_completed`. There is no
`client.resync_timed_out` event. Completion means daRPC has closed the refresh
and object-reconciliation window. It does not promise that `RefreshUserOK` was
received. The DLL's unload diagnostics count one-second fallback completions
as `user_refresh_fallbacks`.

## Object reconciliation

Consumers should keep their object state and apply normal lifecycle events.
They do not need to clear the world or diff a second snapshot after F5.

At packet submission, daRPC marks the currently visible stable entity IDs. As
redraw packets arrive:

- An existing ID stays in place and receives its normal change events.
- A new ID publishes its normal appeared event.
- A prior ID that does not return publishes its normal disappeared event when
  the refresh window closes.

If no authoritative position or redraw packet arrives, daRPC preserves the
last-known objects instead of guessing that they disappeared. Map changes use
the normal map and object lifecycle events and remain a separate reason for the
visible set to change.

Object appeared and disappeared events are ordered before
`client.resync_completed`. There is no `objects.cleared` event and no separate
object-reconciliation completion event. daRPC may recapture its internal
snapshot after that ordered boundary to refresh client-only appearance state.
This does not require a consumer snapshot reload or world clear.

## Consumer sequence

1. Open the Server-Sent Events stream.
2. Pause movement and call `POST /clients/{client}/resync`.
3. Remember the returned `resync_id`, including when `coalesced` is `true`.
4. Apply object and location events in stream order.
5. Resume movement after `client.resync_completed` with the matching ID.

If the stream disconnects before completion, reconnect, read current state,
and request another refresh. Do not wait forever for an event from the old
stream.
