use super::*;

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A living entity started a body animation.
pub(crate) struct EntityAnimated {
    pub(super) observation: EventObservation,
    entity: WorldObject,
    /// Client body-animation identifier.
    animation: u8,
    /// Initial timer value supplied to the client. This is not total animation length.
    initial_duration_ms: i32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A visual effect was attached to a living entity.
pub(crate) struct EntityEffect {
    pub(super) observation: EventObservation,
    entity: WorldObject,
    /// One-based client effect number, or the moving-effect selector sent by the server.
    effect: u16,
    /// Source entity for effects traveling to or displayed on the target.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<WorldObject>,
    /// Packet fallback frame interval. Moving effects use client data instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_interval_ms: Option<i16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A temporary damage meter was displayed over a living entity.
pub(crate) struct EntityDamaged {
    pub(super) observation: EventObservation,
    entity: WorldObject,
    /// Server-supplied remaining health percentage from 0 through 100.
    health_percent: u8,
}

#[derive(Clone, Copy)]
enum EntityCategory {
    Player,
    Monster,
    Mundane,
}

pub(super) fn expand_entity(
    observation: EventObservation,
    update: darpc_model::EntityUpdate,
) -> Option<ClientEvent> {
    let entity = match &update {
        darpc_model::EntityUpdate::Animated { entity, .. }
        | darpc_model::EntityUpdate::Effect { entity, .. }
        | darpc_model::EntityUpdate::Damaged { entity, .. } => entity,
    };
    let category = category(entity)?;
    Some(match update {
        darpc_model::EntityUpdate::Animated {
            entity,
            animation,
            duration_10ms,
        } => {
            let payload = EntityAnimated {
                observation,
                entity: WorldObject::from(&entity),
                animation,
                initial_duration_ms: i32::from((duration_10ms as i16).wrapping_mul(10)),
            };
            match category {
                EntityCategory::Player => ClientEvent::PlayerAnimated(payload),
                EntityCategory::Monster => ClientEvent::MonsterAnimated(payload),
                EntityCategory::Mundane => ClientEvent::MundaneAnimated(payload),
            }
        }
        darpc_model::EntityUpdate::Effect {
            entity,
            effect,
            source,
            frame_interval_ms,
        } => {
            let payload = EntityEffect {
                observation,
                entity: WorldObject::from(&entity),
                effect,
                source: source.as_ref().map(WorldObject::from),
                frame_interval_ms,
            };
            match category {
                EntityCategory::Player => ClientEvent::PlayerEffect(payload),
                EntityCategory::Monster => ClientEvent::MonsterEffect(payload),
                EntityCategory::Mundane => ClientEvent::MundaneEffect(payload),
            }
        }
        darpc_model::EntityUpdate::Damaged {
            entity,
            health_percent,
        } => {
            let payload = EntityDamaged {
                observation,
                entity: WorldObject::from(&entity),
                health_percent,
            };
            match category {
                EntityCategory::Player => ClientEvent::PlayerDamaged(payload),
                EntityCategory::Monster => ClientEvent::MonsterDamaged(payload),
                EntityCategory::Mundane => ClientEvent::MundaneDamaged(payload),
            }
        }
    })
}

fn category(entity: &darpc_model::WorldObject) -> Option<EntityCategory> {
    match entity {
        darpc_model::WorldObject::Player { .. } => Some(EntityCategory::Player),
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Monster,
            ..
        } => Some(EntityCategory::Monster),
        darpc_model::WorldObject::Creature {
            kind: CreatureKind::Npc,
            ..
        } => Some(EntityCategory::Mundane),
        darpc_model::WorldObject::Item { .. } => None,
    }
}
