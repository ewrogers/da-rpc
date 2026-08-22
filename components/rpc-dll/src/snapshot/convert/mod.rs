use super::publication::ReadyPublication;
use crate::{client_text, collections, map_name};
use darpc_game_client::{
    RawCharacter, RawClientText, RawEffects, RawEquipment, RawInventory, RawLifecycle, RawLocation,
    RawModifiers, RawObjects, RawPaneProgression, RawSkillbook, RawSpellbook, RawWorldObject,
};
use darpc_model::{
    CharacterAppearance, CharacterClass, CharacterModifiers, CharacterProgression,
    CharacterSnapshot, CharacterStats, CharacterVitals, ClientLifecycle, ClientSnapshot,
    CreatureKind, Direction, Effect, EffectDuration, Element, EquipmentItem, EquipmentSlot, Gender,
    InventoryItem, MapLocation, Skill, Spell, WorldObject,
};

const SPRITE_ID_MASK: u16 = 0x3FFF;

pub(super) fn snapshot(
    ready: ReadyPublication,
    raw: &darpc_game_client::RawStateSnapshot,
    raw_objects: &RawObjects,
    retained: RetainedState<'_>,
) -> ClientSnapshot {
    ClientSnapshot {
        revision: ready.revision,
        event_sequence: ready.event_sequence,
        captured_tick_ms: ready.captured_tick_ms,
        updated_tick_ms: ready.updated_tick_ms,
        capture_duration_us: ready.capture_duration_us,
        world_generation: ready.world_generation,
        lifecycle: lifecycle(raw.lifecycle),
        character: raw
            .character_available
            .then(|| character_snapshot(&raw.character, raw.world_token, ready.captured_tick_ms)),
        objects: matches!(
            raw.lifecycle,
            RawLifecycle::InGame | RawLifecycle::Disconnected
        )
        .then(|| objects(raw_objects)),
        dialog: crate::dialog::decode_current(retained.dialog),
        active_field_map: crate::field_map::decode_current(retained.field_map),
        message_dialogs: crate::message_dialog::decode_current(retained.message_dialogs),
        group: raw
            .group_available
            .then(|| crate::group::model_state(&raw.group)),
        exchange: crate::exchange::decode_current(retained.exchange),
        legend: Some(crate::legend::model_state(retained.legend)),
        planned_route: crate::route::model(retained.route),
    }
}

pub(super) struct RetainedState<'a> {
    pub(super) dialog: crate::dialog::RawDialog,
    pub(super) field_map: &'a crate::field_map::RawFieldMap,
    pub(super) message_dialogs: &'a crate::message_dialog::RawMessageDialogs,
    pub(super) exchange: crate::exchange::RawExchange,
    pub(super) legend: &'a crate::legend::RawLegendState,
    pub(super) route: &'a crate::route::RawRoute,
}

fn objects(raw: &RawObjects) -> Vec<WorldObject> {
    let mut objects = raw
        .entries
        .iter()
        .copied()
        .take(usize::from(raw.count))
        .flatten()
        .map(|object| match object {
            RawWorldObject::Player {
                id,
                name,
                name_len,
                x,
                y,
                direction,
                is_hidden,
                visual,
            } => WorldObject::Player {
                id,
                name: client_text::decode(&name[..usize::from(name_len)]),
                x,
                y,
                direction: Direction::from_raw(direction)
                    .expect("captured player direction is valid"),
                is_hidden,
                visual: visual.map(crate::objects::visual_model),
                profile: crate::player::profile(id).map(Box::new),
            },
            RawWorldObject::Creature {
                id,
                is_npc,
                is_solid,
                sprite,
                name,
                name_len,
                x,
                y,
                direction,
            } => WorldObject::Creature {
                id,
                kind: if is_npc {
                    CreatureKind::Npc
                } else {
                    CreatureKind::Monster
                },
                is_solid,
                sprite,
                name: client_text::decode(&name[..usize::from(name_len)]),
                x,
                y,
                direction: Direction::from_raw(direction)
                    .expect("captured creature direction is valid"),
            },
            RawWorldObject::Item {
                id,
                sprite,
                dye_color,
                x,
                y,
                z_index,
            } => WorldObject::Item {
                id,
                sprite: sprite & SPRITE_ID_MASK,
                dye_color,
                x,
                y,
                z_index,
            },
        })
        .collect::<Vec<_>>();
    objects.sort_unstable_by_key(WorldObject::id);
    objects
}

fn character_snapshot(raw: &RawCharacter, world_token: u32, tick_ms: u32) -> CharacterSnapshot {
    CharacterSnapshot {
        id: raw.id,
        name: client_text::decode(&raw.name[..usize::from(raw.name_len)]),
        identity: crate::player::self_identity(),
        appearance: raw.appearance.map(|appearance| CharacterAppearance {
            gender: Gender::from_raw(appearance.gender),
            hair_style: appearance.hair_style,
            hair_color: appearance.hair_color,
            body_sprite: appearance.body_sprite,
        }),
        class: CharacterClass::from_raw(raw.class),
        is_hidden: raw.is_hidden,
        is_action_restricted: raw.is_action_restricted,
        is_blinded: raw.is_blinded,
        is_casting: raw.is_casting,
        is_walking: raw.is_walking,
        gold: raw.gold,
        weight: raw.weight,
        max_weight: raw.max_weight,
        progression: progression(raw, raw.pane_progression),
        stats: CharacterStats {
            stat_points: 0,
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
        inventory: raw.inventory_available.then(|| inventory(&raw.inventory)),
        equipment: raw.equipment_available.then(|| equipment(&raw.equipment)),
        spellbook: raw
            .spellbook_available
            .then(|| spellbook(&raw.spellbook, tick_ms)),
        skillbook: raw
            .skillbook_available
            .then(|| skillbook(&raw.skillbook, tick_ms)),
        effects: raw.effects.map(effects),
    }
}

fn effects(raw: RawEffects) -> Vec<Effect> {
    let mut effects = raw
        .effects
        .into_iter()
        .flatten()
        .map(|effect| Effect {
            icon: effect.icon,
            duration: EffectDuration::from_raw(effect.duration)
                .expect("captured effect duration is valid"),
        })
        .collect::<Vec<_>>();
    effects.sort_unstable_by_key(|effect| effect.icon);
    effects
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

fn inventory(raw: &RawInventory) -> Vec<InventoryItem> {
    raw.items
        .iter()
        .copied()
        .flatten()
        .map(collections::inventory_item)
        .collect()
}

fn equipment(raw: &RawEquipment) -> Vec<EquipmentItem> {
    raw.items
        .iter()
        .copied()
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

fn spellbook(raw: &RawSpellbook, tick_ms: u32) -> Vec<Spell> {
    raw.spells
        .iter()
        .copied()
        .flatten()
        .map(|spell| collections::spell(spell, tick_ms))
        .collect()
}

fn skillbook(raw: &RawSkillbook, tick_ms: u32) -> Vec<Skill> {
    raw.skills
        .iter()
        .copied()
        .flatten()
        .map(|raw_skill| collections::skill_model(raw_skill, tick_ms))
        .collect()
}

fn text<const N: usize>(raw: RawClientText<N>) -> Option<String> {
    client_text::decode(&raw.bytes[..usize::from(raw.length)])
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
