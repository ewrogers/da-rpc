use super::ReadyPublication;
use crate::{client_text, map_name};
use darpc_game_client::{
    RawCharacter, RawClientText, RawEquipment, RawInventory, RawLifecycle, RawLocation,
    RawModifiers, RawPaneProgression, RawSkill, RawSkillbook, RawSpell, RawSpellbook,
};
use darpc_model::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CooldownStatus, Element, EquipmentItem, EquipmentSlot, Gender, InventoryItem, MapLocation,
    Skill, Spell, SpellTargetType,
};

const SPRITE_ID_MASK: u16 = 0x3FFF;

pub(super) fn snapshot(ready: ReadyPublication) -> ClientSnapshot {
    ClientSnapshot {
        revision: ready.revision,
        captured_tick_ms: ready.captured_tick_ms,
        capture_duration_us: ready.capture_duration_us,
        world_generation: ready.world_generation,
        lifecycle: lifecycle(ready.raw.lifecycle),
        character: ready.raw.character.map(|character| {
            character_snapshot(character, ready.raw.world_token, ready.captured_tick_ms)
        }),
    }
}

fn character_snapshot(raw: RawCharacter, world_token: u32, tick_ms: u32) -> CharacterSnapshot {
    CharacterSnapshot {
        id: raw.id,
        name: client_text::decode(&raw.name[..usize::from(raw.name_len)]),
        appearance: raw.appearance.map(|appearance| CharacterAppearance {
            gender: Gender::from_raw(appearance.gender),
            hair_style: appearance.hair_style,
            hair_color: appearance.hair_color,
            body_sprite: appearance.body_sprite,
        }),
        class: CharacterClass::from_raw(raw.class),
        action_locked: raw.action_locked,
        is_blinded: raw.is_blinded,
        gold: raw.gold,
        progression: progression(&raw, raw.pane_progression),
        stats: CharacterStats {
            strength: raw.strength,
            intelligence: raw.intelligence,
            wisdom: raw.wisdom,
            constitution: raw.constitution,
            dexterity: raw.dexterity,
        },
        vitals: CharacterVitals {
            health: raw.health,
            max_health: raw.max_health,
            mana: raw.mana,
            max_mana: raw.max_mana,
        },
        modifiers: raw.modifiers.map(modifiers),
        location: raw
            .location
            .map(|location| map_location(location, world_token)),
        inventory: raw.inventory.map(inventory),
        equipment: raw.equipment.map(equipment),
        spellbook: raw.spellbook.map(spellbook),
        skillbook: raw.skillbook.map(|book| skillbook(book, tick_ms)),
    }
}

fn progression(raw: &RawCharacter, pane: Option<RawPaneProgression>) -> CharacterProgression {
    CharacterProgression {
        level: raw.level,
        ability_level: raw.ability_level,
        experience: raw.experience,
        ability_points: pane.map(|value| value.ability_points),
        experience_to_next_level: pane.map(|value| value.experience_to_next_level),
        ability_to_next_level: pane.map(|value| value.ability_to_next_level),
    }
}

fn modifiers(raw: RawModifiers) -> CharacterModifiers {
    CharacterModifiers {
        armor_class: raw.armor_class,
        damage: raw.damage,
        hit: raw.hit,
        magic_resistance: raw.magic_resistance_units.saturating_mul(10),
        attack_element: Element::from_raw(raw.attack_element),
        defense_element: Element::from_raw(raw.defense_element),
    }
}

fn map_location(raw: RawLocation, world_token: u32) -> MapLocation {
    let baseline_name = raw
        .name
        .and_then(|name| client_text::decode(&name.bytes[..usize::from(name.length)]));
    MapLocation {
        id: raw.map_id,
        name: map_name::read(world_token, raw.map_id).or(baseline_name),
        x: raw.x,
        y: raw.y,
        width: raw.width,
        height: raw.height,
    }
}

fn inventory(raw: RawInventory) -> Vec<InventoryItem> {
    raw.items
        .into_iter()
        .flatten()
        .map(|item| InventoryItem {
            slot: item.slot,
            sprite: item.sprite & SPRITE_ID_MASK,
            dye_color: item.dye_color,
            name: inventory_name(item.name, item.can_stack, item.quantity),
            quantity: item.quantity.max(1),
            can_stack: item.can_stack,
            durability: item.durability,
            max_durability: item.max_durability,
        })
        .collect()
}

