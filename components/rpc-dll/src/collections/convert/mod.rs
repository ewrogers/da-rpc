use super::*;

pub(crate) fn inventory_item(item: RawInventoryItem) -> InventoryItem {
    InventoryItem {
        slot: item.slot,
        sprite: item.sprite & 0x3FFF,
        dye_color: item.dye_color,
        name: inventory_name(item.name, item.can_stack, item.quantity),
        quantity: item.quantity.max(1),
        can_stack: item.can_stack,
        durability: item.durability,
        max_durability: item.max_durability,
    }
}

pub(crate) fn spell(raw: RawSpell) -> Spell {
    let (name, level, max_level) =
        parsed_ability_name(raw.name, raw.name_suffix_left, raw.base_name_length);
    let target_type = SpellTargetType::from_raw(raw.argument_type);
    Spell {
        slot: raw.slot,
        icon: raw.icon,
        name,
        level,
        max_level,
        lines: raw.cast_lines,
        target_type,
        prompt: if target_type == SpellTargetType::TextInput {
            raw.prompt.and_then(ascii_text)
        } else {
            None
        },
        cooldown: CooldownStatus {
            active: raw.action_delay_active,
            cooldown_ms: None,
            remaining_ms: None,
        },
    }
}

pub(crate) fn skill_model(raw: RawSkill, tick_ms: u32) -> Skill {
    let (name, level, max_level) =
        parsed_ability_name(raw.name, raw.name_suffix_left, raw.base_name_length);
    let cooldown_ms = raw
        .cooldown_visual_active
        .then(|| raw.cooldown_ends_at.wrapping_sub(raw.cooldown_started_at))
        .filter(|duration| *duration <= i32::MAX as u32);
    let remaining_ms = raw
        .cooldown_visual_active
        .then(|| raw.cooldown_ends_at.wrapping_sub(tick_ms))
        .filter(|remaining| *remaining <= i32::MAX as u32)
        .map(|remaining| cooldown_ms.map_or(remaining, |duration| remaining.min(duration)));
    Skill {
        slot: raw.slot,
        icon: raw.icon,
        name,
        level,
        max_level,
        cooldown: CooldownStatus {
            active: raw.cooldown_visual_active || raw.action_delay_active,
            cooldown_ms,
            remaining_ms,
        },
    }
}

fn parsed_ability_name(
    raw: darpc_game_client::RawClientText<128>,
    suffix_left: i32,
    base_name_length: i32,
) -> (Option<String>, u8, u8) {
    let Some(decoded) = text(raw) else {
        return (None, 0, 0);
    };
    let decoded = decoded.trim();
    if let Some(marker) = decoded.rfind("(Lev:") {
        let name = decoded[..marker].trim();
        let levels = decoded[marker + 5..].strip_suffix(')').unwrap_or_default();
        if let Some((level, max_level)) = levels.split_once('/')
            && let (Ok(level), Ok(max_level)) = (level.parse::<u8>(), max_level.parse::<u8>())
            && !name.is_empty()
        {
            return (Some(name.to_owned()), level, max_level);
        }
    }
    let base_name = usize::try_from(base_name_length)
        .ok()
        .filter(|length| *length > 0)
        .and_then(|length| decoded.get(..length))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(decoded);
    (
        (!base_name.is_empty()).then(|| base_name.to_owned()),
        u8::try_from(suffix_left).unwrap_or(0),
        0,
    )
}

fn inventory_name(
    raw: darpc_game_client::RawClientText<128>,
    can_stack: bool,
    quantity: u32,
) -> Option<String> {
    let decoded = text(raw)?;
    if !can_stack {
        return Some(decoded);
    }
    let trimmed = decoded.trim();
    let canonical = trimmed
        .strip_suffix(']')
        .and_then(|value| value.rsplit_once('['))
        .filter(|(_, count)| count.trim().parse::<u32>() == Ok(quantity))
        .map(|(name, _)| name.trim_end())
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed);
    Some(canonical.to_owned())
}

fn ascii_text(raw: darpc_game_client::RawClientText<128>) -> Option<String> {
    let bytes = raw.bytes[..usize::from(raw.length)]
        .iter()
        .copied()
        .filter(u8::is_ascii)
        .collect::<Vec<_>>();
    let text = String::from_utf8(bytes).expect("filtered bytes are ASCII");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn text(raw: darpc_game_client::RawClientText<128>) -> Option<String> {
    crate::client_text::decode(&raw.bytes[..usize::from(raw.length)])
}

pub(super) fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}
