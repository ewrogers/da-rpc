use darpc_model::{
    CharacterClass, CharacterSnapshot, ClientLifecycle, ClientSnapshot, DialogInteraction,
    DialogKind, DialogSlot, DialogSpriteType, DialogState, Effect, EffectDuration, Element,
    EquipmentItem, FieldMapState, Gender, GroupState, InventoryItem, MessageDialogsState, Skill,
    Spell, SpellTargetType,
};
use serde_json::json;
use std::fmt::Write as _;

use crate::output::json_string;

mod collections;
mod dialog;
mod exchange;

use collections::*;
use dialog::*;
use exchange::*;

pub(crate) fn render_human(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    snapshot: &ClientSnapshot,
) -> String {
    let mut output = format!(
        concat!(
            "snapshot succeeded: pid={} request_id={} ",
            "round_trip_ms={} revision={} event_sequence={} captured_tick_ms={} ",
            "updated_tick_ms={} capture_duration_us={} world_generation={} lifecycle={}"
        ),
        pid,
        request_id,
        round_trip_ms,
        snapshot.revision,
        snapshot.event_sequence,
        snapshot.captured_tick_ms,
        snapshot.updated_tick_ms,
        snapshot.capture_duration_us,
        snapshot.world_generation,
        lifecycle(snapshot.lifecycle),
    );
    render_planned_route(&mut output, snapshot.planned_route.as_ref());
    let Some(character) = &snapshot.character else {
        output.push_str("\ncharacter: unavailable");
        render_group(&mut output, snapshot.group.as_ref());
        render_dialog(&mut output, snapshot.dialog.as_ref());
        render_field_map(&mut output, snapshot.active_field_map.as_ref());
        render_message_dialogs(&mut output, &snapshot.message_dialogs);
        render_exchange(&mut output, snapshot.exchange.as_ref());
        crate::object_output::render_human(&mut output, snapshot.objects.as_deref());
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
            "\ncharacter: id={} name={} gender={} class={} is_hidden={} is_action_restricted={} ",
            "is_blinded={} is_walking={} is_casting={} gold={} weight={} max_weight={} hair_style={} hair_color={} body_sprite={}"
        ),
        id,
        json_string(name),
        gender,
        character_class(character.class),
        character.is_hidden,
        character.is_action_restricted,
        character.is_blinded,
        character.is_walking,
        character.is_casting,
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
        "\nstats: stat_points={} strength={} intelligence={} wisdom={} constitution={} dexterity={}",
        stats.stat_points,
        stats.strength,
        stats.intelligence,
        stats.wisdom,
        stats.constitution,
        stats.dexterity,
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
    render_group(&mut output, snapshot.group.as_ref());
    render_dialog(&mut output, snapshot.dialog.as_ref());
    render_field_map(&mut output, snapshot.active_field_map.as_ref());
    render_message_dialogs(&mut output, &snapshot.message_dialogs);
    render_exchange(&mut output, snapshot.exchange.as_ref());
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
        "event_sequence": snapshot.event_sequence,
        "captured_tick_ms": snapshot.captured_tick_ms,
        "updated_tick_ms": snapshot.updated_tick_ms,
        "capture_duration_us": snapshot.capture_duration_us,
        "world_generation": snapshot.world_generation,
        "lifecycle": lifecycle(snapshot.lifecycle),
        "character": snapshot.character.as_ref().map(character_value),
        "objects": snapshot.objects.as_ref().map(|objects| {
            objects.iter().map(crate::object_output::json_value).collect::<Vec<_>>()
        }),
        "dialog": snapshot.dialog.as_ref().map(dialog_value),
        "active_field_map": snapshot.active_field_map.as_ref().map(field_map_value),
        "message_dialogs": {
            "revision": snapshot.message_dialogs.revision,
            "dialogs": snapshot.message_dialogs.dialogs.iter().map(|dialog| json!({
                "id": dialog.id,
                "text": dialog.text,
                "truncated": dialog.truncated,
            })).collect::<Vec<_>>(),
        },
        "group": snapshot.group.as_ref().map(group_value),
        "exchange": snapshot.exchange.as_ref().map(exchange_value),
        "planned_route": snapshot.planned_route.as_ref().map(|route| json!({
            "generation": route.generation,
            "tiles": route.tiles.iter().map(|tile| json!({
                "x": tile.x,
                "y": tile.y,
            })).collect::<Vec<_>>(),
        })),
    })
}

