# Groups

The group resource shows who is adventuring together, who leads the group, and
which invitations are waiting for an answer. daRPC follows the same group rules
as the game client and lets the server confirm every roster or setting change.

| Use | Route or events |
| --- | --- |
| Read current group state | `GET /clients/{client}/group` |
| Open or close grouping, or leave a group | `POST /clients/{client}/group/toggle` |
| Invite a visible player | `POST /clients/{client}/group/invite` |
| Answer an invitation | `POST /clients/{client}/group/invitations/{id}/accept` or `/decline` |
| Watch changes | `group.*` events on `/clients/{client}/events` |

## Read the current group

```console
curl "http://127.0.0.1:2626/clients/ZiLo/group"
```

```text
GroupSnapshot {
    observation: ObservationMetadata,
    group: GroupState?,
}

GroupState {
    members: Vec<GroupMember>,
    invitations: Vec<GroupInvitation>,
    is_group_open: bool?,
    auto_accept: bool?,
}

GroupMember {
    name: string,
    is_leader: bool,
}

GroupInvitation {
    id: u32,
    inviter: string,
    received_tick_ms: u32?,
}
```

An empty `members` array means the character is adventuring alone. The leader
is the member with `is_leader: true`. `group` is `null` outside a usable game
world.

`is_group_open` is the last setting confirmed by the server. It can be absent
until daRPC observes the character's self-look data. `auto_accept` reports the
client option when daRPC can infer it from an incoming invitation. A pending
invitation found during late attach may not have `received_tick_ms` because its
original packet arrived before daRPC was loaded.

The invitation prompt is local client state rather than a complete server
snapshot. daRPC checks for it at most once every 100 milliseconds. Each check
walks the open prompt list once, which keeps invitation handling responsive
without doing repeated client-memory work on every rendered tick.

The `/status` response also includes `character.is_group_open` and
`character.group_members`. `group_members` is always an array and stays empty
when the character is not grouped.

## Toggle grouping

```console
curl --request POST "http://127.0.0.1:2626/clients/ZiLo/group/toggle"
```

The request has no body. When the character is adventuring alone, it toggles
whether other players may invite them. When the character is already grouped,
the same native action leaves the group or disbands it for the leader. The REST
command result only confirms that the client submitted the action. Read the
updated state or wait for a group event for the server-confirmed result.

## Invite a player

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"target":"OtherPlayer"}' \
  "http://127.0.0.1:2626/clients/ZiLo/group/invite"
```

`target` may be a case-insensitive visible player name or a visible player
object ID. The target must be on screen and cannot be the calling character.

`group.invitation_sent` means the local client submitted the request. The game
does not report a remote decline or acceptance directly. A closed group setting
may instead produce the usual system message that the player refuses to join.
Membership changes remain authoritative.

## Answer an invitation

Pending invitations have a daRPC invitation ID:

```console
curl --request POST \
  "http://127.0.0.1:2626/clients/ZiLo/group/invitations/7/accept"

curl --request POST \
  "http://127.0.0.1:2626/clients/ZiLo/group/invitations/7/decline"
```

These requests have no body. They require the existing game invitation prompt,
submit the answer on the client main thread, and close that prompt normally. An
unknown or already closed ID returns `404`.
`group.invitation_closed` records the local answer. Acceptance is confirmed
later by `group.joined` or another roster event.

The server messages `Group disbanded.` and `<name> is joining this group.` start
a 30-second self-look refresh window. Refresh requests use daRPC's internal
self-inspection path: the `0x39` response updates group and legend state but is
suppressed before the native handler can open the self-look interface. The
self-look roster is `Adventuring alone` while solo or a newline-delimited list
headed by `Group members` while grouped. The list ends with `Total n`, and a
leading `*` marks the leader. While grouped, daRPC refreshes the roster every
two seconds. This catches joins, departures, and disbands even when the game
does not send fresh self-look data to every member at the same moment.

## Live group events

Every state-bearing event contains the complete resulting `group`, so a
consumer can replace its retained group value without reconstructing hidden
client behavior.

| Event | Meaning |
| --- | --- |
| `group.settings_changed` | The server-confirmed group-open setting changed. |
| `group.invitation_sent` | The local client submitted an invitation request. |
| `group.invitation_received` | A group prompt appeared and can be answered by ID. |
| `group.invitation_closed` | A prompt was answered, dismissed, or invalidated. |
| `group.joined` | A solo character received a nonempty roster. |
| `group.member_joined` | A member appeared in an existing roster. |
| `group.member_left` | A member disappeared from an existing roster. |
| `group.disbanded` | The roster became empty. |

Invitation close reasons are `accept_requested`, `declined`, and `dismissed`.
See [Live events](events.md) for the common event envelope, ordering, and
resynchronization behavior.
