use super::ObservationMetadata;
use darpc_model::{
    ClientSnapshot as ModelClientSnapshot, Effect as ModelEffect,
    EffectDuration as ModelEffectDuration,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Effects {
    observation: ObservationMetadata,
    effects: Option<Vec<Effect>>,
}

impl Effects {
    pub(crate) fn from_model(pid: u32, snapshot: &ModelClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            effects: snapshot.character.as_ref().and_then(|character| {
                character
                    .effects
                    .as_ref()
                    .map(|effects| effects.iter().copied().map(Effect::from).collect())
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct Effect {
    icon: u16,
    /// Relative remaining-duration band, not an exact time value.
    duration: EffectDuration,
}

impl From<ModelEffect> for Effect {
    fn from(value: ModelEffect) -> Self {
        Self {
            icon: value.icon,
            duration: value.duration.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EffectDuration {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    White,
}

impl From<ModelEffectDuration> for EffectDuration {
    fn from(value: ModelEffectDuration) -> Self {
        match value {
            ModelEffectDuration::Blue => Self::Blue,
            ModelEffectDuration::Green => Self::Green,
            ModelEffectDuration::Yellow => Self::Yellow,
            ModelEffectDuration::Orange => Self::Orange,
            ModelEffectDuration::Red => Self::Red,
            ModelEffectDuration::White => Self::White,
        }
    }
}
