use super::{decode_optional_string, encode_optional_string};
use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u16, push_u32},
};
use darpc_model::{
    CharacterSnapshot, CooldownStatus, Effect, EffectDuration, EquipmentItem, EquipmentSlot,
    InventoryItem, Skill, Spell, SpellTargetType,
};

const INVENTORY_SLOT_COUNT: usize = 60;
const EQUIPMENT_SLOT_COUNT: usize = 18;
const ABILITY_SLOT_COUNT: usize = 90;
const EFFECT_SLOT_COUNT: usize = 10;
const MAX_COLLECTION_NAME_LEN: usize = 127;

pub(super) struct DecodedCollections {
    pub(super) inventory: Option<Vec<InventoryItem>>,
    pub(super) equipment: Option<Vec<EquipmentItem>>,
    pub(super) spellbook: Option<Vec<Spell>>,
    pub(super) skillbook: Option<Vec<Skill>>,
    pub(super) effects: Option<Vec<Effect>>,
}

pub(super) fn encode(
    output: &mut Vec<u8>,
    character: &CharacterSnapshot,
) -> Result<(), EncodeError> {
    encode_inventory(output, character.inventory.as_deref())?;
    encode_equipment(output, character.equipment.as_deref())?;
    encode_spells(output, character.spellbook.as_deref())?;
    encode_skills(output, character.skillbook.as_deref())?;
    encode_effects(output, character.effects.as_deref())?;
    Ok(())
}

pub(super) fn decode(reader: &mut PayloadReader<'_>) -> Result<DecodedCollections, DecodeError> {
    Ok(DecodedCollections {
        inventory: decode_inventory(reader)?,
        equipment: decode_equipment(reader)?,
        spellbook: decode_spells(reader)?,
        skillbook: decode_skills(reader)?,
        effects: decode_effects(reader)?,
    })
}

fn encode_effects(output: &mut Vec<u8>, effects: Option<&[Effect]>) -> Result<(), EncodeError> {
    let Some(effects) = encode_collection_header(output, effects, EFFECT_SLOT_COUNT)? else {
        return Ok(());
    };
    for (index, effect) in effects.iter().enumerate() {
        if effects[..index]
            .iter()
            .any(|current| current.icon == effect.icon)
        {
            return Err(EncodeError::DuplicateEffectIcon { icon: effect.icon });
        }
        push_u16(output, effect.icon);
        output.push(effect.duration.raw());
    }
    Ok(())
}

fn decode_effects(reader: &mut PayloadReader<'_>) -> Result<Option<Vec<Effect>>, DecodeError> {
    let Some(count) = decode_collection_header(reader, EFFECT_SLOT_COUNT)? else {
        return Ok(None);
    };
    let mut effects = Vec::with_capacity(count);
    for _ in 0..count {
        let icon = reader.read_u16()?;
        if effects.iter().any(|effect: &Effect| effect.icon == icon) {
            return Err(DecodeError::DuplicateEffectIcon { icon });
        }
        let duration = reader.read_u8()?;
        effects.push(Effect {
            icon,
            duration: EffectDuration::from_raw(duration)
                .ok_or(DecodeError::InvalidEffectDuration { actual: duration })?,
        });
    }
    Ok(Some(effects))
}

fn encode_inventory(
    output: &mut Vec<u8>,
    items: Option<&[InventoryItem]>,
) -> Result<(), EncodeError> {
    let Some(items) = encode_collection_header(output, items, INVENTORY_SLOT_COUNT)? else {
        return Ok(());
    };
    let mut slots = [false; INVENTORY_SLOT_COUNT];
    for item in items {
        encode_slot(item.slot, &mut slots)?;
        output.push(item.slot);
        push_u16(output, item.sprite);
        output.push(item.dye_color);
        encode_optional_string(output, item.name.as_deref(), MAX_COLLECTION_NAME_LEN)?;
        push_u32(output, item.quantity);
        push_bool(output, item.can_stack);
        push_u32(output, item.durability);
        push_u32(output, item.max_durability);
    }
    Ok(())
}

