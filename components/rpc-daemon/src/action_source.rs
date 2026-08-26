use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ActionSource {
    Unknown,
    Client,
    Command { command_id: u32 },
}

impl From<darpc_model::ActionSource> for ActionSource {
    fn from(value: darpc_model::ActionSource) -> Self {
        match value {
            darpc_model::ActionSource::Unknown => Self::Unknown,
            darpc_model::ActionSource::Client => Self::Client,
            darpc_model::ActionSource::Command { command_id } => Self::Command { command_id },
        }
    }
}
