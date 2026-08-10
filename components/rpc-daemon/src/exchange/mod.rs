use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeSnapshot {
    observation: ObservationMetadata,
    /// Current player exchange, or null when no exchange window is open.
    exchange: Option<ExchangeState>,
}

impl ExchangeSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            exchange: snapshot.exchange.as_ref().map(ExchangeState::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeState {
    id: u32,
    partner: String,
    local: ExchangeOffer,
    other: ExchangeOffer,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeOffer {
    items: Vec<ExchangeItem>,
    gold: u32,
    accepted: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeItem {
    index: u8,
    sprite: u16,
    dye_color: u8,
    quantity: u8,
    name: String,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExchangeParty {
    Local,
    Other,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeOpened {
    pub(crate) observation: EventObservation,
    exchange: ExchangeState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeItemAdded {
    pub(crate) observation: EventObservation,
    party: ExchangeParty,
    item: ExchangeItem,
    exchange: ExchangeState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeGoldChanged {
    pub(crate) observation: EventObservation,
    party: ExchangeParty,
    gold: u32,
    exchange: ExchangeState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeAccepted {
    pub(crate) observation: EventObservation,
    party: ExchangeParty,
    message: String,
    exchange: ExchangeState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeCompleted {
    pub(crate) observation: EventObservation,
    message: String,
    exchange: ExchangeState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ExchangeCancelled {
    pub(crate) observation: EventObservation,
    message: String,
    exchange: ExchangeState,
}

impl From<&model::ExchangeState> for ExchangeState {
    fn from(value: &model::ExchangeState) -> Self {
        Self {
            id: value.id,
            partner: value.partner.clone(),
            local: ExchangeOffer::from(&value.local),
            other: ExchangeOffer::from(&value.other),
        }
    }
}

impl From<&model::ExchangeOffer> for ExchangeOffer {
    fn from(value: &model::ExchangeOffer) -> Self {
        Self {
            items: value.items.iter().map(ExchangeItem::from).collect(),
            gold: value.gold,
            accepted: value.accepted,
        }
    }
}

impl From<&model::ExchangeItem> for ExchangeItem {
    fn from(value: &model::ExchangeItem) -> Self {
        Self {
            index: value.index,
            sprite: value.sprite,
            dye_color: value.dye_color,
            quantity: value.quantity.unwrap_or(1),
            name: value.name.clone(),
        }
    }
}

impl From<model::ExchangeParty> for ExchangeParty {
    fn from(value: model::ExchangeParty) -> Self {
        match value {
            model::ExchangeParty::Local => Self::Local,
            model::ExchangeParty::Other => Self::Other,
        }
    }
}

impl ExchangeOpened {
    pub(crate) fn new(observation: EventObservation, state: model::ExchangeState) -> Self {
        Self {
            observation,
            exchange: ExchangeState::from(&state),
        }
    }
}

impl ExchangeItemAdded {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::ExchangeState,
        party: model::ExchangeParty,
        item: model::ExchangeItem,
    ) -> Self {
        Self {
            observation,
            party: party.into(),
            item: ExchangeItem::from(&item),
            exchange: ExchangeState::from(&state),
        }
    }
}

impl ExchangeGoldChanged {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::ExchangeState,
        party: model::ExchangeParty,
        gold: u32,
    ) -> Self {
        Self {
            observation,
            party: party.into(),
            gold,
            exchange: ExchangeState::from(&state),
        }
    }
}

impl ExchangeAccepted {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::ExchangeState,
        party: model::ExchangeParty,
        message: String,
    ) -> Self {
        Self {
            observation,
            party: party.into(),
            message,
            exchange: ExchangeState::from(&state),
        }
    }
}

impl ExchangeCompleted {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::ExchangeState,
        message: String,
    ) -> Self {
        Self {
            observation,
            message,
            exchange: ExchangeState::from(&state),
        }
    }
}

impl ExchangeCancelled {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::ExchangeState,
        message: String,
    ) -> Self {
        Self {
            observation,
            message,
            exchange: ExchangeState::from(&state),
        }
    }
}
