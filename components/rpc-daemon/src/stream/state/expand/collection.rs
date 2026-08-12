use super::*;

pub(super) fn expand(observation: EventObservation, update: StateUpdate) -> Vec<ClientEvent> {
    let mut events = Vec::with_capacity(1);
    match update {
        StateUpdate::Inventory(update) => {
            let change = update.change;
            let payload = InventorySlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(InventoryItem::from),
                after: update.after.as_ref().map(InventoryItem::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::ItemAdded(payload),
                CollectionChange::Removed => ClientEvent::ItemRemoved(payload),
                CollectionChange::Changed => ClientEvent::ItemChanged(payload),
            });
            events
        }
        StateUpdate::Spellbook(update) => {
            let change = update.change;
            let metadata_changed = update
                .before
                .as_ref()
                .zip(update.after.as_ref())
                .is_some_and(|(before, after)| spell_metadata_changed(before, after));
            let cooldown =
                update
                    .before
                    .as_ref()
                    .zip(update.after.as_ref())
                    .and_then(|(before, after)| {
                        cooldown_transition(before.cooldown, after.cooldown)
                            .map(|transition| (transition, after.name.clone()))
                    });
            let payload = SpellSlotChanged {
                observation: observation.clone(),
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Spell::from),
                after: update.after.as_ref().map(Spell::from),
            };
            match change {
                CollectionChange::Added => events.push(ClientEvent::SpellAdded(payload)),
                CollectionChange::Removed => events.push(ClientEvent::SpellRemoved(payload)),
                CollectionChange::Changed if metadata_changed => {
                    events.push(ClientEvent::SpellChanged(payload));
                }
                CollectionChange::Changed => {}
            }
            if change == CollectionChange::Changed
                && let Some((transition, name)) = cooldown
            {
                events.push(cooldown_event(
                    observation,
                    update.slot,
                    name,
                    transition,
                    AbilityKind::Spell,
                ));
            }
            events
        }
        StateUpdate::Skillbook(update) => {
            let change = update.change;
            let metadata_changed = update
                .before
                .as_ref()
                .zip(update.after.as_ref())
                .is_some_and(|(before, after)| skill_metadata_changed(before, after));
            let cooldown =
                update
                    .before
                    .as_ref()
                    .zip(update.after.as_ref())
                    .and_then(|(before, after)| {
                        cooldown_transition(before.cooldown, after.cooldown)
                            .map(|transition| (transition, after.name.clone()))
                    });
            let payload = SkillSlotChanged {
                observation: observation.clone(),
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Skill::from),
                after: update.after.as_ref().map(Skill::from),
            };
            match change {
                CollectionChange::Added => events.push(ClientEvent::SkillAdded(payload)),
                CollectionChange::Removed => events.push(ClientEvent::SkillRemoved(payload)),
                CollectionChange::Changed if metadata_changed => {
                    events.push(ClientEvent::SkillChanged(payload));
                }
                CollectionChange::Changed => {}
            }
            if change == CollectionChange::Changed
                && let Some((transition, name)) = cooldown
            {
                events.push(cooldown_event(
                    observation,
                    update.slot,
                    name,
                    transition,
                    AbilityKind::Skill,
                ));
            }
            events
        }
        _ => unreachable!("collection expansion received a non-collection update"),
    }
}

#[derive(Clone, Copy)]
enum AbilityKind {
    Skill,
    Spell,
}

#[derive(Clone, Copy)]
enum CooldownTransition {
    Started { remaining_ms: Option<u32> },
    Ready,
}

fn cooldown_transition(
    before: darpc_model::CooldownStatus,
    after: darpc_model::CooldownStatus,
) -> Option<CooldownTransition> {
    if before == after {
        return None;
    }
    if after.active {
        return Some(CooldownTransition::Started {
            remaining_ms: after.remaining_ms,
        });
    }
    before.active.then_some(CooldownTransition::Ready)
}

fn cooldown_event(
    observation: EventObservation,
    slot: u8,
    name: Option<String>,
    transition: CooldownTransition,
    kind: AbilityKind,
) -> ClientEvent {
    match (kind, transition) {
        (AbilityKind::Skill, CooldownTransition::Started { remaining_ms }) => {
            ClientEvent::SkillCooldown(CooldownStarted {
                observation,
                slot,
                name,
                remaining_ms,
            })
        }
        (AbilityKind::Spell, CooldownTransition::Started { remaining_ms }) => {
            ClientEvent::SpellCooldown(CooldownStarted {
                observation,
                slot,
                name,
                remaining_ms,
            })
        }
        (AbilityKind::Skill, CooldownTransition::Ready) => ClientEvent::SkillReady(AbilityReady {
            observation,
            slot,
            name,
        }),
        (AbilityKind::Spell, CooldownTransition::Ready) => ClientEvent::SpellReady(AbilityReady {
            observation,
            slot,
            name,
        }),
    }
}

fn skill_metadata_changed(before: &darpc_model::Skill, after: &darpc_model::Skill) -> bool {
    before.slot != after.slot
        || before.icon != after.icon
        || before.name != after.name
        || before.level != after.level
        || before.max_level != after.max_level
}

fn spell_metadata_changed(before: &darpc_model::Spell, after: &darpc_model::Spell) -> bool {
    before.slot != after.slot
        || before.icon != after.icon
        || before.name != after.name
        || before.level != after.level
        || before.max_level != after.max_level
        || before.lines != after.lines
        || before.target_type != after.target_type
        || before.prompt != after.prompt
}
