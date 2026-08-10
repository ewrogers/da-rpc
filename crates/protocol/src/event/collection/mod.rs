use super::*;

pub(super) fn encode_slot_update<T>(
    output: &mut Vec<u8>,
    update: &SlotUpdate<T>,
    encode_item: fn(&mut Vec<u8>, &T) -> Result<(), EncodeError>,
) -> Result<(), EncodeError> {
    if update.batch_count == 0 || update.batch_index >= update.batch_count {
        return Err(EncodeError::InvalidCollectionBatch {
            index: update.batch_index,
            count: update.batch_count,
        });
    }
    output.push(update.batch_index);
    output.push(update.batch_count);
    output.push(match update.change {
        CollectionChange::Added => 1,
        CollectionChange::Removed => 2,
        CollectionChange::Changed => 3,
    });
    output.push(update.slot);
    let fields = u8::from(update.before.is_some()) | (u8::from(update.after.is_some()) << 1);
    if fields == 0 {
        return Err(EncodeError::EmptyCollectionUpdate);
    }
    output.push(fields);
    if let Some(before) = &update.before {
        encode_item(output, before)?;
    }
    if let Some(after) = &update.after {
        encode_item(output, after)?;
    }
    Ok(())
}

pub(super) fn decode_slot_update<T>(
    reader: &mut PayloadReader<'_>,
    decode_item: fn(&mut PayloadReader<'_>) -> Result<T, DecodeError>,
) -> Result<SlotUpdate<T>, DecodeError>
where
    T: CollectionSlot,
{
    let batch_index = reader.read_u8()?;
    let batch_count = reader.read_u8()?;
    if batch_count == 0 || batch_index >= batch_count {
        return Err(DecodeError::InvalidCollectionBatch {
            index: batch_index,
            count: batch_count,
        });
    }
    let change = match reader.read_u8()? {
        1 => CollectionChange::Added,
        2 => CollectionChange::Removed,
        3 => CollectionChange::Changed,
        actual => return Err(DecodeError::InvalidCollectionChange { actual }),
    };
    let slot = reader.read_u8()?;
    let fields = reader.read_u8()?;
    if fields == 0 || fields & !0x03 != 0 {
        return Err(DecodeError::InvalidCollectionFields { actual: fields });
    }
    let before = (fields & 0x01 != 0)
        .then(|| decode_item(reader))
        .transpose()?;
    let after = (fields & 0x02 != 0)
        .then(|| decode_item(reader))
        .transpose()?;
    if before.as_ref().is_some_and(|item| item.slot() != slot)
        || after.as_ref().is_some_and(|item| item.slot() != slot)
    {
        return Err(DecodeError::CollectionSlotMismatch { slot });
    }
    Ok(SlotUpdate {
        batch_index,
        batch_count,
        change,
        slot,
        before,
        after,
    })
}

pub(super) trait CollectionSlot {
    fn slot(&self) -> u8;
}

impl CollectionSlot for InventoryItem {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl CollectionSlot for Spell {
    fn slot(&self) -> u8 {
        self.slot
    }
}

impl CollectionSlot for Skill {
    fn slot(&self) -> u8 {
        self.slot
    }
}