fn decode_inventory(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<Vec<InventoryItem>>, DecodeError> {
    let Some(count) = decode_collection_header(reader, INVENTORY_SLOT_COUNT)? else {
        return Ok(None);
    };
    let mut slots = [false; INVENTORY_SLOT_COUNT];
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = decode_slot(reader, &mut slots)?;
        items.push(InventoryItem {
            slot,
            sprite: reader.read_u16()?,
            dye_color: reader.read_u8()?,
            name: decode_optional_string(reader, MAX_COLLECTION_NAME_LEN)?,
            quantity: reader.read_u32()?,
            can_stack: reader.read_bool()?,
            durability: reader.read_u32()?,
            max_durability: reader.read_u32()?,
        });
    }
    Ok(Some(items))
}

fn encode_equipment(
    output: &mut Vec<u8>,
    items: Option<&[EquipmentItem]>,
) -> Result<(), EncodeError> {
    let Some(items) = encode_collection_header(output, items, EQUIPMENT_SLOT_COUNT)? else {
        return Ok(());
    };
    let mut slots = [false; EQUIPMENT_SLOT_COUNT];
    for item in items {
        let slot = item.slot.raw();
        encode_slot(slot, &mut slots)?;
        output.push(slot);
        push_u16(output, item.sprite);
        output.push(item.dye_color);
        encode_optional_string(output, item.name.as_deref(), MAX_COLLECTION_NAME_LEN)?;
        push_u32(output, item.durability);
        push_u32(output, item.max_durability);
    }
    Ok(())
}

fn decode_equipment(
    reader: &mut PayloadReader<'_>,
) -> Result<Option<Vec<EquipmentItem>>, DecodeError> {
    let Some(count) = decode_collection_header(reader, EQUIPMENT_SLOT_COUNT)? else {
        return Ok(None);
    };
    let mut slots = [false; EQUIPMENT_SLOT_COUNT];
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = decode_slot(reader, &mut slots)?;
        items.push(EquipmentItem {
            slot: EquipmentSlot::from_raw(slot).expect("validated equipment slot is known"),
            sprite: reader.read_u16()?,
            dye_color: reader.read_u8()?,
            name: decode_optional_string(reader, MAX_COLLECTION_NAME_LEN)?,
            durability: reader.read_u32()?,
            max_durability: reader.read_u32()?,
        });
    }
    Ok(Some(items))
}

fn encode_spells(output: &mut Vec<u8>, spells: Option<&[Spell]>) -> Result<(), EncodeError> {
    let Some(spells) = encode_collection_header(output, spells, ABILITY_SLOT_COUNT)? else {
        return Ok(());
    };
    let mut slots = [false; ABILITY_SLOT_COUNT];
    for spell in spells {
        encode_slot(spell.slot, &mut slots)?;
        output.push(spell.slot);
        push_u16(output, spell.icon);
        encode_optional_string(output, spell.name.as_deref(), MAX_COLLECTION_NAME_LEN)?;
        output.push(spell.level);
        output.push(spell.max_level);
        output.push(spell.lines);
        output.push(spell.target_type.raw());
        encode_optional_string(output, spell.prompt.as_deref(), MAX_COLLECTION_NAME_LEN)?;
        encode_cooldown(output, spell.cooldown);
    }
    Ok(())
}

fn decode_spells(reader: &mut PayloadReader<'_>) -> Result<Option<Vec<Spell>>, DecodeError> {
    let Some(count) = decode_collection_header(reader, ABILITY_SLOT_COUNT)? else {
        return Ok(None);
    };
    let mut slots = [false; ABILITY_SLOT_COUNT];
    let mut spells = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = decode_slot(reader, &mut slots)?;
        spells.push(Spell {
            slot,
            icon: reader.read_u16()?,
            name: decode_optional_string(reader, MAX_COLLECTION_NAME_LEN)?,
            level: reader.read_u8()?,
            max_level: reader.read_u8()?,
            lines: reader.read_u8()?,
            target_type: SpellTargetType::from_raw(reader.read_u8()?),
            prompt: decode_optional_string(reader, MAX_COLLECTION_NAME_LEN)?,
            cooldown: decode_cooldown(reader)?,
        });
    }
    Ok(Some(spells))
}

