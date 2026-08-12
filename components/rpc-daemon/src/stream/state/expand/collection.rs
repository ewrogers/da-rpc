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
            let payload = SpellSlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Spell::from),
                after: update.after.as_ref().map(Spell::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::SpellAdded(payload),
                CollectionChange::Removed => ClientEvent::SpellRemoved(payload),
                CollectionChange::Changed => ClientEvent::SpellChanged(payload),
            });
            events
        }
        StateUpdate::Skillbook(update) => {
            let change = update.change;
            let payload = SkillSlotChanged {
                observation,
                batch_index: u16::from(update.batch_index),
                batch_count: u16::from(update.batch_count),
                slot: update.slot,
                before: update.before.as_ref().map(Skill::from),
                after: update.after.as_ref().map(Skill::from),
            };
            events.push(match change {
                CollectionChange::Added => ClientEvent::SkillAdded(payload),
                CollectionChange::Removed => ClientEvent::SkillRemoved(payload),
                CollectionChange::Changed => ClientEvent::SkillChanged(payload),
            });
            events
        }
        _ => unreachable!("collection expansion received a non-collection update"),
    }
}
