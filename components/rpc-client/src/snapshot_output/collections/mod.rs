use super::*;

pub(super) fn render_collections(output: &mut String, character: &CharacterSnapshot) {
    render_inventory(output, character.inventory.as_deref());
    render_equipment(output, character.equipment.as_deref());
    render_spells(output, character.spellbook.as_deref());
    render_skills(output, character.skillbook.as_deref());
    render_effects(output, character.effects.as_deref());
}

pub(super) fn inventory_value(item: &InventoryItem) -> serde_json::Value {
    json!({
        "slot": item.slot,
        "sprite": item.sprite,
        "dye_color": item.dye_color,
        "name": item.name,
        "quantity": item.quantity,
        "can_stack": item.can_stack,
        "durability": item.durability,
        "max_durability": item.max_durability,
    })
}

pub(super) fn equipment_value(item: &EquipmentItem) -> serde_json::Value {
    json!({
        "slot": item.slot.as_str(),
        "sprite": item.sprite,
        "dye_color": item.dye_color,
        "name": item.name,
        "durability": item.durability,
        "max_durability": item.max_durability,
    })
}

pub(super) fn spell_value(spell: &Spell) -> serde_json::Value {
    json!({
        "slot": spell.slot,
        "icon": spell.icon,
        "name": spell.name,
        "level": spell.level,
        "max_level": spell.max_level,
        "lines": spell.lines,
        "target_type": target_type(spell.target_type),
        "prompt": spell.prompt,
        "cooldown": {
            "active": spell.cooldown.active,
            "remaining_ms": spell.cooldown.remaining_ms,
        },
    })
}

pub(super) fn skill_value(skill: &Skill) -> serde_json::Value {
    json!({
        "slot": skill.slot,
        "icon": skill.icon,
        "name": skill.name,
        "level": skill.level,
        "max_level": skill.max_level,
        "cooldown": {
            "active": skill.cooldown.active,
            "remaining_ms": skill.cooldown.remaining_ms,
        },
    })
}

pub(super) fn effect_value(effect: &Effect) -> serde_json::Value {
    json!({
        "icon": effect.icon,
        "duration": effect_duration(effect.duration),
    })
}

fn render_inventory(output: &mut String, items: Option<&[InventoryItem]>) {
    let Some(items) = items else {
        output.push_str("\ninventory: unavailable");
        return;
    };
    let _ = write!(output, "\ninventory: {} occupied", items.len());
    for item in items {
        let name = item.name.as_deref().unwrap_or("unavailable");
        let _ = write!(
            output,
            concat!(
                "\n  slot={}: name={} sprite={} dye_color={} quantity={} can_stack={} ",
                "durability={}/{}"
            ),
            item.slot,
            json_string(name),
            item.sprite,
            item.dye_color,
            item.quantity,
            item.can_stack,
            item.durability,
            item.max_durability,
        );
    }
}

fn render_equipment(output: &mut String, items: Option<&[EquipmentItem]>) {
    let Some(items) = items else {
        output.push_str("\nequipment: unavailable");
        return;
    };
    let _ = write!(output, "\nequipment: {} occupied", items.len());
    for item in items {
        let name = item.name.as_deref().unwrap_or("unavailable");
        let _ = write!(
            output,
            "\n  slot={}: name={} sprite={} dye_color={} durability={}/{}",
            item.slot.as_str(),
            json_string(name),
            item.sprite,
            item.dye_color,
            item.durability,
            item.max_durability,
        );
    }
}

fn render_spells(output: &mut String, spells: Option<&[Spell]>) {
    let Some(spells) = spells else {
        output.push_str("\nspellbook: unavailable");
        return;
    };
    let _ = write!(output, "\nspellbook: {} occupied", spells.len());
    for spell in spells {
        let name = spell.name.as_deref().unwrap_or("unavailable");
        let _ = write!(
            output,
            concat!(
                "\n  slot={}: name={} icon={} level={}/{} lines={} target_type={} prompt={} ",
                "cooldown_active={} cooldown_remaining_ms={}"
            ),
            spell.slot,
            json_string(name),
            spell.icon,
            spell.level,
            spell.max_level,
            spell.lines,
            target_type(spell.target_type),
            json_string(spell.prompt.as_deref().unwrap_or("unavailable")),
            spell.cooldown.active,
            optional_ms(spell.cooldown.remaining_ms),
        );
    }
}

fn render_skills(output: &mut String, skills: Option<&[Skill]>) {
    let Some(skills) = skills else {
        output.push_str("\nskillbook: unavailable");
        return;
    };
    let _ = write!(output, "\nskillbook: {} occupied", skills.len());
    for skill in skills {
        let name = skill.name.as_deref().unwrap_or("unavailable");
        let _ = write!(
            output,
            concat!(
                "\n  slot={}: name={} icon={} level={}/{} cooldown_active={} ",
                "cooldown_remaining_ms={}"
            ),
            skill.slot,
            json_string(name),
            skill.icon,
            skill.level,
            skill.max_level,
            skill.cooldown.active,
            optional_ms(skill.cooldown.remaining_ms),
        );
    }
}

fn render_effects(output: &mut String, effects: Option<&[Effect]>) {
    let Some(effects) = effects else {
        output.push_str("\neffects: unavailable");
        return;
    };
    let _ = write!(output, "\neffects: {} active", effects.len());
    for effect in effects {
        let _ = write!(
            output,
            "\n  icon={} duration={}",
            effect.icon,
            effect_duration(effect.duration),
        );
    }
}

fn effect_duration(value: EffectDuration) -> &'static str {
    match value {
        EffectDuration::Blue => "blue",
        EffectDuration::Green => "green",
        EffectDuration::Yellow => "yellow",
        EffectDuration::Orange => "orange",
        EffectDuration::Red => "red",
        EffectDuration::White => "white",
    }
}

fn target_type(value: SpellTargetType) -> &'static str {
    match value {
        SpellTargetType::None => "none",
        SpellTargetType::TextInput => "text_input",
        SpellTargetType::Target => "target",
        SpellTargetType::Unknown(_) => "unknown",
    }
}

fn optional_ms(value: Option<u32>) -> String {
    value.map_or_else(|| "unavailable".into(), |value| value.to_string())
}
