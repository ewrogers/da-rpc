use darpc_model::{
    CharacterClass, CharacterSnapshot, ClientLifecycle, ClientSnapshot, Effect, EffectDuration,
    Element, EquipmentItem, Gender, InventoryItem, Skill, Spell, SpellTargetType,
};
use serde_json::json;
use std::fmt::Write as _;

use crate::output::json_string;

pub(crate) fn render_human(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    snapshot: &ClientSnapshot,
) -> String {
    let mut output = format!(
        concat!(
            "snapshot succeeded: pid={} request_id={} ",
            "round_trip_ms={} revision={} captured_tick_ms={} ",
            "capture_duration_us={} world_generation={} lifecycle={}"
        ),
        pid,
        request_id,
        round_trip_ms,
        snapshot.revision,
        snapshot.captured_tick_ms,
        snapshot.capture_duration_us,
        snapshot.world_generation,
        lifecycle(snapshot.lifecycle),
    );
    let Some(character) = &snapshot.character else {
        output.push_str("\ncharacter: unavailable");
        return output;
    };
    let id = character
        .id
        .map_or_else(|| "unavailable".into(), |value| value.to_string());
    let name = character.name.as_deref().unwrap_or("unavailable");
    let appearance = character.appearance;
    let gender = appearance.map_or("unavailable", |value| gender(value.gender));
    let _ = write!(
        output,
        concat!(
            "\ncharacter: id={} name={} gender={} class={} is_action_restricted={} ",
            "is_blinded={} is_walking={} gold={} weight={} max_weight={} hair_style={} hair_color={} body_sprite={}"
        ),
        id,
        json_string(name),
        gender,
        character_class(character.class),
        character.is_action_restricted,
        character.is_blinded,
        character.is_walking,
        character.gold,
        character.weight,
        character.max_weight,
        optional_number(appearance.map(|value| value.hair_style)),
        optional_number(appearance.map(|value| value.hair_color)),
        optional_number(appearance.map(|value| value.body_sprite)),
    );
    let progression = &character.progression;
    let _ = write!(
        output,
        concat!(
            "\nprogression: level={} ability_level={} experience={} ability_points={} ",
            "experience_to_next_level={} ability_to_next_level={}"
        ),
        progression.level,
        progression.ability_level,
        progression.experience,
        optional_number(progression.ability_points),
        optional_number(progression.experience_to_next_level),
        optional_number(progression.ability_to_next_level),
    );
    let stats = character.stats;
    let _ = write!(
        output,
        "\nstats: strength={} intelligence={} wisdom={} constitution={} dexterity={}",
        stats.strength, stats.intelligence, stats.wisdom, stats.constitution, stats.dexterity,
    );
    let vitals = character.vitals;
    let _ = write!(
        output,
        "\nvitals: health={} max_health={} mana={} max_mana={}",
        vitals.health, vitals.max_health, vitals.mana, vitals.max_mana,
    );
    if let Some(modifiers) = character.modifiers {
        let _ = write!(
            output,
            concat!(
                "\nmodifiers: armor_class={} damage={} hit={} magic_resistance={} ",
                "attack_element={} defense_element={}"
            ),
            modifiers.armor_class,
            modifiers.damage,
            modifiers.hit,
            modifiers.magic_resistance,
            element(modifiers.attack_element),
            element(modifiers.defense_element),
        );
    } else {
        output.push_str("\nmodifiers: unavailable");
    }
    if let Some(location) = &character.location {
        let _ = write!(
            output,
            "\nlocation: id={} name={} x={} y={} width={} height={}",
            location.id,
            location.name.as_deref().unwrap_or("unavailable"),
            optional_number(location.x),
            optional_number(location.y),
            location.width,
            location.height,
        );
    } else {
        output.push_str("\nlocation: unavailable");
    }
    render_collections(&mut output, character);
    crate::object_output::render_human(&mut output, snapshot.objects.as_deref());
    output
}

pub(crate) fn render_json(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    snapshot: &ClientSnapshot,
) -> String {
    json!({
        "ok": true,
        "command": "snapshot",
        "pid": pid,
        "request_id": request_id,
        "round_trip_ms": round_trip_ms,
        "snapshot": snapshot_value(snapshot),
    })
    .to_string()
}

fn snapshot_value(snapshot: &ClientSnapshot) -> serde_json::Value {
    json!({
        "revision": snapshot.revision,
        "captured_tick_ms": snapshot.captured_tick_ms,
        "capture_duration_us": snapshot.capture_duration_us,
        "world_generation": snapshot.world_generation,
        "lifecycle": lifecycle(snapshot.lifecycle),
        "character": snapshot.character.as_ref().map(character_value),
        "objects": snapshot.objects.as_ref().map(|objects| {
            objects.iter().map(crate::object_output::json_value).collect::<Vec<_>>()
        }),
    })
}

