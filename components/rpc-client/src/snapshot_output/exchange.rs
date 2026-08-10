use darpc_model::{ExchangeOffer, ExchangeState};
use serde_json::json;
use std::fmt::Write as _;

pub(super) fn exchange_value(exchange: &ExchangeState) -> serde_json::Value {
    json!({
        "id": exchange.id,
        "partner": exchange.partner,
        "local": offer_value(&exchange.local),
        "other": offer_value(&exchange.other),
    })
}

fn offer_value(offer: &ExchangeOffer) -> serde_json::Value {
    json!({
        "items": offer.items.iter().map(|item| json!({
            "index": item.index,
            "sprite": item.sprite,
            "dye_color": item.dye_color,
            "quantity": item.quantity,
            "name": item.name,
        })).collect::<Vec<_>>(),
        "gold": offer.gold,
        "accepted": offer.accepted,
    })
}

pub(super) fn render_exchange(output: &mut String, exchange: Option<&ExchangeState>) {
    let Some(exchange) = exchange else {
        output.push_str("\nexchange: unavailable");
        return;
    };
    let _ = write!(
        output,
        "\nexchange: id={} partner={} local_gold={} other_gold={} local_accepted={} other_accepted={}",
        exchange.id,
        exchange.partner,
        exchange.local.gold,
        exchange.other.gold,
        exchange.local.accepted,
        exchange.other.accepted,
    );
    render_offer(output, "local", &exchange.local);
    render_offer(output, "other", &exchange.other);
}

fn render_offer(output: &mut String, party: &str, offer: &ExchangeOffer) {
    for item in &offer.items {
        let quantity = item
            .quantity
            .map_or_else(|| "unavailable".to_owned(), |value| value.to_string());
        let _ = write!(
            output,
            "\nexchange_item: party={} index={} name={} quantity={} sprite={} dye_color={}",
            party, item.index, item.name, quantity, item.sprite, item.dye_color,
        );
    }
}