fn render_message_dialogs(output: &mut String, state: &MessageDialogsState) {
    let _ = write!(
        output,
        "\nmessage_dialogs: revision={} dialogs={}",
        state.revision,
        state.dialogs.len(),
    );
    for dialog in &state.dialogs {
        let text = dialog
            .text
            .as_deref()
            .map_or_else(|| "unavailable".into(), json_string);
        let _ = write!(
            output,
            "\nmessage_dialog: id={} text={} truncated={}",
            dialog.id, text, dialog.truncated,
        );
    }
}

fn render_field_map(output: &mut String, field_map: Option<&FieldMapState>) {
    let Some(field_map) = field_map else {
        output.push_str("\nfield_map: unavailable");
        return;
    };
    let selection = field_map.selection.map_or_else(
        || "none".into(),
        |value| value.destination_index.to_string(),
    );
    let _ = write!(
        output,
        "\nfield_map: revision={} field_name={} current_node_index={} destinations={} selection={}",
        field_map.revision,
        json_string(&field_map.field_name),
        optional_number(field_map.current_node_index),
        field_map.destinations.len(),
        selection,
    );
    for destination in &field_map.destinations {
        let _ = write!(
            output,
            "\nfield_map_destination: index={} screen_x={} screen_y={} name={} checksum={} map_id={} x={} y={}",
            destination.index,
            destination.screen_x,
            destination.screen_y,
            json_string(&destination.name),
            destination.checksum,
            destination.map_id,
            destination.map_x,
            destination.map_y,
        );
    }
}

fn field_map_value(field_map: &FieldMapState) -> serde_json::Value {
    json!({
        "revision": field_map.revision,
        "field_name": field_map.field_name,
        "current_node_index": field_map.current_node_index,
        "destinations": field_map.destinations.iter().map(|destination| json!({
            "index": destination.index,
            "screen_x": destination.screen_x,
            "screen_y": destination.screen_y,
            "name": destination.name,
            "checksum": destination.checksum,
            "map_id": destination.map_id,
            "map_x": destination.map_x,
            "map_y": destination.map_y,
        })).collect::<Vec<_>>(),
        "selection": field_map.selection.map(|selection| json!({
            "destination_index": selection.destination_index,
        })),
    })
}

fn render_planned_route(output: &mut String, route: Option<&darpc_model::PlannedRoute>) {
    let Some(route) = route else {
        output.push_str("\nplanned_route: unavailable");
        return;
    };
    let _ = write!(
        output,
        "\nplanned_route: generation={} tiles={}",
        route.generation,
        route.tiles.len()
    );
    for (index, tile) in route.tiles.iter().enumerate() {
        let _ = write!(
            output,
            "\nplanned_route_tile: index={} x={} y={}",
            index, tile.x, tile.y
        );
    }
}

fn group_value(group: &GroupState) -> serde_json::Value {
    json!({
        "members": group.members.iter().map(|member| json!({
            "name": member.name,
            "is_leader": member.is_leader,
        })).collect::<Vec<_>>(),
        "invitations": group.invitations.iter().map(|invitation| json!({
            "id": invitation.id,
            "inviter": invitation.inviter,
            "received_tick_ms": invitation.received_tick_ms,
        })).collect::<Vec<_>>(),
        "is_group_open": group.is_group_open,
        "auto_accept": group.auto_accept,
    })
}

fn render_group(output: &mut String, group: Option<&GroupState>) {
    let Some(group) = group else {
        output.push_str("\ngroup: unavailable");
        return;
    };
    let _ = write!(
        output,
        "\ngroup: members={} invitations={} is_group_open={} auto_accept={}",
        group.members.len(),
        group.invitations.len(),
        optional_value(group.is_group_open),
        optional_value(group.auto_accept),
    );
    for member in &group.members {
        let _ = write!(
            output,
            "\ngroup_member: name={} is_leader={}",
            member.name, member.is_leader
        );
    }
    for invitation in &group.invitations {
        let _ = write!(
            output,
            "\ngroup_invitation: id={} inviter={} received_tick_ms={}",
            invitation.id,
            invitation.inviter,
            optional_number(invitation.received_tick_ms),
        );
    }
}

fn optional_value(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unavailable",
    }
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
        "is_hidden": character.is_hidden,
        "is_action_restricted": character.is_action_restricted,
        "is_blinded": character.is_blinded,
        "is_walking": character.is_walking,
        "is_casting": character.is_casting,
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
            "stat_points": stats.stat_points,
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

#[cfg(test)]
mod tests;
