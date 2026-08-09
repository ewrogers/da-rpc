use darpc_model::{
    CharacterClass, CharacterSnapshot, ClientLifecycle, ClientSnapshot, DialogInteraction,
    DialogKind, DialogSlot, DialogSpriteType, DialogState, Effect, EffectDuration, Element,
    EquipmentItem, Gender, GroupState, InventoryItem, Skill, Spell, SpellTargetType,
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
    let Some(character) = &snapshot.character else {
        output.push_str("\ncharacter: unavailable");
        render_group(&mut output, snapshot.group.as_ref());
        render_dialog(&mut output, snapshot.dialog.as_ref());
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
            "\ncharacter: id={} name={} gender={} class={} is_action_restricted={} ",
            "is_blinded={} is_walking={} is_casting={} gold={} weight={} max_weight={} hair_style={} hair_color={} body_sprite={}"
        ),
        id,
        json_string(name),
        gender,
        character_class(character.class),
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
    render_group(&mut output, snapshot.group.as_ref());
    render_dialog(&mut output, snapshot.dialog.as_ref());
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
        "group": snapshot.group.as_ref().map(group_value),
    })
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

fn dialog_value(dialog: &DialogState) -> serde_json::Value {
    json!({
        "revision": dialog.revision,
        "kind": dialog_kind(dialog.kind),
        "target": { "id": dialog.target.id },
        "speaker": {
            "name": dialog.speaker.name,
            "sprite": dialog.speaker.sprite,
            "sprite_type": dialog_sprite_type(dialog.speaker.sprite_type),
            "color": dialog.speaker.color,
            "show_graphic": dialog.speaker.show_graphic,
        },
        "content": dialog.content,
        "response_pending": dialog.response_pending,
        "navigation": {
            "previous": dialog.navigation.previous,
            "next": dialog.navigation.next,
            "close": dialog.navigation.close,
        },
        "interaction": dialog_interaction_value(&dialog.interaction),
    })
}

fn dialog_interaction_value(interaction: &DialogInteraction) -> serde_json::Value {
    match interaction {
        DialogInteraction::Message => json!({ "type": "message" }),
        DialogInteraction::Choices(choices) => json!({
            "type": "choices",
            "data": choices.iter().map(|choice| json!({
                "index": choice.index,
                "text": choice.text,
            })).collect::<Vec<_>>(),
        }),
        DialogInteraction::Input(input) => json!({
            "type": "input",
            "data": {
                "prolog": input.prolog,
                "maximum_bytes": input.maximum_bytes,
                "epilog": input.epilog,
            },
        }),
        DialogInteraction::Items(items) => json!({
            "type": "items",
            "data": items.iter().map(|item| json!({
                "index": item.index,
                "sprite": item.sprite,
                "color": item.color,
                "name": item.name,
                "description": item.description,
                "value": item.value,
                "available_quantity": item.available_quantity,
            })).collect::<Vec<_>>(),
        }),
        DialogInteraction::Inventory(slots) => dialog_slots_value("inventory", slots),
        DialogInteraction::Spells(slots) => dialog_slots_value("spells", slots),
        DialogInteraction::Skills(slots) => dialog_slots_value("skills", slots),
        DialogInteraction::Protected => json!({ "type": "protected" }),
        DialogInteraction::Unsupported => json!({ "type": "unsupported" }),
    }
}

fn dialog_slots_value(kind: &str, slots: &[DialogSlot]) -> serde_json::Value {
    json!({
        "type": kind,
        "data": slots.iter().map(|slot| json!({
            "index": slot.index,
            "slot": slot.slot,
            "value": slot.value,
            "name": slot.name,
            "sprite": slot.sprite,
            "color": slot.color,
        })).collect::<Vec<_>>(),
    })
}

