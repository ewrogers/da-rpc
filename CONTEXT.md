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

**Bulletin**:
The active native client session for global boards, trade boards, world boards,
guild boards, and player mail. Its state includes one active view: sections,
entries, an entry, or a composer, plus navigation and operation state.
_Avoid_: message board state, bulletin window

**Bulletin section**:
A server-identified board or mailbox presented within a bulletin session. A
section owns an entry list and retains its observed source.
_Avoid_: category, channel

**Bulletin entry**:
One board article or player-mail message, identified within a bulletin section.
An entry summary belongs to a list; an opened entry also includes its body and
raw navigation fields.
_Avoid_: post when referring to mail, email

**Bulletin composer**:
The native new-article or player-mail editing view. Its current unsent field
values are the draft; board drafts have subject and body, while mail drafts also
have a recipient.
_Avoid_: draft as a separate persisted object

**Bulletin mutation outcome**:
A server-confirmed submission or deletion, or a rejected bulletin mutation. A
failure names the attempted bulletin action and retains the server's raw result.
_Avoid_: operation result, action submitted
