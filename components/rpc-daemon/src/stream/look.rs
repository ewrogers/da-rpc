use super::EventObservation;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct LookResult {
    pub(super) observation: EventObservation,
    command_id: u32,
    target: LookTarget,
    text: String,
}

impl LookResult {
    pub(crate) fn from_model(
        observation: EventObservation,
        result: darpc_model::LookResult,
    ) -> Self {
        Self {
            observation,
            command_id: result.command_id,
            target: result.target.into(),
            text: result.text,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LookTarget {
    Ahead,
    Tile { x: u16, y: u16 },
}

impl From<darpc_model::LookTarget> for LookTarget {
    fn from(value: darpc_model::LookTarget) -> Self {
        match value {
            darpc_model::LookTarget::Ahead => Self::Ahead,
            darpc_model::LookTarget::Tile { x, y } => Self::Tile { x, y },
        }
    }
}
