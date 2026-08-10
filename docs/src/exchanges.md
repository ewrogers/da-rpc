# Exchange

daRPC can open and complete the game's normal player exchange without replacing
the exchange window. The server still owns the offer and decides when it is
complete or cancelled.

## Start an exchange

Use the existing give routes with a visible player name or object ID:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"Wine","quantity":3,"target":{"name":"OtherPlayer"}}' \
  "http://127.0.0.1:2626/clients/ZiLo/items/give"

curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"amount":1000,"target":{"name":"OtherPlayer"}}' \
  "http://127.0.0.1:2626/clients/ZiLo/gold/give"
```

These requests start the exchange. They do not mean ZiLo accepted it. Wait for
`exchange.opened` or read the exchange resource before adding more to the
offer.

## Read the current exchange

```console
curl "http://127.0.0.1:2626/clients/ZiLo/exchange"
```

The response contains `exchange: null` when no tracked exchange is open.
Otherwise it contains both offers:

```text
ExchangeState {
    id: u32,
    partner: string,
    local: ExchangeOffer,
    other: ExchangeOffer,
}

ExchangeOffer {
    items: ExchangeItem[],
    gold: u32,
    accepted: bool,
}

ExchangeItem {
    index: u8,
    sprite: u16,
    dye_color: u8,
    quantity: u8,
    name: string,
}
```

`index` is the zero-based position in the eight-row exchange offer. `quantity`
is 1 when the server sends no stack suffix and otherwise reflects the stack
count carried in the exchange item name.

Character status also exposes `is_in_exchange` for a quick open or closed
check.

## Change the local offer

Add an item by one-based inventory slot or case-insensitive name:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"slot":4,"quantity":3}' \
  "http://127.0.0.1:2626/clients/ZiLo/exchange/items"

curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"name":"Wine"}' \
  "http://127.0.0.1:2626/clients/ZiLo/exchange/items"
```

Quantity defaults to 1. It must fit the available stack and the exchange
protocol's range of 1 through 255. daRPC sends the game's first add-item request,
waits for the server's quantity prompt when needed, and only then submits the
count.

Set gold once:

```console
curl --request POST \
  --header "Content-Type: application/json" \
  --data '{"amount":1000}' \
  "http://127.0.0.1:2626/clients/ZiLo/exchange/gold"
```

The amount must be nonzero and no greater than the character's current gold.
The game permits gold to be set only once for an exchange. Offered items cannot
be removed individually. Cancel the exchange and begin again to change either
of those choices.

Once either player accepts, daRPC stops accepting offer changes for that
exchange.

## Accept or cancel

```console
curl --request POST \
  "http://127.0.0.1:2626/clients/ZiLo/exchange/accept"

curl --request POST \
  "http://127.0.0.1:2626/clients/ZiLo/exchange/cancel"
```

These routes send a request and leave the window open until the server confirms
the result. Completion requires both players to accept. Cancellation by either
player closes the exchange. The normal client displays a one-button result
alert on both paths. Launch with `--skip-exchange-alerts`, or set
`skip_exchange_alerts: true` in a daemon launch request, to replace that modal
with the server's same confirmation text in the floating game-message bar.
This visual confirmation does not synthesize a chat-history message. The typed
terminal exchange event remains the durable automation signal.

## Live exchange events

Subscribe through `GET /clients/{client}/events`:

| Event | Meaning |
| --- | --- |
| `exchange.opened` | The server opened an exchange window. |
| `exchange.item_added` | Either offer gained or replaced an item row. |
| `exchange.gold_changed` | Either offer's gold changed. |
| `exchange.accepted` | One player accepted and the exchange remains open. |
| `exchange.completed` | Both players accepted and the exchange closed. |
| `exchange.cancelled` | The server cancelled and closed the exchange. |

Every state-bearing event includes the complete resulting `exchange` value.
`item_added`, `gold_changed`, and `accepted` also include `party`, which is
`local` or `other`. Completion and cancellation include the final server
message and final offer state.

If an event consumer falls behind, reread `/exchange`. The DLL includes its
retained exchange in every fresh snapshot, so daemon reconnects do not lose an
exchange that daRPC has already observed.

An exchange window that was already open before late injection cannot be
reconstructed from the initial memory walk. Cancel or finish that window and
start the next exchange after daRPC is attached.
