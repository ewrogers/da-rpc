use darpc_model::{CreatureKind, Direction, HumanVisual, PlayerVisual, WorldObject};
use serde_json::json;
use std::fmt::Write as _;

pub(crate) fn render_human(output: &mut String, objects: Option<&[WorldObject]>) {
    let Some(objects) = objects else {
        output.push_str("\nobjects: unavailable");
        return;
    };
    let _ = write!(output, "\nobjects: {} visible", objects.len());
    for object in objects {
        match object {
            WorldObject::Player {
                id,
                name,
                x,
                y,
                direction,
                is_hidden,
                visual,
                ..
            } => {
                let _ = write!(
                    output,
                    "\n  player id={id} name={} x={x} y={y} direction={} is_hidden={is_hidden} is_solid=true visual={visual:?}",
                    name.as_deref().unwrap_or("unavailable"),
                    direction_name(*direction),
                );
            }
            WorldObject::Creature {
                id,
                kind,
                is_solid,
                sprite,
                name,
                x,
                y,
                direction,
            } => {
                let _ = write!(
                    output,
                    "\n  {} id={id} sprite={} name={} x={x} y={y} direction={} is_solid={is_solid}",
                    match kind {
                        CreatureKind::Monster => "monster",
                        CreatureKind::Npc => "npc",
                    },
                    sprite.map_or_else(|| "unavailable".into(), |value| value.to_string()),
                    name.as_deref().unwrap_or("unavailable"),
                    direction_name(*direction),
                );
            }
            WorldObject::Item {
                id,
                sprite,
                dye_color,
                x,
                y,
                z_index,
            } => {
                let _ = write!(
                    output,
                    "\n  item id={id} sprite={sprite} dye_color={dye_color} x={x} y={y} z_index={z_index} is_solid=false"
                );
            }
        }
    }
}

pub(crate) fn json_value(object: &WorldObject) -> serde_json::Value {
    match object {
        WorldObject::Player {
            id,
            name,
            x,
            y,
            direction,
            is_hidden,
            visual,
            ..
        } => json!({
            "kind": "player", "id": id, "name": name, "x": x, "y": y,
            "direction": direction_name(*direction), "is_hidden": is_hidden,
            "is_solid": true,
            "visual": visual.as_ref().map(visual_json),
        }),
        WorldObject::Creature {
            id,
            kind,
            is_solid,
            sprite,
            name,
            x,
            y,
            direction,
        } => json!({
            "kind": match kind { CreatureKind::Monster => "monster", CreatureKind::Npc => "npc" },
            "id": id, "sprite": sprite, "name": name, "x": x, "y": y,
            "direction": direction_name(*direction), "is_solid": is_solid,
        }),
        WorldObject::Item {
            id,
            sprite,
            dye_color,
            x,
            y,
            z_index,
        } => json!({
            "kind": "item", "id": id, "sprite": sprite, "dye_color": dye_color,
            "x": x, "y": y, "z_index": z_index, "is_solid": false,
        }),
    }
}

fn visual_json(visual: &PlayerVisual) -> serde_json::Value {
    match visual {
        PlayerVisual::Human(HumanVisual {
            gender,
            head_sprite,
            body_sprite,
            arms_sprite,
            boots_sprite,
            pants_sprite,
            armor_sprite,
            weapon_sprite,
            shield_sprite,
            overcoat_sprite,
            accessory1_sprite,
            accessory2_sprite,
            accessory3_sprite,
            hair_color,
            skin_color,
            boots_color,
            pants_color,
            overcoat_color,
            accessory1_color,
            accessory2_color,
            accessory3_color,
            rest_position,
            face_shape,
            is_translucent,
        }) => json!({
            "form": "human", "gender": gender_name(*gender),
            "head_sprite": head_sprite, "body_sprite": body_sprite,
            "arms_sprite": arms_sprite, "boots_sprite": boots_sprite,
            "pants_sprite": pants_sprite, "armor_sprite": armor_sprite,
            "weapon_sprite": weapon_sprite, "shield_sprite": shield_sprite,
            "overcoat_sprite": overcoat_sprite, "accessory1_sprite": accessory1_sprite,
            "accessory2_sprite": accessory2_sprite, "accessory3_sprite": accessory3_sprite,
            "hair_color": hair_color, "skin_color": skin_color,
            "boots_color": boots_color, "pants_color": pants_color,
            "overcoat_color": overcoat_color, "accessory1_color": accessory1_color,
            "accessory2_color": accessory2_color, "accessory3_color": accessory3_color,
            "rest_position": rest_position, "face_shape": face_shape,
            "is_translucent": is_translucent,
        }),
        PlayerVisual::Creature {
            sprite,
            color,
            boots_color,
            pants_color,
        } => json!({
            "form": "creature", "sprite": sprite, "color": color,
            "boots_color": boots_color, "pants_color": pants_color,
        }),
    }
}

const fn gender_name(gender: darpc_model::Gender) -> &'static str {
    match gender {
        darpc_model::Gender::Male => "male",
        darpc_model::Gender::Female => "female",
        darpc_model::Gender::Unknown(_) => "unknown",
    }
}

const fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::North => "north",
        Direction::East => "east",
        Direction::South => "south",
        Direction::West => "west",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground_item() -> WorldObject {
        WorldObject::Item {
            id: 7,
            sprite: 321,
            dye_color: 5,
            x: 10,
            y: 20,
            z_index: 2,
        }
    }

    #[test]
    fn ground_item_output_includes_dye_color() {
        let item = ground_item();
        let mut output = String::new();
        render_human(&mut output, Some(core::slice::from_ref(&item)));
        assert!(output.contains("dye_color=5"));
        assert_eq!(json_value(&item)["dye_color"], 5);
    }
}
