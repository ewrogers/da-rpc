use super::*;

pub(super) fn dialog_value(dialog: &DialogState) -> serde_json::Value {
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

pub(super) fn render_dialog(output: &mut String, dialog: Option<&DialogState>) {
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
