use super::{ClientEvent, EventObservation, TilePosition};
use crate::state::Direction;
use darpc_model::ActionUpdate;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ItemUsed {
    pub(super) observation: EventObservation,
    slot: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ItemDropped {
    pub(super) observation: EventObservation,
    slot: u8,
    quantity: u32,
    destination: TilePosition,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ItemGiven {
    pub(super) observation: EventObservation,
    slot: u8,
    quantity: u32,
    target_id: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GoldDropped {
    pub(super) observation: EventObservation,
    amount: u32,
    destination: TilePosition,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct GoldGiven {
    pub(super) observation: EventObservation,
    amount: u32,
    target_id: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ItemPickedUp {
    pub(super) observation: EventObservation,
    destination_slot: u8,
    position: TilePosition,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct EquipmentUnequipped {
    pub(super) observation: EventObservation,
    slot: &'static str,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct Emoted {
    pub(super) observation: EventObservation,
    code: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct Turned {
    pub(super) observation: EventObservation,
    direction: Direction,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ClientResync {
    pub(super) observation: EventObservation,
    resync_id: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ClientResyncCompleted {
    pub(super) observation: EventObservation,
    resync_id: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ClientResyncTimedOut {
    pub(super) observation: EventObservation,
    resync_id: u32,
}

pub(super) fn expand_action(observation: EventObservation, update: ActionUpdate) -> ClientEvent {
    match update {
        ActionUpdate::ItemUsed { slot } => ClientEvent::ItemUsed(ItemUsed { observation, slot }),
        ActionUpdate::ItemDropped {
            slot,
            quantity,
            position,
        } => ClientEvent::ItemDropped(ItemDropped {
            observation,
            slot,
            quantity,
            destination: position.into(),
        }),
        ActionUpdate::ItemGiven {
            slot,
            quantity,
            object_id,
        } => ClientEvent::ItemGiven(ItemGiven {
            observation,
            slot,
            quantity,
            target_id: object_id,
        }),
        ActionUpdate::GoldDropped { amount, position } => ClientEvent::GoldDropped(GoldDropped {
            observation,
            amount,
            destination: position.into(),
        }),
        ActionUpdate::GoldGiven { amount, object_id } => ClientEvent::GoldGiven(GoldGiven {
            observation,
            amount,
            target_id: object_id,
        }),
        ActionUpdate::ItemPickedUp {
            destination_slot,
            position,
        } => ClientEvent::ItemPickedUp(ItemPickedUp {
            observation,
            destination_slot,
            position: position.into(),
        }),
        ActionUpdate::EquipmentUnequipped { slot } => {
            ClientEvent::EquipmentUnequipped(EquipmentUnequipped {
                observation,
                slot: slot.as_str(),
            })
        }
        ActionUpdate::Emoted { code } => ClientEvent::Emoted(Emoted { observation, code }),
        ActionUpdate::Turned { direction } => ClientEvent::Turned(Turned {
            observation,
            direction: direction.into(),
        }),
        ActionUpdate::Resync { resync_id } => ClientEvent::ClientResync(ClientResync {
            observation,
            resync_id,
        }),
        ActionUpdate::ResyncCompleted { resync_id } => {
            ClientEvent::ClientResyncCompleted(ClientResyncCompleted {
                observation,
                resync_id,
            })
        }
        ActionUpdate::ResyncTimedOut { resync_id } => {
            ClientEvent::ClientResyncTimedOut(ClientResyncTimedOut {
                observation,
                resync_id,
            })
        }
    }
}