fn render_dialog(output: &mut String, dialog: Option<&DialogState>) {
    let Some(dialog) = dialog else {
        output.push_str("\ndialog: none");
        return;
    };
    let _ = write!(
        output,
        concat!(
            "\ndialog: revision={} kind={} target_id={} speaker={} response_pending={} ",
            "previous={} next={} close={} content={}"
        ),
        dialog.revision,
        dialog_kind(dialog.kind),
        dialog.target.id,
        json_string(dialog.speaker.name.as_deref().unwrap_or("unavailable")),
        dialog.response_pending,
        dialog.navigation.previous,
        dialog.navigation.next,
        dialog.navigation.close,
        dialog
            .content
            .as_deref()
            .map_or_else(|| "none".into(), json_string),
    );
    match &dialog.interaction {
        DialogInteraction::Message => output.push_str("\ndialog interaction: message"),
        DialogInteraction::Choices(choices) => {
            output.push_str("\ndialog choices:\nINDEX\tTEXT");
            for choice in choices {
                let _ = write!(output, "\n{}\t{}", choice.index, json_string(&choice.text));
            }
        }
        DialogInteraction::Input(input) => {
            let _ = write!(
                output,
                "\ndialog input: maximum_bytes={} prolog={} epilog={}",
                input.maximum_bytes,
                input
                    .prolog
                    .as_deref()
                    .map_or_else(|| "none".into(), json_string),
                input
                    .epilog
                    .as_deref()
                    .map_or_else(|| "none".into(), json_string),
            );
        }
        DialogInteraction::Items(items) => {
            output.push_str("\ndialog items:\nINDEX\tNAME\tVALUE\tAVAILABLE");
            for item in items {
                let _ = write!(
                    output,
                    "\n{}\t{}\t{}\t{}",
                    item.index,
                    item.name.as_deref().unwrap_or("unavailable"),
                    optional_number(item.value),
                    optional_number(item.available_quantity),
                );
            }
        }
        DialogInteraction::Inventory(slots) => render_dialog_slots(output, "inventory", slots),
        DialogInteraction::Spells(slots) => render_dialog_slots(output, "spells", slots),
        DialogInteraction::Skills(slots) => render_dialog_slots(output, "skills", slots),
        DialogInteraction::Protected => output.push_str("\ndialog interaction: protected"),
        DialogInteraction::Unsupported => output.push_str("\ndialog interaction: unsupported"),
    }
}

fn render_dialog_slots(output: &mut String, kind: &str, slots: &[DialogSlot]) {
    let _ = write!(output, "\ndialog {kind}:\nINDEX\tSLOT\tNAME\tVALUE");
    for slot in slots {
        let _ = write!(
            output,
            "\n{}\t{}\t{}\t{}",
            slot.index,
            slot.slot,
            slot.name.as_deref().unwrap_or("unavailable"),
            optional_number(slot.value),
        );
    }
}

const fn dialog_kind(kind: DialogKind) -> &'static str {
    match kind {
        DialogKind::Merchant => "merchant",
        DialogKind::Pursuit => "pursuit",
    }
}

const fn dialog_sprite_type(sprite_type: DialogSpriteType) -> &'static str {
    match sprite_type {
        DialogSpriteType::Creature => "creature",
        DialogSpriteType::Item => "item",
        DialogSpriteType::Unknown => "unknown",
    }
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

#[cfg(test)]
mod tests {
    use super::{render_human, render_json};
    use darpc_model::{
        ClientLifecycle, ClientSnapshot, DialogChoice, DialogInteraction, DialogKind,
        DialogNavigation, DialogSpeaker, DialogSpriteType, DialogState, DialogTarget,
    };

    fn snapshot() -> ClientSnapshot {
        ClientSnapshot {
            revision: 3,
            event_sequence: 12,
            captured_tick_ms: 100,
            updated_tick_ms: 125,
            capture_duration_us: 40,
            world_generation: 2,
            lifecycle: ClientLifecycle::InGame,
            character: None,
            objects: None,
            dialog: Some(DialogState {
                revision: 7,
                kind: DialogKind::Pursuit,
                target: DialogTarget { id: 77 },
                speaker: DialogSpeaker {
                    name: Some("Innkeeper".into()),
                    sprite: 12,
                    sprite_type: DialogSpriteType::Creature,
                    color: 3,
                    show_graphic: true,
                },
                content: Some("Welcome".into()),
                response_pending: false,
                navigation: DialogNavigation {
                    previous: false,
                    next: true,
                    close: true,
                },
                interaction: DialogInteraction::Choices(vec![DialogChoice {
                    index: 0,
                    text: "Ask".into(),
                }]),
            }),
            group: None,
        }
    }

    #[test]
    fn snapshot_output_keeps_dialog_without_character_state() {
        let snapshot = snapshot();
        let human = render_human(42, 1, 2, &snapshot);
        assert!(human.contains("event_sequence=12"));
        assert!(human.contains("character: unavailable"));
        assert!(human.contains("dialog: revision=7"));
        assert!(human.contains("0\t\"Ask\""));

        let json: serde_json::Value =
            serde_json::from_str(&render_json(42, 1, 2, &snapshot)).unwrap();
        assert_eq!(json["snapshot"]["event_sequence"], 12);
        assert_eq!(json["snapshot"]["updated_tick_ms"], 125);
        assert_eq!(json["snapshot"]["dialog"]["revision"], 7);
        assert_eq!(
            json["snapshot"]["dialog"]["interaction"],
            serde_json::json!({
                "type": "choices",
                "data": [{ "index": 0, "text": "Ask" }],
            })
        );
    }
}
