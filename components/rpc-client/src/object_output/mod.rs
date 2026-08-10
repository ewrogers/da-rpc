use darpc_model::{CreatureKind, Direction, WorldObject};
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
                ..
            } => {
                let _ = write!(
                    output,
                    "\n  player id={id} name={} x={x} y={y} direction={}",
                    name.as_deref().unwrap_or("unavailable"),
                    direction_name(*direction),
                );
            }
            WorldObject::Creature {
                id,
                kind,
                sprite,
                name,
                x,
                y,
                direction,
            } => {
                let _ = write!(
                    output,
                    "\n  {} id={id} sprite={} name={} x={x} y={y} direction={}",
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
                x,
                y,
                z_index,
            } => {
                let _ = write!(
                    output,
                    "\n  item id={id} sprite={sprite} x={x} y={y} z_index={z_index}"
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
            ..
        } => json!({
            "kind": "player", "id": id, "name": name, "x": x, "y": y,
            "direction": direction_name(*direction),
        }),
        WorldObject::Creature {
            id,
            kind,
            sprite,
            name,
            x,
            y,
            direction,
        } => json!({
            "kind": match kind { CreatureKind::Monster => "monster", CreatureKind::Npc => "npc" },
            "id": id, "sprite": sprite, "name": name, "x": x, "y": y,
            "direction": direction_name(*direction),
        }),
        WorldObject::Item {
            id,
            sprite,
            x,
            y,
            z_index,
        } => json!({
            "kind": "item", "id": id, "sprite": sprite, "x": x, "y": y,
            "z_index": z_index,
        }),
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
