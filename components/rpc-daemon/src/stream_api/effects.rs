use super::EventObservation;
use crate::snapshot_api::EffectDuration;
use darpc_model::Effect;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct EffectAdded {
    pub(super) observation: EventObservation,
    icon: u16,
    duration: EffectDuration,
}

impl EffectAdded {
    pub(super) fn new(observation: EventObservation, effect: Effect) -> Self {
        Self {
            observation,
            icon: effect.icon,
            duration: effect.duration.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct EffectRemoved {
    pub(super) observation: EventObservation,
    icon: u16,
}

impl EffectRemoved {
    pub(super) const fn new(observation: EventObservation, icon: u16) -> Self {
        Self { observation, icon }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct EffectChanged {
    pub(super) observation: EventObservation,
    icon: u16,
    duration: EffectDuration,
}

impl EffectChanged {
    pub(super) fn new(observation: EventObservation, effect: Effect) -> Self {
        Self {
            observation,
            icon: effect.icon,
            duration: effect.duration.into(),
        }
    }
}