fn encode_skills(output: &mut Vec<u8>, skills: Option<&[Skill]>) -> Result<(), EncodeError> {
    let Some(skills) = encode_collection_header(output, skills, ABILITY_SLOT_COUNT)? else {
        return Ok(());
    };
    let mut slots = [false; ABILITY_SLOT_COUNT];
    for skill in skills {
        encode_slot(skill.slot, &mut slots)?;
        output.push(skill.slot);
        push_u16(output, skill.icon);
        encode_optional_string(output, skill.name.as_deref(), MAX_COLLECTION_NAME_LEN)?;
        output.push(skill.level);
        output.push(skill.max_level);
        encode_cooldown(output, skill.cooldown);
    }
    Ok(())
}

fn decode_skills(reader: &mut PayloadReader<'_>) -> Result<Option<Vec<Skill>>, DecodeError> {
    let Some(count) = decode_collection_header(reader, ABILITY_SLOT_COUNT)? else {
        return Ok(None);
    };
    let mut slots = [false; ABILITY_SLOT_COUNT];
    let mut skills = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = decode_slot(reader, &mut slots)?;
        skills.push(Skill {
            slot,
            icon: reader.read_u16()?,
            name: decode_optional_string(reader, MAX_COLLECTION_NAME_LEN)?,
            level: reader.read_u8()?,
            max_level: reader.read_u8()?,
            cooldown: decode_cooldown(reader)?,
        });
    }
    Ok(Some(skills))
}

fn encode_collection_header<'a, T>(
    output: &mut Vec<u8>,
    values: Option<&'a [T]>,
    max: usize,
) -> Result<Option<&'a [T]>, EncodeError> {
    push_bool(output, values.is_some());
    let Some(values) = values else {
        return Ok(None);
    };
    if values.len() > max {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: values.len(),
            max,
        });
    }
    output.push(u8::try_from(values.len()).map_err(|_| EncodeError::LengthOverflow)?);
    Ok(Some(values))
}

fn decode_collection_header(
    reader: &mut PayloadReader<'_>,
    max: usize,
) -> Result<Option<usize>, DecodeError> {
    if !reader.read_bool()? {
        return Ok(None);
    }
    let length = usize::from(reader.read_u8()?);
    if length > max {
        return Err(DecodeError::SnapshotCollectionTooLong { length, max });
    }
    Ok(Some(length))
}

fn encode_slot<const N: usize>(slot: u8, slots: &mut [bool; N]) -> Result<(), EncodeError> {
    let index = usize::from(slot)
        .checked_sub(1)
        .filter(|index| *index < N)
        .ok_or(EncodeError::InvalidSnapshotSlot {
            slot,
            max: u8::try_from(N).expect("snapshot slot count fits u8"),
        })?;
    if slots[index] {
        return Err(EncodeError::DuplicateSnapshotSlot { slot });
    }
    slots[index] = true;
    Ok(())
}

fn decode_slot<const N: usize>(
    reader: &mut PayloadReader<'_>,
    slots: &mut [bool; N],
) -> Result<u8, DecodeError> {
    let slot = reader.read_u8()?;
    let index = usize::from(slot)
        .checked_sub(1)
        .filter(|index| *index < N)
        .ok_or(DecodeError::InvalidSnapshotSlot {
            slot,
            max: u8::try_from(N).expect("snapshot slot count fits u8"),
        })?;
    if slots[index] {
        return Err(DecodeError::DuplicateSnapshotSlot { slot });
    }
    slots[index] = true;
    Ok(slot)
}

fn encode_cooldown(output: &mut Vec<u8>, cooldown: CooldownStatus) {
    push_bool(output, cooldown.active);
    push_bool(output, cooldown.remaining_ms.is_some());
    if let Some(remaining_ms) = cooldown.remaining_ms {
        push_u32(output, remaining_ms);
    }
}

fn decode_cooldown(reader: &mut PayloadReader<'_>) -> Result<CooldownStatus, DecodeError> {
    Ok(CooldownStatus {
        active: reader.read_bool()?,
        remaining_ms: if reader.read_bool()? {
            Some(reader.read_u32()?)
        } else {
            None
        },
    })
}
