use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct MessageDialogsSnapshot {
    observation: ObservationMetadata,
    state: MessageDialogsState,
}

impl MessageDialogsSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            state: MessageDialogsState::from(&snapshot.message_dialogs),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MessageDialogsState {
    /// Wrapping revision required by dismiss commands.
    revision: u32,
    dialogs: Vec<MessageDialog>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MessageDialog {
    /// Opaque identifier valid only with the containing revision.
    id: u32,
    /// Displayed dialog text, or null when client memory was unavailable.
    text: Option<String>,
    /// True when displayed text exceeded the capture limit.
    truncated: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct MessageDialogsChanged {
    pub(crate) observation: EventObservation,
    state: MessageDialogsState,
}

impl MessageDialogsChanged {
    pub(crate) fn new(observation: EventObservation, state: model::MessageDialogsState) -> Self {
        Self {
            observation,
            state: MessageDialogsState::from(&state),
        }
    }
}

impl From<&model::MessageDialogsState> for MessageDialogsState {
    fn from(value: &model::MessageDialogsState) -> Self {
        Self {
            revision: value.revision,
            dialogs: value.dialogs.iter().map(MessageDialog::from).collect(),
        }
    }
}

impl From<&model::MessageDialog> for MessageDialog {
    fn from(value: &model::MessageDialog) -> Self {
        Self {
            id: value.id,
            text: value.text.clone(),
            truncated: value.truncated,
        }
    }
}
