use darpc_model::{
    CharacterClass, CharacterSnapshot, ClientLifecycle, ClientSnapshot, Element, Gender,
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
            "ipc snapshot succeeded: pid={} request_id={} ",
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
    let gender = character.gender.map_or("unavailable", gender);
    let _ = write!(
        output,
        "\ncharacter: id={id} name={} gender={gender} class={} gold={}",
        json_string(name),
        character_class(character.class),
        character.gold,
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
        optional_u32(progression.ability_points),
        optional_u32(progression.experience_to_next_level),
        optional_u32(progression.ability_to_next_level),
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
            optional_i32(location.x),
            optional_i32(location.y),
            location.width,
            location.height,
        );
    } else {
        output.push_str("\nlocation: unavailable");
    }
    collections::render_human(&mut output, character);
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
        "command": "ipc.snapshot",
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
    })
}

fn character_value(character: &CharacterSnapshot) -> serde_json::Value {
    let progression = character.progression;
    let stats = character.stats;
    let vitals = character.vitals;
    json!({
        "id": character.id,
        "name": character.name,
        "gender": character.gender.map(gender),
        "gender_id": character.gender.map(Gender::raw),
        "class": character_class(character.class),
        "class_id": character.class.raw(),
        "gold": character.gold,
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
            items.iter().map(collections::inventory_value).collect::<Vec<_>>()
        }),
        "equipment": character.equipment.as_ref().map(|items| {
            items.iter().map(collections::equipment_value).collect::<Vec<_>>()
        }),
        "spellbook": character.spellbook.as_ref().map(|spells| {
            spells.iter().map(collections::spell_value).collect::<Vec<_>>()
        }),
        "skillbook": character.skillbook.as_ref().map(|skills| {
            skills.iter().map(collections::skill_value).collect::<Vec<_>>()
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

fn optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "unavailable".into(), |value| value.to_string())
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "unavailable".into(), |value| value.to_string())
}
mod collections;
