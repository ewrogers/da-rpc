use super::EventObservation;
use crate::snapshot_api::EffectDuration;
use darpc_model::Effect;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A spell-effect icon became active.
pub(crate) struct EffectAdded {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
    /// Relative remaining-duration color band, not an exact time.
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
/// A spell-effect icon is no longer active.
pub(crate) struct EffectRemoved {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
}

impl EffectRemoved {
    pub(super) const fn new(observation: EventObservation, icon: u16) -> Self {
        Self { observation, icon }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// The relative remaining-duration band changed for an active icon.
pub(crate) struct EffectChanged {
    pub(super) observation: EventObservation,
    /// Client spell-effect icon identifier.
    icon: u16,
    /// New relative remaining-duration color band, not an exact time.
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