fn equipment(raw: RawEquipment) -> Vec<EquipmentItem> {
    raw.items
        .into_iter()
        .flatten()
        .map(|item| EquipmentItem {
            slot: EquipmentSlot::from_raw(item.slot).expect("captured equipment slot is valid"),
            sprite: item.sprite & SPRITE_ID_MASK,
            dye_color: item.dye_color,
            name: text(item.name),
            durability: item.durability,
            max_durability: item.max_durability,
        })
        .collect()
}

fn spellbook(raw: RawSpellbook) -> Vec<Spell> {
    raw.spells.into_iter().flatten().map(spell).collect()
}

fn spell(raw: RawSpell) -> Spell {
    let (name, level, max_level) =
        ability_name(raw.name, raw.name_suffix_left, raw.base_name_length);
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
            remaining_ms: None,
        },
    }
}

fn skillbook(raw: RawSkillbook, tick_ms: u32) -> Vec<Skill> {
    raw.skills
        .into_iter()
        .flatten()
        .map(|raw_skill| skill(raw_skill, tick_ms))
        .collect()
}

fn skill(raw: RawSkill, tick_ms: u32) -> Skill {
    let (name, level, max_level) =
        ability_name(raw.name, raw.name_suffix_left, raw.base_name_length);
    let active = raw.cooldown_visual_active || raw.action_delay_active;
    Skill {
        slot: raw.slot,
        icon: raw.icon,
        name,
        level,
        max_level,
        cooldown: CooldownStatus {
            active,
            remaining_ms: raw
                .cooldown_visual_active
                .then(|| raw.cooldown_ends_at.wrapping_sub(tick_ms))
                .filter(|remaining| *remaining <= i32::MAX as u32),
        },
    }
}

fn ability_name(
    raw: RawClientText<128>,
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
    let level = u8::try_from(suffix_left).unwrap_or(0);
    (
        (!base_name.is_empty()).then(|| base_name.to_owned()),
        level,
        0,
    )
}

fn text<const N: usize>(raw: RawClientText<N>) -> Option<String> {
    client_text::decode(&raw.bytes[..usize::from(raw.length)])
}

fn inventory_name(raw: RawClientText<128>, can_stack: bool, quantity: u32) -> Option<String> {
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

fn ascii_text<const N: usize>(raw: RawClientText<N>) -> Option<String> {
    let bytes = raw.bytes[..usize::from(raw.length)]
        .iter()
        .copied()
        .filter(u8::is_ascii)
        .collect::<Vec<_>>();
    let text = String::from_utf8(bytes).expect("filtered bytes are ASCII");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn lifecycle(raw: RawLifecycle) -> ClientLifecycle {
    match raw {
        RawLifecycle::Unknown => ClientLifecycle::Unknown,
        RawLifecycle::Title => ClientLifecycle::Title,
        RawLifecycle::Transition => ClientLifecycle::Transition,
        RawLifecycle::InGame => ClientLifecycle::InGame,
        RawLifecycle::Disconnected => ClientLifecycle::Disconnected,
    }
}

#[cfg(test)]
mod tests {
    use super::{ascii_text, inventory_name};
    use darpc_game_client::RawClientText;

    #[test]
    fn canonicalizes_stack_names_and_ascii_prompts() {
        assert_eq!(
            inventory_name(raw_text(b"Dark Belt [ 3 ]"), true, 3).as_deref(),
            Some("Dark Belt")
        );
        assert_eq!(
            inventory_name(raw_text(b"Dark Belt [ 3 ]"), false, 3).as_deref(),
            Some("Dark Belt [ 3 ]")
        );
        assert_eq!(
            ascii_text(raw_text(b"Target \xFFname?")).as_deref(),
            Some("Target name?")
        );
    }

    fn raw_text(value: &[u8]) -> RawClientText<128> {
        let mut bytes = [0; 128];
        bytes[..value.len()].copy_from_slice(value);
        RawClientText {
            bytes,
            length: u8::try_from(value.len()).unwrap(),
        }
    }
}
