# Emotes

daRPC can play the same character expressions exposed by the normal client UI.

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"wave"}' \
  "http://127.0.0.1:2626/clients/ZiLo/emote"
```

Names are case-insensitive. The confirmed names are:

| Ctrl shortcut | Name | Code | Ctrl+Alt shortcut | Name | Code |
| --- | --- | ---: | --- | --- | ---: |
| Ctrl+1 | `smile` | 0 | Ctrl+Alt+1 | `rock` | 25 |
| Ctrl+2 | `cry` | 1 | Ctrl+Alt+2 | `scissors` | 26 |
| Ctrl+3 | `sad` | 2 | Ctrl+Alt+3 | `paper` | 27 |
| Ctrl+4 | `wink` | 3 | Ctrl+Alt+4 | `oof` | 28 |
| Ctrl+5 | `stunned` | 4 | Ctrl+Alt+5 | `speechless` | 29 |
| Ctrl+6 | `raz` | 5 | Ctrl+Alt+6 | `blue` | 30 |
| Ctrl+7 | `surprise` | 6 | Ctrl+Alt+7 | `blush` | 31 |
| Ctrl+8 | `sleepy` | 7 | Ctrl+Alt+8 | `heart` | 32 |
| Ctrl+9 | `yawn` | 8 | Ctrl+Alt+9 | `sweat` | 33 |
| Ctrl+0 | `kiss` | 12 | Ctrl+Alt+0 | `sing` | 34 |
| Ctrl+- | `wave` | 13 | Ctrl+Alt+- | `ack` | 35 |

You may provide a numeric client code instead, for example `{"code":13}`.
Numeric codes also keep the unnamed Alt-only expressions available. A code must
be one exposed by the client UI: 0 through 8 or 12 through 35.

The HTTP response reports whether the main-thread command ran. An observed
request also produces `character.emoted` with the numeric code.
