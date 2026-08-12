use super::RawDialog;
use darpc_model::{
    DialogChoice, DialogInput, DialogInteraction, DialogItem, DialogKind, DialogNavigation,
    DialogSlot, DialogSpeaker, DialogSpriteType, DialogState, DialogTarget,
};

const MAX_DIALOG_ROWS: usize = 512;
const CREATURE_TAG: u16 = 0x4000;
const ITEM_TAG: u16 = 0x8000;
const SPRITE_MASK: u16 = 0x3FFF;

pub(super) fn decode(raw: RawDialog) -> Result<DialogState, ()> {
    let body = &raw.bytes[..usize::from(raw.length)];
    match body.first().copied() {
        Some(0x2F) => decode_merchant(body, raw.revision, raw.response_pending),
        Some(0x30) => decode_pursuit(body, raw.revision, raw.response_pending),
        _ => Err(()),
    }
}

fn decode_merchant(body: &[u8], revision: u32, pending: bool) -> Result<DialogState, ()> {
    let mut reader = Reader::new(body);
    reader.u8()?;
    let menu_type = reader.u8()?;
    let (target, speaker) = common(&mut reader, false)?;
    let content = reader.string16()?;
    let interaction = match menu_type {
        0 | 1 => {
            if menu_type == 1 {
                reader.string8()?;
            }
            DialogInteraction::Choices(choices(&mut reader, true)?)
        }
        2 | 3 => {
            if menu_type == 3 {
                reader.string8()?;
            }
            reader.u16()?;
            DialogInteraction::Input(DialogInput {
                prolog: None,
                maximum_bytes: u8::MAX,
                epilog: None,
            })
        }
        4 | 10 => merchant_items(&mut reader)?,
        5 | 11 => merchant_slots(&mut reader, DialogKind::Merchant)?,
        6 | 7 => merchant_abilities(&mut reader, menu_type)?,
        8 | 9 => merchant_books(&mut reader, menu_type)?,
        _ => DialogInteraction::Unsupported,
    };
    Ok(DialogState {
        revision,
        kind: DialogKind::Merchant,
        target,
        speaker,
        content: Some(content),
        response_pending: pending,
        navigation: DialogNavigation {
            close: true,
            ..DialogNavigation::default()
        },
        interaction,
    })
}

fn decode_pursuit(body: &[u8], revision: u32, pending: bool) -> Result<DialogState, ()> {
    let mut reader = Reader::new(body);
    reader.u8()?;
    let dialog_type = reader.u8()?;
    if dialog_type == 10 {
        return Err(());
    }
    let (target, mut speaker) = common(&mut reader, true)?;
    reader.u16()?;
    reader.u16()?;
    let previous = reader.u8()? != 0;
    let next = reader.u8()? != 0;
    speaker.show_graphic = reader.u8()? != 0;
    speaker.name = option_text(reader.string8()?);
    let content = matches!(dialog_type, 0 | 2 | 4 | 6 | 9)
        .then(|| reader.string16())
        .transpose()?;
    let interaction = match dialog_type {
        0 | 1 => DialogInteraction::Message,
        2 | 3 | 6 => DialogInteraction::Choices(choices(&mut reader, false)?),
        4 | 5 => DialogInteraction::Input(DialogInput {
            prolog: option_text(reader.string8()?),
            maximum_bytes: reader.u8()?,
            epilog: option_text(reader.string8()?),
        }),
        9 => {
            reader.string8()?;
            reader.u8()?;
            reader.string8()?;
            DialogInteraction::Protected
        }
        _ => DialogInteraction::Unsupported,
    };
    Ok(DialogState {
        revision,
        kind: DialogKind::Pursuit,
        target,
        speaker,
        content,
        response_pending: pending,
        navigation: DialogNavigation {
            previous: previous && !pending,
            next: next && !pending,
            close: true,
        },
        interaction,
    })
}

fn common(reader: &mut Reader<'_>, pursuit: bool) -> Result<(DialogTarget, DialogSpeaker), ()> {
    reader.u8()?;
    let id = reader.u32()?;
    reader.skip(1)?;
    let raw_sprite = reader.u16()?;
    let color = reader.u8()?;
    reader.skip(4)?;
    let (sprite_type, sprite) = if raw_sprite & ITEM_TAG != 0 {
        (DialogSpriteType::Item, raw_sprite & SPRITE_MASK)
    } else if raw_sprite & CREATURE_TAG != 0 {
        (DialogSpriteType::Creature, raw_sprite & SPRITE_MASK)
    } else {
        (DialogSpriteType::Unknown, raw_sprite & SPRITE_MASK)
    };
    let mut speaker = DialogSpeaker {
        name: None,
        sprite,
        sprite_type,
        color,
        show_graphic: false,
    };
    if !pursuit {
        speaker.show_graphic = reader.u8()? != 0;
        speaker.name = option_text(reader.string8()?);
    }
    Ok((DialogTarget { id }, speaker))
}

