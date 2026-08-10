use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16, push_u32},
};
use darpc_model::{ExchangeItem, ExchangeOffer, ExchangeParty, ExchangeState, ExchangeUpdate};

pub const MAX_EXCHANGE_ITEMS: usize = 8;
pub const MAX_EXCHANGE_NAME_LEN: usize = 255;
pub const MAX_EXCHANGE_MESSAGE_LEN: usize = 255;

pub(crate) fn encode_optional_state(
    output: &mut Vec<u8>,
    state: Option<&ExchangeState>,
) -> Result<(), EncodeError> {
    push_bool(output, state.is_some());
    if let Some(state) = state {
        encode_state(output, state)?;
    }
    Ok(())
}

pub(crate) fn decode_optional_state(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<ExchangeState>, DecodeError> {
    reader
        .read_bool()?
        .then(|| decode_state(reader))
        .transpose()
}

pub(crate) fn encode_update(
    output: &mut Vec<u8>,
    update: &ExchangeUpdate,
) -> Result<(), EncodeError> {
    match update {
        ExchangeUpdate::Opened(state) => {
            output.push(1);
            encode_state(output, state)?;
        }
        ExchangeUpdate::ItemAdded { state, party, item } => {
            output.push(2);
            encode_state(output, state)?;
            encode_party(output, *party);
            encode_item(output, item)?;
        }
        ExchangeUpdate::GoldChanged { state, party, gold } => {
            output.push(3);
            encode_state(output, state)?;
            encode_party(output, *party);
            push_u32(output, *gold);
        }
        ExchangeUpdate::Accepted {
            state,
            party,
            message,
        } => {
            output.push(4);
            encode_state(output, state)?;
            encode_party(output, *party);
            encode_string(output, message, MAX_EXCHANGE_MESSAGE_LEN)?;
        }
        ExchangeUpdate::Completed { state, message } => {
            output.push(5);
            encode_state(output, state)?;
            encode_string(output, message, MAX_EXCHANGE_MESSAGE_LEN)?;
        }
        ExchangeUpdate::Cancelled { state, message } => {
            output.push(6);
            encode_state(output, state)?;
            encode_string(output, message, MAX_EXCHANGE_MESSAGE_LEN)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_update(reader: &mut PayloadReader<'_>) -> Result<ExchangeUpdate, DecodeError> {
    match reader.read_u8()? {
        1 => Ok(ExchangeUpdate::Opened(decode_state(reader)?)),
        2 => Ok(ExchangeUpdate::ItemAdded {
            state: decode_state(reader)?,
            party: decode_party(reader)?,
            item: decode_item(reader)?,
        }),
        3 => Ok(ExchangeUpdate::GoldChanged {
            state: decode_state(reader)?,
            party: decode_party(reader)?,
            gold: reader.read_u32()?,
        }),
        4 => Ok(ExchangeUpdate::Accepted {
            state: decode_state(reader)?,
            party: decode_party(reader)?,
            message: decode_string(reader, MAX_EXCHANGE_MESSAGE_LEN)?,
        }),
        5 => Ok(ExchangeUpdate::Completed {
            state: decode_state(reader)?,
            message: decode_string(reader, MAX_EXCHANGE_MESSAGE_LEN)?,
        }),
        6 => Ok(ExchangeUpdate::Cancelled {
            state: decode_state(reader)?,
            message: decode_string(reader, MAX_EXCHANGE_MESSAGE_LEN)?,
        }),
        actual => Err(DecodeError::InvalidExchangeField { actual }),
    }
}

fn encode_state(output: &mut Vec<u8>, state: &ExchangeState) -> Result<(), EncodeError> {
    push_u32(output, state.id);
    encode_string(output, &state.partner, MAX_EXCHANGE_NAME_LEN)?;
    encode_offer(output, &state.local)?;
    encode_offer(output, &state.other)
}

fn decode_state(reader: &mut PayloadReader<'_>) -> Result<ExchangeState, DecodeError> {
    Ok(ExchangeState {
        id: reader.read_u32()?,
        partner: decode_string(reader, MAX_EXCHANGE_NAME_LEN)?,
        local: decode_offer(reader)?,
        other: decode_offer(reader)?,
    })
}

fn encode_offer(output: &mut Vec<u8>, offer: &ExchangeOffer) -> Result<(), EncodeError> {
    if offer.items.len() > MAX_EXCHANGE_ITEMS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: offer.items.len(),
            max: MAX_EXCHANGE_ITEMS,
        });
    }
    output.push(offer.items.len() as u8);
    for item in &offer.items {
        encode_item(output, item)?;
    }
    push_u32(output, offer.gold);
    push_bool(output, offer.accepted);
    Ok(())
}

fn decode_offer(reader: &mut PayloadReader<'_>) -> Result<ExchangeOffer, DecodeError> {
    let count = usize::from(reader.read_u8()?);
    if count > MAX_EXCHANGE_ITEMS {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_EXCHANGE_ITEMS,
        });
    }
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(decode_item(reader)?);
    }
    Ok(ExchangeOffer {
        items,
        gold: reader.read_u32()?,
        accepted: reader.read_bool()?,
    })
}

fn encode_item(output: &mut Vec<u8>, item: &ExchangeItem) -> Result<(), EncodeError> {
    output.push(item.index);
    push_u16(output, item.sprite);
    output.push(item.dye_color);
    push_bool(output, item.quantity.is_some());
    if let Some(quantity) = item.quantity {
        output.push(quantity);
    }
    encode_string(output, &item.name, MAX_EXCHANGE_NAME_LEN)
}

fn decode_item(reader: &mut PayloadReader<'_>) -> Result<ExchangeItem, DecodeError> {
    let index = reader.read_u8()?;
    if usize::from(index) >= MAX_EXCHANGE_ITEMS {
        return Err(DecodeError::InvalidExchangeField { actual: index });
    }
    Ok(ExchangeItem {
        index,
        sprite: reader.read_u16()?,
        dye_color: reader.read_u8()?,
        quantity: reader.read_bool()?.then(|| reader.read_u8()).transpose()?,
        name: decode_string(reader, MAX_EXCHANGE_NAME_LEN)?,
    })
}

fn encode_party(output: &mut Vec<u8>, party: ExchangeParty) {
    output.push(match party {
        ExchangeParty::Local => 0,
        ExchangeParty::Other => 1,
    });
}

fn decode_party(reader: &mut PayloadReader<'_>) -> Result<ExchangeParty, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(ExchangeParty::Local),
        1 => Ok(ExchangeParty::Other),
        actual => Err(DecodeError::InvalidExchangeField { actual }),
    }
}

fn encode_string(output: &mut Vec<u8>, value: &str, max: usize) -> Result<(), EncodeError> {
    if value.len() > max {
        return Err(EncodeError::EventStringTooLong {
            length: value.len(),
            max,
        });
    }
    let length = u16::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow)?;
    push_u16(output, length);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_string(reader: &mut PayloadReader<'_>, max: usize) -> Result<String, DecodeError> {
    let length = usize::from(reader.read_u16()?);
    if length > max {
        return Err(DecodeError::EventStringTooLong { length, max });
    }
    std::str::from_utf8(reader.take(length)?)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}
