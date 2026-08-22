use super::*;

#[derive(Clone, Copy)]
enum ObjectChangeKind {
    Appeared,
    Disappeared,
    Moved,
    DirectionChanged,
}

#[derive(Clone, Copy)]
enum ObjectCategory {
    Player,
    Monster,
    Npc,
    Item,
}

pub(super) fn expand(observation: EventObservation, update: ObjectUpdate) -> Option<ClientEvent> {
    let (kind, object) = match update {
        ObjectUpdate::Appeared(object) => (ObjectChangeKind::Appeared, object),
        ObjectUpdate::Disappeared(object) => (ObjectChangeKind::Disappeared, object),
        ObjectUpdate::Moved(object) => (ObjectChangeKind::Moved, object),
        ObjectUpdate::DirectionChanged(object) => (ObjectChangeKind::DirectionChanged, object),
    };
    let category = match &object {
        darpc_model::WorldObject::Player { .. } => ObjectCategory::Player,
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Monster,
            ..
        } => ObjectCategory::Monster,
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Npc,
            ..
        } => ObjectCategory::Npc,
        darpc_model::WorldObject::Item { .. } => ObjectCategory::Item,
    };
    let changed = ObjectChanged {
        observation,
        object: WorldObject::from(&object),
    };
    Some(match (category, kind) {
        (ObjectCategory::Player, ObjectChangeKind::Appeared) => {
            ClientEvent::PlayerAppeared(changed)
        }
        (ObjectCategory::Player, ObjectChangeKind::Disappeared) => {
            ClientEvent::PlayerDisappeared(changed)
        }
        (ObjectCategory::Player, ObjectChangeKind::Moved) => ClientEvent::PlayerMoved(changed),
        (ObjectCategory::Player, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::PlayerDirectionChanged(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Appeared) => {
            ClientEvent::MonsterAppeared(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Disappeared) => {
            ClientEvent::MonsterDisappeared(changed)
        }
        (ObjectCategory::Monster, ObjectChangeKind::Moved) => ClientEvent::MonsterMoved(changed),
        (ObjectCategory::Monster, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::MonsterDirectionChanged(changed)
        }
        (ObjectCategory::Npc, ObjectChangeKind::Appeared) => ClientEvent::MundaneAppeared(changed),
        (ObjectCategory::Npc, ObjectChangeKind::Disappeared) => {
            ClientEvent::MundaneDisappeared(changed)
        }
        (ObjectCategory::Npc, ObjectChangeKind::Moved) => ClientEvent::MundaneMoved(changed),
        (ObjectCategory::Npc, ObjectChangeKind::DirectionChanged) => {
            ClientEvent::MundaneDirectionChanged(changed)
        }
        (ObjectCategory::Item, ObjectChangeKind::Appeared) => ClientEvent::ItemAppeared(changed),
        (ObjectCategory::Item, ObjectChangeKind::Disappeared) => {
            ClientEvent::ItemDisappeared(changed)
        }
        (ObjectCategory::Item, ObjectChangeKind::Moved) => ClientEvent::ItemMoved(changed),
        (ObjectCategory::Item, ObjectChangeKind::DirectionChanged) => return None,
    })
}
