# Online players

The Who list shows players currently reported online by the game server. daRPC
requests the list through one chosen client, so guildmate markers are from that
character's point of view.

## Read the list

```text
GET /clients/{client}/who
```

The response keeps the server's original player order:

```json
{
  "pid": 6076,
  "received_tick_ms": 14290500,
  "world_count": 128,
  "country_count": 42,
  "players": [
    {
      "name": "ZiLo",
      "title": "Aisling",
      "class": "priest",
      "state": "awake",
      "color": 3,
      "is_master": true,
      "is_guildmate": true
    }
  ]
}
```

`world_count` is the larger population count displayed by the client.
`country_count` is the number of player rows supplied for this character. The
`players` array can be shorter after applying filters.

Player states are `awake`, `do_not_disturb`, `daydreaming`, `need_group`,
`grouped`, `lone_hunter`, `group_hunting`, `need_help`, or `unknown`.

## Filter the response

Use a comma-separated, case-insensitive class filter:

```text
GET /clients/ZiLo/who?classes=warrior,rogue
```

Supported class names are `peasant`, `warrior`, `rogue`, `wizard`, `priest`,
and `monk`. Add `guild_only=true` to keep only players marked as guildmates:

```text
GET /clients/ZiLo/who?classes=priest,wizard&guild_only=true
```

Filters never reorder the list. An unknown class returns `400 Bad Request`.

## Request behavior

This route asks the game server for fresh data instead of reading the normal
character snapshot. Requests made within one second share the same in-flight
or recently completed result. If no matching response arrives within three
seconds, the route returns `504 Gateway Timeout`.

daRPC captures its own response before the client opens the Who panel, so the
request does not interrupt play. A Who request made by the player still opens
and updates the normal client panel. The outbound and inbound requests are
matched in order so one does not consume the other's response.

Who is a point-in-time query, not a live state stream. It does not produce a
Server-Sent Events (SSE) event. Request it again when a consumer needs a newer
list.

For daemon-free use, the direct command returns the same unfiltered list:

```text
darpc who --pid <pid>
darpc --output json who --pid <pid>
```