fn character_value(character: &CharacterSnapshot) -> serde_json::Value {
    let progression = character.progression;
    let stats = character.stats;
    let vitals = character.vitals;
    let appearance = character.appearance;
    json!({
        "id": character.id,
        "name": character.name,
        "gender": appearance.map(|value| gender(value.gender)),
        "hair_style": appearance.map(|value| value.hair_style),
        "hair_color": appearance.map(|value| value.hair_color),
        "body_sprite": appearance.map(|value| value.body_sprite),
        "class": character_class(character.class),
        "is_action_restricted": character.is_action_restricted,
        "is_blinded": character.is_blinded,
        "is_walking": character.is_walking,
        "gold": character.gold,
        "weight": character.weight,
        "max_weight": character.max_weight,
        "progression": {
            "level": progression.level,
            "ability_level": progression.ability_level,
            "experience": progression.experience,
            "ability_points": progression.ability_points,
            "experience_to_next_level": progression.experience_to_next_level,
            "ability_to_next_level": progression.ability_to_next_level,
        },
        "stats": {
            "strength": stats.strength,
            "intelligence": stats.intelligence,
            "wisdom": stats.wisdom,
            "constitution": stats.constitution,
            "dexterity": stats.dexterity,
        },
        "vitals": {
            "health": vitals.health,
            "max_health": vitals.max_health,
            "mana": vitals.mana,
            "max_mana": vitals.max_mana,
        },
        "modifiers": character.modifiers.map(|value| json!({
            "armor_class": value.armor_class,
            "damage": value.damage,
            "hit": value.hit,
            "magic_resistance": value.magic_resistance,
            "attack_element": element(value.attack_element),
            "defense_element": element(value.defense_element),
        })),
        "location": character.location.as_ref().map(|value| json!({
            "id": value.id,
            "name": value.name,
            "x": value.x,
            "y": value.y,
            "width": value.width,
            "height": value.height,
        })),
        "inventory": character.inventory.as_ref().map(|items| {
            items.iter().map(inventory_value).collect::<Vec<_>>()
        }),
        "equipment": character.equipment.as_ref().map(|items| {
            items.iter().map(equipment_value).collect::<Vec<_>>()
        }),
        "spellbook": character.spellbook.as_ref().map(|spells| {
            spells.iter().map(spell_value).collect::<Vec<_>>()
        }),
        "skillbook": character.skillbook.as_ref().map(|skills| {
            skills.iter().map(skill_value).collect::<Vec<_>>()
        }),
        "effects": character.effects.as_ref().map(|effects| {
            effects.iter().map(effect_value).collect::<Vec<_>>()
        }),
    })
}

fn lifecycle(value: ClientLifecycle) -> &'static str {
    match value {
        ClientLifecycle::Unknown => "unknown",
        ClientLifecycle::Title => "title",
        ClientLifecycle::Transition => "transition",
        ClientLifecycle::InGame => "in_game",
        ClientLifecycle::Disconnected => "disconnected",
    }
}

fn gender(value: Gender) -> &'static str {
    match value {
        Gender::Male => "male",
        Gender::Female => "female",
        Gender::Unknown(_) => "unknown",
    }
}

fn character_class(value: CharacterClass) -> &'static str {
    match value {
        CharacterClass::Peasant => "peasant",
        CharacterClass::Warrior => "warrior",
        CharacterClass::Rogue => "rogue",
        CharacterClass::Wizard => "wizard",
        CharacterClass::Priest => "priest",
        CharacterClass::Monk => "monk",
        CharacterClass::Unknown(_) => "unknown",
    }
}

fn element(value: Element) -> &'static str {
    match value {
        Element::None => "none",
        Element::Fire => "fire",
        Element::Water => "water",
        Element::Wind => "wind",
        Element::Earth => "earth",
        Element::Light => "light",
        Element::Dark => "dark",
        Element::Wood => "wood",
        Element::Metal => "metal",
        Element::Undead => "undead",
        Element::Unknown(_) => "unknown",
    }
}

fn optional_number(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "unavailable".into(), |value| value.to_string())
}

fn render_collections(output: &mut String, character: &CharacterSnapshot) {
    render_inventory(output, character.inventory.as_deref());
    render_equipment(output, character.equipment.as_deref());
    render_spells(output, character.spellbook.as_deref());
    render_skills(output, character.skillbook.as_deref());
    render_effects(output, character.effects.as_deref());
}

fn inventory_value(item: &InventoryItem) -> serde_json::Value {
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

fn equipment_value(item: &EquipmentItem) -> serde_json::Value {
    json!({
        "slot": item.slot.as_str(),
        "sprite": item.sprite,
        "dye_color": item.dye_color,
        "name": item.name,
        "durability": item.durability,
        "max_durability": item.max_durability,
    })
}

fn spell_value(spell: &Spell) -> serde_json::Value {
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

fn skill_value(skill: &Skill) -> serde_json::Value {
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

fn effect_value(effect: &Effect) -> serde_json::Value {
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