fn choices(reader: &mut Reader<'_>, has_ids: bool) -> Result<Vec<DialogChoice>, ()> {
    let count = usize::from(reader.u8()?);
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let text = reader.string8()?;
        if has_ids {
            reader.u16()?;
        }
        result.push(DialogChoice {
            index: index as u16,
            text,
        });
    }
    Ok(result)
}

fn merchant_items(reader: &mut Reader<'_>) -> Result<DialogInteraction, ()> {
    let pursuit_id = reader.u16()?;
    let count = reader.row_count16()?;
    let mut items = Vec::with_capacity(count);
    for index in 0..count {
        let (sprite, color, value, quantity, name, description) = if pursuit_id == 0x004B {
            reader.u32()?;
            let sprite = reader.u16()? & SPRITE_MASK;
            let color = reader.u8()?;
            let value = reader.u32()?;
            let quantity = Some(reader.u8()?);
            let name = option_text(reader.string8()?);
            let description = if reader.u8()? == 1 {
                option_text(reader.string8()?)
            } else {
                None
            };
            reader.u32()?;
            reader.u32()?;
            (sprite, color, Some(value), quantity, name, description)
        } else {
            let sprite = reader.u16()? & SPRITE_MASK;
            let color = reader.u8()?;
            let value = Some(reader.u32()?);
            let name = option_text(reader.string8()?);
            let description = option_text(reader.string8()?);
            (sprite, color, value, None, name, description)
        };
        items.push(DialogItem {
            index: index as u16,
            sprite,
            color,
            name,
            description,
            value,
            available_quantity: quantity,
        });
    }
    Ok(DialogInteraction::Items(items))
}

fn merchant_slots(reader: &mut Reader<'_>, _: DialogKind) -> Result<DialogInteraction, ()> {
    let pursuit_id = reader.u16()?;
    let count = usize::from(reader.u8()?);
    let mut slots = Vec::with_capacity(count);
    for index in 0..count {
        slots.push(DialogSlot {
            index: index as u16,
            slot: reader.u8()?,
            value: (pursuit_id == 0x004E).then(|| reader.u32()).transpose()?,
            name: None,
            sprite: None,
            color: None,
        });
    }
    Ok(DialogInteraction::Inventory(slots))
}

fn merchant_abilities(reader: &mut Reader<'_>, menu_type: u8) -> Result<DialogInteraction, ()> {
    reader.u16()?;
    let count = reader.row_count16()?;
    let mut slots = Vec::with_capacity(count);
    for index in 0..count {
        reader.u8()?;
        let sprite = reader.u16()? & SPRITE_MASK;
        let color = reader.u8()?;
        slots.push(DialogSlot {
            index: index as u16,
            slot: 0,
            value: None,
            name: option_text(reader.string8()?),
            sprite: Some(sprite),
            color: Some(color),
        });
    }
    Ok(if menu_type == 6 {
        DialogInteraction::Spells(slots)
    } else {
        DialogInteraction::Skills(slots)
    })
}

fn merchant_books(reader: &mut Reader<'_>, menu_type: u8) -> Result<DialogInteraction, ()> {
    reader.u16()?;
    let count = if reader.remaining() == 0 {
        0
    } else {
        usize::from(reader.u8()?)
    };
    let mut slots = Vec::with_capacity(count);
    for index in 0..count {
        slots.push(DialogSlot {
            index: index as u16,
            slot: reader.u8()?,
            value: None,
            name: None,
            sprite: None,
            color: None,
        });
    }
    Ok(if menu_type == 8 {
        DialogInteraction::Spells(slots)
    } else {
        DialogInteraction::Skills(slots)
    })
}

fn option_text(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

pub(super) fn decode_text(bytes: &[u8]) -> String {
    crate::client_text::decode(bytes).unwrap_or_default()
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ()> {
        let end = self.offset.checked_add(length).ok_or(())?;
        let value = self.bytes.get(self.offset..end).ok_or(())?;
        self.offset = end;
        Ok(value)
    }

    fn skip(&mut self, length: usize) -> Result<(), ()> {
        self.take(length).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8, ()> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ()> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| ())?,
        ))
    }

    fn u32(&mut self) -> Result<u32, ()> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| ())?,
        ))
    }

    fn string8(&mut self) -> Result<String, ()> {
        let length = usize::from(self.u8()?);
        Ok(decode_text(self.take(length)?))
    }

    fn string16(&mut self) -> Result<String, ()> {
        let length = usize::from(self.u16()?);
        Ok(decode_text(self.take(length)?))
    }

    fn row_count16(&mut self) -> Result<usize, ()> {
        let count = usize::from(self.u16()?);
        (count <= MAX_DIALOG_ROWS).then_some(count).ok_or(())
    }
}
