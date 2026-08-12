use super::*;

pub(super) fn expand(
    observation: EventObservation,
    update: darpc_model::StatusUpdate,
) -> Vec<ClientEvent> {
    let mut events = Vec::with_capacity(7);
    if let Some(core) = update.core {
        events.push(ClientEvent::StatsChanged(StatsChanged {
            observation: observation.clone(),
            strength: core.stats.strength,
            intelligence: core.stats.intelligence,
            wisdom: core.stats.wisdom,
            constitution: core.stats.constitution,
            dexterity: core.stats.dexterity,
        }));
        events.push(ClientEvent::WeightChanged(WeightChanged {
            observation: observation.clone(),
            weight: core.weight,
            max_weight: core.max_weight,
        }));
    }
    if update.core.is_some() || update.vitals.is_some() {
        events.push(ClientEvent::VitalsChanged(VitalsChanged {
            observation: observation.clone(),
            health: update.vitals.map(|value| value.health),
            max_health: update.core.map(|value| value.max_health),
            mana: update.vitals.map(|value| value.mana),
            max_mana: update.core.map(|value| value.max_mana),
        }));
    }
    if update.core.is_some() || update.progression.is_some() {
        events.push(ClientEvent::ProgressionChanged(ProgressionChanged {
            observation: observation.clone(),
            level: update.core.map(|value| value.level),
            ability_level: update.core.map(|value| value.ability_level),
            experience: update.progression.map(|value| value.experience),
            ability_points: update.progression.map(|value| value.ability_points),
            experience_to_next_level: update
                .progression
                .map(|value| value.experience_to_next_level),
            ability_to_next_level: update.progression.map(|value| value.ability_to_next_level),
        }));
    }
    if let Some(gold) = update.gold {
        events.push(ClientEvent::GoldChanged(GoldChanged {
            observation: observation.clone(),
            gold,
        }));
    }
    if let Some(modifiers) = update.modifiers {
        events.push(ClientEvent::ModifiersChanged(ModifiersChanged {
            observation: observation.clone(),
            armor_class: modifiers.armor_class,
            damage: modifiers.damage,
            hit: modifiers.hit,
            magic_resistance: modifiers.magic_resistance,
            attack_element: Element::from(modifiers.attack_element),
            defense_element: Element::from(modifiers.defense_element),
        }));
    }
    if let Some(is_blinded) = update.is_blinded {
        events.push(ClientEvent::BlindChanged(BlindChanged {
            observation: observation.clone(),
            is_blinded,
        }));
    }
    if let Some(is_action_restricted) = update.is_action_restricted {
        events.push(ClientEvent::ActionRestrictionChanged(
            ActionRestrictionChanged {
                observation,
                is_action_restricted,
            },
        ));
    }
    events
}
